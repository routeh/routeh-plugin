//! The channel between the router and a plugin's service (docs/PLUGINS.md,
//! The channel): what the router sends, what it takes from a plugin, and
//! how a line is cut.
use routeh_plugin_protocol::channel::{
    self, ChannelError, LINE_LIMIT, Lines, Message, Phase, Reading,
};
use routeh_plugin_protocol::{Manifest, Schema};

fn awg() -> Manifest {
    Manifest::parse(crate::AWG).unwrap()
}

fn settings(manifest: &Manifest, text: &str) -> routeh_plugin_protocol::Settings {
    let table: toml::Table = text.parse().unwrap();
    manifest.settings.check(&table).unwrap()
}

#[test]
fn desired_is_whole_deterministic_and_carries_its_digest() {
    let manifest = awg();
    let desired = channel::desired(&settings(&manifest, "log_level = \"info\""), &[]);
    let digest = desired.digest.clone();
    assert!(digest.starts_with("sha256:"), "{digest}");
    assert_eq!(digest.len(), "sha256:".len() + 64);
    assert_eq!(
        desired.text,
        format!(
            "{{\"desired\":{{\"digest\":\"{digest}\",\"instances\":{{}},\"settings\":{{\"log_level\":\"info\"}}}}}}\n"
        )
    );
    // The digest is that of what the plugin is to be, without itself.
    assert_eq!(
        digest,
        format!(
            "sha256:{}",
            routeh_plugin_protocol::sha256::hex(&routeh_plugin_protocol::sha256::sha256(
                b"{\"instances\":{},\"settings\":{\"log_level\":\"info\"}}"
            ))
        )
    );
    let again = channel::desired(&settings(&manifest, "log_level = \"info\""), &[]);
    assert_eq!(again, desired);
    let other = channel::desired(&settings(&manifest, "log_level = \"debug\""), &[]);
    assert_ne!(other.digest, desired.digest);
    assert_eq!(channel::digest_of(&desired.text), Some(digest));
    assert_eq!(channel::digest_of("{}"), None);
    assert_eq!(channel::digest_of("not json"), None);
}

#[test]
fn desired_of_a_plugin_without_settings_is_empty() {
    let desired = channel::desired(&Schema::default().check(&toml::Table::new()).unwrap(), &[]);
    assert!(
        desired
            .text
            .ends_with("\"instances\":{},\"settings\":{}}}\n"),
        "{}",
        desired.text
    );
}

#[test]
fn reads_hello() {
    let message = channel::read(
        br#"{"hello":{"protocol":1,"plugin":"awg","version":"0.3.1"}}"#,
        &awg(),
    )
    .unwrap();
    let Message::Hello(hello) = message else {
        panic!("{message:?}")
    };
    assert_eq!(hello.protocol, 1);
    assert_eq!(hello.plugin, "awg");
    assert_eq!(hello.version, "0.3.1");
}

#[test]
fn a_hello_of_another_plugin_or_protocol_is_refused() {
    assert_eq!(
        channel::read(
            br#"{"hello":{"protocol":1,"plugin":"wg","version":"1"}}"#,
            &awg()
        )
        .unwrap_err(),
        ChannelError::Refused("hello names plugin wg, not awg".into())
    );
    assert_eq!(
        channel::read(
            br#"{"hello":{"protocol":2,"plugin":"awg","version":"1"}}"#,
            &awg()
        )
        .unwrap_err(),
        ChannelError::Refused("protocol 2 is not 1".into())
    );
}

/// Before its first `desired` a plugin has converged to nothing: AmneziaWG's
/// awg-ctl ( `crates/awg-ctl/src/proto.rs`, `digest:
/// Option<String>`) says so with a `null` digest right after `hello`. That
/// is a state of no digest, which no `desired` is, not a broken line.
#[test]
fn a_state_before_any_desired_has_no_digest() {
    let Message::State(state) = channel::read(
        br#"{"state":{"digest":null,"settled":true,"values":{"peers_up":0}}}"#,
        &awg(),
    )
    .unwrap() else {
        panic!()
    };
    assert_eq!(state.digest, "");
    assert!(state.settled);
}

#[test]
fn reads_state_keeping_only_declared_values() {
    let line = br#"{"state":{"digest":"sha256:5e1d","settled":true,
        "values":{"peers_up":1,"unknown":7},
        "instances":{"vpn":{"phase":"up","message":"","peer":"203.0.113.7:51820",
          "values":{"last_handshake":1795012345,"junk_packets":"many","extra":1}}}}}"#;
    let line: Vec<u8> = line.iter().copied().filter(|b| *b != b'\n').collect();
    let Message::State(state) = channel::read(&line, &awg()).unwrap() else {
        panic!()
    };
    assert_eq!(state.digest, "sha256:5e1d");
    assert!(state.settled);
    assert_eq!(
        state.values,
        vec![("peers_up".to_owned(), Reading::Count(1))]
    );
    let vpn = &state.instances["vpn"];
    assert_eq!(vpn.phase, Phase::Up);
    assert_eq!(vpn.peer.as_deref(), Some("203.0.113.7:51820"));
    // A value of the wrong type is missing; an undeclared one is dropped.
    assert_eq!(
        vpn.values,
        vec![("last_handshake".to_owned(), Reading::Time(1_795_012_345))]
    );
    assert_eq!(state.dropped, vec!["unknown", "vpn.extra"]);
    assert_eq!(state.wrong, vec!["vpn.junk_packets"]);
}

#[test]
fn reads_every_type_of_value() {
    let manifest = Manifest::parse(&crate::minimal(
        "\n[[status]]\nkey = \"t\"\nlabel = \"T\"\ntype = \"text\"\n\
         [[status]]\nkey = \"b\"\nlabel = \"B\"\ntype = \"bytes\"\n\
         [[status]]\nkey = \"d\"\nlabel = \"D\"\ntype = \"duration\"\n\
         [[status]]\nkey = \"a\"\nlabel = \"A\"\ntype = \"address\"\n\
         [[status]]\nkey = \"z\"\nlabel = \"Z\"\ntype = \"address\"\n\
         [[status]]\nkey = \"n\"\nlabel = \"N\"\ntype = \"count\"\n",
    ))
    .unwrap();
    let Message::State(state) = channel::read(
        br#"{"state":{"digest":"sha256:0","settled":false,"values":{"t":"hi","b":12,"d":30,"a":"fd00::1","z":"nowhere","n":-1}}}"#,
        &manifest,
    )
    .unwrap() else {
        panic!()
    };
    assert!(!state.settled);
    assert!(state.instances.is_empty());
    assert_eq!(
        state.values,
        vec![
            ("a".to_owned(), Reading::Address("fd00::1".parse().unwrap())),
            ("b".to_owned(), Reading::Bytes(12)),
            ("d".to_owned(), Reading::Duration(30)),
            ("t".to_owned(), Reading::Text("hi".into())),
        ]
    );
    assert_eq!(state.wrong, vec!["n", "z"]);
}

#[test]
fn an_instance_in_error_and_every_phase() {
    for (word, phase) in [
        ("starting", Phase::Starting),
        ("connecting", Phase::Connecting),
        ("up", Phase::Up),
        ("down", Phase::Down),
        ("error", Phase::Error),
    ] {
        let line = format!(
            r#"{{"state":{{"digest":"d","settled":true,"instances":{{"x":{{"phase":"{word}","message":"m"}}}}}}}}"#
        );
        let Message::State(state) = channel::read(line.as_bytes(), &awg()).unwrap() else {
            panic!()
        };
        assert_eq!(state.instances["x"].phase, phase);
        assert_eq!(state.instances["x"].message, "m");
        assert_eq!(state.instances["x"].peer, None);
        assert_eq!(state.in_error(), phase == Phase::Error);
        assert_eq!(
            state.first_error().map(|(key, _)| key.as_str()),
            (phase == Phase::Error).then_some("x")
        );
    }
}

#[test]
fn refuses_what_is_no_message() {
    for (line, reason) in [
        (&b"not json"[..], "not a JSON object"),
        (br#"[1]"#, "not a JSON object"),
        (br#"{}"#, "neither hello, state nor done"),
        (br#"{"desired":{}}"#, "neither hello, state nor done"),
        (
            br#"{"hello":{"protocol":1}}"#,
            "hello: missing field `plugin`",
        ),
        (
            br#"{"state":{"settled":true}}"#,
            "state: missing field `digest`",
        ),
        (
            br#"{"state":{"digest":"d","settled":true,"instances":{"x":{"phase":"gone"}}}}"#,
            "state: instance x: unknown phase gone",
        ),
    ] {
        let error = channel::read(line, &awg()).unwrap_err();
        let ChannelError::Refused(said) = &error else {
            panic!("{error:?}")
        };
        assert!(said.starts_with(reason), "{said} for {line:?}");
    }
}

#[test]
fn lines_are_cut_at_newlines_and_bounded() {
    let mut lines = Lines::new(LINE_LIMIT);
    lines.push(b"{\"a\":1}\n{\"b\"");
    assert_eq!(lines.next_line().unwrap(), Some(b"{\"a\":1}".to_vec()));
    assert_eq!(lines.next_line().unwrap(), None);
    lines.push(b":2}\r\n");
    assert_eq!(lines.next_line().unwrap(), Some(b"{\"b\":2}".to_vec()));
    assert_eq!(lines.next_line().unwrap(), None);

    let mut lines = Lines::new(8);
    lines.push(b"12345678\n");
    assert_eq!(lines.next_line().unwrap(), Some(b"12345678".to_vec()));
    lines.push(b"123456789");
    assert_eq!(lines.next_line().unwrap_err(), ChannelError::TooLong(8));
    assert_eq!(
        ChannelError::TooLong(8).to_string(),
        "a line is longer than 8 bytes"
    );
}

#[test]
fn a_line_without_limit_is_read_whole() {
    let mut lines = Lines::unbounded();
    let long = vec![b'x'; LINE_LIMIT * 3];
    lines.push(&long);
    lines.push(b"\n");
    assert_eq!(lines.next_line().unwrap().unwrap().len(), LINE_LIMIT * 3);
}

#[test]
fn hello_and_state_are_written_as_a_plugin_writes_them() {
    assert_eq!(
        channel::hello_line("demo", "1.0"),
        "{\"hello\":{\"plugin\":\"demo\",\"protocol\":1,\"version\":\"1.0\"}}\n"
    );
    let manifest = Manifest::parse(crate::MINIMAL).unwrap();
    let line = channel::state_line("sha256:x", true);
    let Message::State(state) = channel::read(line.trim_end().as_bytes(), &manifest).unwrap()
    else {
        panic!()
    };
    assert_eq!(state.digest, "sha256:x");
    assert!(state.settled);
}

#[test]
fn a_bare_reading_keeps_what_an_apply_needs() {
    let Message::State(state) = channel::read_bare(
        br#"{"state":{"digest":"d","settled":true,"values":{"peers_up":1},"instances":{"x":{"phase":"error"}}}}"#,
        "awg",
    )
    .unwrap() else {
        panic!()
    };
    assert!(state.values.is_empty());
    assert_eq!(state.dropped, ["peers_up"]);
    assert!(state.in_error());
    assert!(
        channel::read_bare(
            br#"{"hello":{"protocol":1,"plugin":"wg","version":"1"}}"#,
            "awg"
        )
        .is_err()
    );
}

#[test]
fn desired_holds_every_instance_by_key_with_its_device_mark_and_settings() {
    let none = Schema::default().check(&toml::Table::new()).unwrap();
    let instance = |key: &str, device: &str, mark: u32, settings: &str| channel::Instance {
        key: key.to_owned(),
        device: device.to_owned(),
        mark,
        settings: settings.parse().unwrap(),
    };
    let desired = channel::desired(
        &none,
        &[
            instance("vpn", "tn_vpn", 256, "port = 51820\nhost = \"a\""),
            instance("b", "tn_b", 257, ""),
        ],
    );
    let body = r#"{"instances":{"b":{"device":"tn_b","kind":"tunnel","mark":257,"settings":{}},"vpn":{"device":"tn_vpn","kind":"tunnel","mark":256,"settings":{"host":"a","port":51820}}},"settings":{}}"#;
    let digest = format!(
        "sha256:{}",
        routeh_plugin_protocol::sha256::hex(&routeh_plugin_protocol::sha256::sha256(
            body.as_bytes()
        ))
    );
    assert_eq!(desired.digest, digest);
    assert_eq!(
        desired.text,
        format!(
            "{{\"desired\":{{\"digest\":\"{digest}\",{}}}}}\n",
            &body[1..body.len() - 1]
        )
    );
    // What a plugin reads of it: each instance's key and device.
    assert_eq!(
        channel::instances_of(&desired.text),
        Some(vec![
            ("b".to_owned(), "tn_b".to_owned()),
            ("vpn".to_owned(), "tn_vpn".to_owned())
        ])
    );
    assert_eq!(
        channel::instances_of(&channel::desired(&none, &[]).text),
        Some(Vec::new())
    );
    for text in ["not json", "{}", r#"{"desired":{"instances":{"x":{}}}}"#] {
        assert_eq!(channel::instances_of(text), None, "{text}");
    }
}

#[test]
fn a_state_line_says_each_instance_with_its_phase() {
    let line = channel::state_line_with(
        "sha256:x",
        true,
        &[("vpn", Phase::Up, ""), ("b", Phase::Error, "no such kind")],
    );
    assert_eq!(
        line,
        "{\"state\":{\"digest\":\"sha256:x\",\"instances\":{\"b\":{\"message\":\"no such kind\",\"phase\":\"error\"},\"vpn\":{\"message\":\"\",\"phase\":\"up\"}},\"settled\":true}}\n"
    );
    let Message::State(state) = channel::read_bare(line.trim_end().as_bytes(), "awg").unwrap()
    else {
        panic!()
    };
    assert!(state.in_error());
    assert_eq!(state.instances["vpn"].phase, Phase::Up);
    for phase in [
        Phase::Starting,
        Phase::Connecting,
        Phase::Up,
        Phase::Down,
        Phase::Error,
    ] {
        let line = channel::state_line_with("d", false, &[("k", phase, "")]);
        let Message::State(state) = channel::read_bare(line.trim_end().as_bytes(), "x").unwrap()
        else {
            panic!()
        };
        assert_eq!(state.instances["k"].phase, phase);
    }
}

#[test]
fn a_plugin_that_goes_off_is_told_its_settings_without_an_instance() {
    let manifest = awg();
    let settings = settings(&manifest, "log_level = \"info\"");
    let with = channel::desired(
        &settings,
        &[channel::Instance {
            key: "vpn".to_owned(),
            device: "tn_vpn".to_owned(),
            mark: 256,
            settings: toml::Table::new(),
        }],
    );
    // What it is told before its service is stopped: what it was told,
    // with nothing to make.
    assert_eq!(
        channel::emptied(&with.text),
        Some(channel::desired(&settings, &[]))
    );
    assert_eq!(
        channel::emptied(&channel::desired(&settings, &[]).text),
        Some(channel::desired(&settings, &[]))
    );
    for text in ["not json", "{}", r#"{"desired":{"instances":{}}}"#] {
        assert_eq!(channel::emptied(text), None, "{text}");
    }
}
