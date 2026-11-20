//! Values of the document checked against a schema, and their canonical
//! form: what Save writes back.
use routeh_plugin_protocol::{Manifest, Schema, Value};
use std::net::IpAddr;

fn awg() -> Schema {
    Manifest::parse(crate::AWG)
        .unwrap()
        .tunnel
        .unwrap()
        .settings
}

/// A schema of one field `f` declared as `spec`.
fn one(spec: &str) -> Schema {
    Manifest::parse(&crate::kind(&format!("f = {spec}"), &["f"]))
        .unwrap()
        .tunnel
        .unwrap()
        .settings
}

fn table(text: &str) -> toml::Table {
    text.parse().unwrap()
}

/// `f = <value>` checked against `schema`: its value, or the refusal.
fn check(schema: &Schema, value: &str) -> Result<Value, String> {
    schema
        .check(&table(&format!("{}\nf = {value}", crate::ADDR_VALUE)))
        .map(|settings| settings.get("f").unwrap().clone())
        .map_err(|refusal| refusal.to_string())
}

const KEY: &str = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=";

#[test]
fn takes_the_example_and_writes_it_canonically() {
    let text = format!(
        "address = \"10.8.0.2/32\"\nlisten_port = 51820\nprivate_key = \"{KEY}\"\nendpoint = \"VPN.Example:51820\"\npeer_public_key = \"{}\"\nkeepalive = \"60s\"\njc = 4\nh1 = \"100000-200000\"\ni1 = \"<b 0x01><r 10>\"\n",
        KEY.trim_end_matches('=')
    );
    let settings = awg().check(&table(&text)).unwrap();
    assert_eq!(
        settings.get("address"),
        Some(&Value::Prefix("10.8.0.2".parse::<IpAddr>().unwrap(), 32))
    );
    assert_eq!(settings.get("jc"), Some(&Value::Integer(4)));
    assert_eq!(
        settings.get("h1"),
        Some(&Value::Range {
            lo: 100_000,
            hi: 200_000
        })
    );
    assert_eq!(settings.get("jmin"), None, "absent is off");
    let written = settings.to_toml();
    assert_eq!(
        written,
        table(&format!(
            "address = \"10.8.0.2/32\"\nendpoint = \"vpn.example:51820\"\nh1 = \"100000-200000\"\ni1 = \"<b 0x01><r 10>\"\njc = 4\nkeepalive = \"1m\"\nlisten_port = 51820\npeer_public_key = \"{KEY}\"\nprivate_key = \"{KEY}\"\n"
        )),
        "base64 padded, the host lowercase, the duration in its largest unit"
    );
    // What is written is taken again as it is.
    assert_eq!(awg().check(&written).unwrap(), settings);
}

#[test]
fn refuses_what_it_does_not_declare_and_wants_what_is_required() {
    assert_eq!(
        awg().check(&table("jc = 4")).unwrap_err().to_string(),
        "address: required"
    );
    let text = format!(
        "address = \"10.8.0.2/32\"\nprivate_key = \"{KEY}\"\nendpoint = \"h:1\"\npeer_public_key = \"{KEY}\"\njd = 1"
    );
    assert_eq!(
        awg().check(&table(&text)).unwrap_err().to_string(),
        "jd: unknown setting"
    );
}

#[test]
fn numbers_and_ranges_within_their_bounds() {
    let u8 = one("{ type = \"u8\", min = 2, max = 9, label = \"F\" }");
    assert_eq!(check(&u8, "9"), Ok(Value::Integer(9)));
    assert_eq!(check(&u8, "1").unwrap_err(), "f: 2 to 9");
    assert_eq!(check(&u8, "-1").unwrap_err(), "f: 2 to 9");
    assert_eq!(check(&u8, "\"3\"").unwrap_err(), "f: must be an integer");
    let range = one("{ type = \"range\", of = \"u16\", min = 5, label = \"F\" }");
    assert_eq!(check(&range, "\"7-7\""), Ok(Value::Range { lo: 7, hi: 7 }));
    for (text, reason) in [
        ("\"9-7\"", "f: the first number is above the second"),
        ("\"4-7\"", "f: 5 to 65535"),
        ("\"7-70000\"", "f: 5 to 65535"),
        (
            "\"07-9\"",
            "f: a range is two whole numbers with '-' between them (\"100-200\")",
        ),
        (
            "\"7\"",
            "f: a range is two whole numbers with '-' between them (\"100-200\")",
        ),
        ("7", "f: must be a string"),
    ] {
        assert_eq!(check(&range, text).unwrap_err(), reason, "{text}");
    }
    let port = one("{ type = \"port\", label = \"F\" }");
    assert_eq!(check(&port, "0").unwrap_err(), "f: 1 to 65535");
    assert_eq!(check(&port, "443"), Ok(Value::Port(443)));
    let duration = one("{ type = \"duration\", min = \"10s\", max = \"2h\", label = \"F\" }");
    assert_eq!(check(&duration, "\"120m\""), Ok(Value::Duration(7200)));
    assert_eq!(check(&duration, "\"3h\"").unwrap_err(), "f: 10s to 2h");
    assert_eq!(check(&duration, "\"5s\"").unwrap_err(), "f: 10s to 2h");
    assert_eq!(
        check(&duration, "\"1d\"").unwrap_err(),
        "f: a duration is a whole number and s, m or h (\"25s\")"
    );
}

#[test]
fn text_is_one_line_within_its_length_and_pattern() {
    let text = one("{ type = \"text\", max_len = 4, pattern = '[a-z]+', label = \"F\" }");
    assert_eq!(check(&text, "\"abcd\""), Ok(Value::Text("abcd".into())));
    assert_eq!(
        check(&text, "\"abcde\"").unwrap_err(),
        "f: at most 4 characters"
    );
    assert_eq!(
        check(&text, "\"ab1\"").unwrap_err(),
        "f: not of the form the plugin asks for"
    );
    assert_eq!(
        check(&text, "\"xab1\"").unwrap_err(),
        "f: not of the form the plugin asks for",
        "the whole value matches"
    );
    assert_eq!(
        check(&text, "\"a\\tb\"").unwrap_err(),
        "f: no control or invisible format characters"
    );
    assert_eq!(
        check(&text, "\"ab\\u202E\"").unwrap_err(),
        "f: no control or invisible format characters",
        "a right-to-left override would turn the text around unseen"
    );
    let template = one("{ type = \"template\", label = \"F\" }");
    assert_eq!(
        check(&template, "\"<a>\\n\\t<b>\""),
        Ok(Value::Text("<a>\n\t<b>".into()))
    );
    assert_eq!(
        check(&template, "\"a\\r\\n\"").unwrap_err(),
        "f: no control or invisible format characters but tab and newline"
    );
    let long = format!("\"{}\"", "x".repeat(4097));
    assert_eq!(
        check(&template, &long).unwrap_err(),
        "f: at most 4096 characters",
        "its default"
    );
}

#[test]
fn bytes_are_base64_of_their_length() {
    let bytes = one("{ type = \"bytes\", len = 32, secret = true, label = \"F\" }");
    assert_eq!(
        check(&bytes, &format!("\"{KEY}\"")),
        Ok(Value::Bytes((0..32).collect()))
    );
    assert_eq!(
        check(&bytes, "\"AAEC\"").unwrap_err(),
        "f: base64 of 32 bytes"
    );
    assert_eq!(
        check(&bytes, "\"not base64!\"").unwrap_err(),
        "f: base64 of 32 bytes"
    );
}

#[test]
fn addresses_and_hosts() {
    let v4 = one("{ type = \"ipv4_prefix\", label = \"F\" }");
    assert_eq!(
        check(&v4, "\"10.8.0.2/24\""),
        Ok(Value::Prefix("10.8.0.2".parse().unwrap(), 24)),
        "host bits stay: it is an interface's address"
    );
    for text in [
        "\"10.8.0.2\"",
        "\"10.8.0.2/33\"",
        "\"fd00::1/64\"",
        "\"10.8.0.2/024\"",
    ] {
        assert_eq!(
            check(&v4, text).unwrap_err(),
            "f: an IPv4 address with its prefix length (\"10.8.0.2/32\")",
            "{text}"
        );
    }
    let v6 = one("{ type = \"ipv6_prefix\", label = \"F\" }");
    assert_eq!(
        check(&v6, "\"FD00:0::1/64\""),
        Ok(Value::Prefix("fd00::1".parse().unwrap(), 64))
    );
    assert_eq!(
        check(&v6, "\"10.0.0.1/8\"").unwrap_err(),
        "f: an IPv6 address with its prefix length (\"fd00::2/64\")"
    );
    let address = one("{ type = \"address\", label = \"F\" }");
    assert_eq!(
        check(&address, "\"::1\""),
        Ok(Value::Address("::1".parse().unwrap()))
    );
    assert_eq!(
        check(&address, "\"10.0.0.1/8\"").unwrap_err(),
        "f: an IPv4 or IPv6 address"
    );
    let host = one("{ type = \"host_port\", label = \"F\" }");
    for (text, written) in [
        ("\"Vpn.Example.com:51820\"", "vpn.example.com:51820"),
        ("\"203.0.113.7:1\"", "203.0.113.7:1"),
        ("\"[FD00::1]:443\"", "[fd00::1]:443"),
    ] {
        let value = check(&host, text).unwrap();
        assert_eq!(value.to_toml().as_str(), Some(written));
    }
    for text in [
        "\"999.1.1.1:5\"",
        "\"1.2.3:5\"",
        "\"host\"",
        "\"host:0\"",
        "\"fd00::1:443\"",
        "\"-host:1\"",
        "\"a..b:1\"",
        "\"host:01\"",
        "\"ho_st:1\"",
    ] {
        assert_eq!(
            check(&host, text).unwrap_err(),
            "f: a host name or address and a port (\"vpn.example.com:51820\", \"[fd00::1]:443\")",
            "{text}"
        );
    }
}

#[test]
fn choices_lists_and_groups() {
    let choice = one("{ type = \"choice\", values = [\"udp\", \"tcp\"], label = \"F\" }");
    assert_eq!(check(&choice, "\"tcp\""), Ok(Value::Choice("tcp".into())));
    assert_eq!(check(&choice, "\"TCP\"").unwrap_err(), "f: one of udp, tcp");
    let list = one("{ type = \"list\", item = { type = \"port\" }, max_items = 2, label = \"F\" }");
    assert_eq!(
        check(&list, "[1, 2]"),
        Ok(Value::List(vec![Value::Port(1), Value::Port(2)]))
    );
    assert_eq!(check(&list, "[1, 2, 3]").unwrap_err(), "f: at most 2");
    assert_eq!(check(&list, "[1, 0]").unwrap_err(), "f.1: 1 to 65535");
    assert_eq!(check(&list, "1").unwrap_err(), "f: must be an array");
    let group = one(
        "{ type = \"group\", fields = { endpoint = { type = \"host_port\", required = true, label = \"Endpoint\" }, key = { type = \"bytes\", len = 32, label = \"Key\" } }, columns = [\"endpoint\", \"key\"], max_rows = 2, label = \"Peers\" }",
    );
    let rows = check(&group, "[{ endpoint = \"a:1\" }, { endpoint = \"b:2\", key = \"AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=\" }]").unwrap();
    let Value::Group(rows) = rows else {
        panic!("{rows:?}")
    };
    assert_eq!(rows.len(), 2);
    assert!(rows[1].get("key").is_some());
    assert_eq!(
        check(&group, "[{ key = \"x\" }]").unwrap_err(),
        "f.0.endpoint: required"
    );
    assert_eq!(
        check(&group, "[{ endpoint = \"a:1\", port = 1 }]").unwrap_err(),
        "f.0.port: unknown setting"
    );
    assert_eq!(check(&group, "[1]").unwrap_err(), "f.0: must be a table");
    assert_eq!(
        check(
            &group,
            "[{ endpoint = \"a:1\" }, { endpoint = \"a:1\" }, { endpoint = \"a:1\" }]"
        )
        .unwrap_err(),
        "f: at most 2"
    );
    let bools = one("{ type = \"bool\", label = \"F\" }");
    assert_eq!(check(&bools, "true"), Ok(Value::Bool(true)));
    assert_eq!(check(&bools, "1").unwrap_err(), "f: must be a boolean");
}

#[test]
fn the_two_constraints() {
    let schema = Manifest::parse(&format!(
        "{}\n[tunnel.constraints]\nexactly_one = [[\"password\", \"key\"]]\nrequires = {{ keepalive = \"password\" }}\n",
        crate::kind(
            "password = { type = \"text\", secret = true, label = \"Password\" }\nkey = { type = \"text\", label = \"Key\" }\nkeepalive = { type = \"duration\", label = \"Keepalive\" }",
            &["password", "key", "keepalive"]
        )
    ))
    .unwrap()
    .tunnel
    .unwrap()
    .settings;
    let with = |text: &str| table(&format!("{}\n{text}", crate::ADDR_VALUE));
    let refusal = |text: &str| schema.check(&with(text)).unwrap_err().to_string();
    assert_eq!(refusal(""), "password: one of Password and Key");
    assert_eq!(
        refusal("password = \"p\"\nkey = \"k\""),
        "key: only one of Password and Key"
    );
    assert_eq!(
        refusal("key = \"k\"\nkeepalive = \"5s\""),
        "keepalive: Keepalive needs Password"
    );
    assert!(
        schema
            .check(&with("password = \"p\"\nkeepalive = \"5s\""))
            .is_ok()
    );
}

#[test]
fn every_type_is_written_as_it_is_taken_again() {
    let schema = Manifest::parse(&crate::kind(
        "b = { type = \"bool\", label = \"B\" }\n\
         a = { type = \"address\", label = \"A\" }\n\
         z = { type = \"duration\", label = \"Z\" }\n\
         h = { type = \"duration\", label = \"H\" }\n\
         s = { type = \"duration\", label = \"S\" }\n\
         l = { type = \"list\", item = { type = \"ipv6_prefix\" }, label = \"L\" }\n\
         c = { type = \"choice\", values = [\"x\", \"y\"], label = \"C\" }\n\
         g = { type = \"group\", fields = { k = { type = \"bytes\", len = 2, label = \"K\" }, on = { type = \"bool\", label = \"On\" } }, columns = [\"k\", \"on\"], label = \"G\" }",
        &["b", "a", "z", "h", "s", "l", "c", "g"],
    ))
    .unwrap()
    .tunnel
    .unwrap()
    .settings;
    let given = table(
        "addr = \"10.0.0.2/32\"\nb = false\na = \"FD00::0:1\"\nz = \"0m\"\nh = \"7200s\"\ns = \"90s\"\nl = [\"FD00::1/64\"]\nc = \"y\"\ng = [{ k = \"AAE\", on = true }]",
    );
    let canonical = table(
        "addr = \"10.0.0.2/32\"\nb = false\na = \"fd00::1\"\nz = \"0s\"\nh = \"2h\"\ns = \"90s\"\nl = [\"fd00::1/64\"]\nc = \"y\"\ng = [{ k = \"AAE=\", on = true }]",
    );
    let settings = schema.check(&given).unwrap();
    assert_eq!(settings.to_toml(), canonical);
    assert_eq!(schema.check(&canonical).unwrap(), settings);
    assert_eq!(check(&one("{ type = \"group\", fields = { k = { type = \"bool\", label = \"K\" } }, columns = [\"k\"], label = \"F\" }"), "1").unwrap_err(), "f: must be an array");
}

#[test]
fn settings_are_written_in_the_form_order_as_document_text() {
    let schema = Manifest::parse(&crate::kind(
        "t = { type = \"template\", label = \"T\" }\n\
         g = { type = \"group\", fields = { z = { type = \"text\", label = \"Z\" }, a = { type = \"u8\", label = \"A\" } }, columns = [\"z\", \"a\"], label = \"G\" }\n\
         r = { type = \"range\", of = \"u32\", label = \"R\" }\n\
         k = { type = \"bytes\", len = 2, secret = true, label = \"K\" }\n\
         e = { type = \"host_port\", label = \"E\" }\n\
         l = { type = \"list\", item = { type = \"duration\" }, label = \"L\" }",
        &["t", "g", "r", "k", "e", "l"],
    ))
    .unwrap()
    .tunnel
    .unwrap()
    .settings;
    let settings = schema
        .check(&table(&format!(
            "{}\nl = [\"60s\", \"5s\"]\ne = \"[FD00::1]:443\"\nk = \"AAE\"\nr = \"1-2\"\ng = [{{ a = 1, z = 'say \"hi\" \\\\ there' }}]\nt = \"<a>\\n\\t<b>\"",
            crate::ADDR_VALUE
        )))
        .unwrap();
    let text = schema.write(&settings);
    assert_eq!(
        text,
        "addr = \"10.0.0.2/32\"\nt = \"<a>\\n\\t<b>\"\ng = [{ z = \"say \\\"hi\\\" \\\\\\\\ there\", a = 1 }]\nr = \"1-2\"\nk = \"AAE=\"\ne = \"[fd00::1]:443\"\nl = [\"1m\", \"5s\"]\n"
    );
    // The text is read back as the same settings.
    assert_eq!(schema.check(&table(&text)).unwrap(), settings);
}

#[test]
fn settings_never_print_their_values() {
    let settings = one("{ type = \"text\", secret = true, label = \"F\" }")
        .check(&table(&format!("{}\nf = \"hunter2\"", crate::ADDR_VALUE)))
        .unwrap();
    let printed = format!("{settings:?}");
    assert!(
        !printed.contains("hunter2") && !printed.contains("10.0.0.2"),
        "{printed}"
    );
    assert!(printed.contains("\"f\""), "{printed}");
}

#[test]
fn the_fields_of_each_role() {
    let schema = awg();
    assert_eq!(schema.address(true), Some("address"));
    assert_eq!(schema.address(false), None);
    assert_eq!(
        schema.listen(),
        Some(("listen_port", routeh_plugin_protocol::Protocol::Udp))
    );
    assert_eq!(schema.endpoint(), Some("endpoint"));
    assert_eq!(one("{ type = \"bool\", label = \"F\" }").endpoint(), None);
}
