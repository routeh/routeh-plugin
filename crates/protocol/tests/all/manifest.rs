//! A manifest as `install` reads it: the example of docs/TUNNELS.md.
use routeh_plugin_protocol::{FieldType, Manifest, Page, Protocol, Role, StatusType};
use std::time::Duration;

#[test]
fn reads_every_part_of_the_example() {
    let manifest = Manifest::parse(crate::AWG).unwrap();
    assert_eq!(manifest.id.as_str(), "awg");
    assert_eq!(manifest.name.as_str(), "AmneziaWG");
    assert_eq!(manifest.version, "0.3.1");
    assert_eq!(
        manifest.requires,
        [
            "kmod-udptunnel4",
            "kmod-udptunnel6",
            "kmod-crypto-lib-chacha20poly1305",
            "kmod-crypto-lib-curve25519"
        ]
    );
    assert_eq!(manifest.modules, ["amneziawg"]);
    assert_eq!(
        manifest.kernel.as_deref(),
        Some("6.12.112~67e205f26d0458c4d12f1fd315216398-r1")
    );
    assert_eq!(manifest.service.command, "bin/awg-ctl");
    assert_eq!(manifest.service.capabilities, ["CAP_NET_ADMIN"]);
    assert_eq!(manifest.service.settle_within, Duration::from_secs(10));

    assert_eq!(manifest.settings.keys().collect::<Vec<_>>(), ["log_level"]);
    assert_eq!(manifest.status.len(), 1);
    assert_eq!(manifest.status[0].key.as_str(), "peers_up");
    assert_eq!(manifest.status[0].kind, StatusType::Count);

    let tunnel = manifest.tunnel.as_ref().unwrap();
    assert_eq!(tunnel.link_kind, "amneziawg");
    assert_eq!(
        (tunnel.mtu.default, tunnel.mtu.min, tunnel.mtu.max),
        (1380, 1280, 1420)
    );
    let status: Vec<_> = tunnel
        .status
        .iter()
        .map(|s| (s.key.as_str(), s.label.as_str(), s.kind))
        .collect();
    assert_eq!(
        status,
        [
            ("last_handshake", "Last handshake", StatusType::Time),
            ("junk_packets", "Junk packets sent", StatusType::Count)
        ]
    );
    let form: Vec<_> = tunnel
        .form
        .iter()
        .map(|s| (s.title.as_str(), s.folded, s.fields.len()))
        .collect();
    assert_eq!(
        form,
        [
            ("Interface", false, 3),
            ("Peer", false, 3),
            ("Obfuscation", true, 8)
        ]
    );

    // The form gives the order a person reads the fields in.
    assert_eq!(
        tunnel.settings.keys().collect::<Vec<_>>(),
        [
            "address",
            "listen_port",
            "private_key",
            "endpoint",
            "peer_public_key",
            "keepalive",
            "jc",
            "jmin",
            "jmax",
            "s1",
            "h1",
            "h2",
            "i1",
            "header_protection_key"
        ]
    );
    let field = |key| tunnel.settings.field(key).unwrap();
    assert_eq!(field("address").role, Some(Role::Address));
    assert!(field("address").required);
    assert_eq!(field("listen_port").role, Some(Role::Listen(Protocol::Udp)));
    assert_eq!(field("endpoint").role, Some(Role::Endpoint));
    assert!(field("private_key").secret);
    assert!(!field("peer_public_key").secret);
    assert_eq!(field("private_key").label, "Private key");
    assert!(matches!(
        field("private_key").kind,
        FieldType::Bytes { len: 32 }
    ));
    assert!(matches!(
        field("jc").kind,
        FieldType::Integer {
            min: 0,
            max: 128,
            ..
        }
    ));
    assert!(matches!(
        field("jmin").kind,
        FieldType::Integer {
            min: 0,
            max: 65535,
            ..
        }
    ));
    assert!(matches!(
        field("h1").kind,
        FieldType::Range {
            min: 5,
            max: u32::MAX,
            ..
        }
    ));

    assert_eq!(manifest.api, 1);
    assert_eq!(
        manifest.page,
        Some(Page {
            title: "AmneziaWG".into(),
            icon: Some("shield".into()),
        })
    );
}

#[test]
fn the_least_a_manifest_says() {
    let manifest = Manifest::parse(crate::MINIMAL).unwrap();
    assert!(manifest.requires.is_empty() && manifest.modules.is_empty());
    assert_eq!(manifest.kernel, None);
    assert!(manifest.service.capabilities.is_empty());
    assert_eq!(
        manifest.service.settle_within,
        Duration::from_secs(10),
        "the default"
    );
    assert!(manifest.tunnel.is_none() && manifest.page.is_none() && manifest.status.is_empty());
    assert_eq!(manifest.settings.keys().count(), 0);
}

#[test]
fn a_page_may_go_without_an_icon() {
    let manifest =
        Manifest::parse(&crate::minimal("\n[page]\ntitle = \"AdGuard Home\"\n")).unwrap();
    let page = manifest.page.unwrap();
    assert_eq!(page.title, "AdGuard Home");
    assert_eq!(page.icon, None);
    assert!(manifest.actions.is_empty());
}

#[test]
fn a_manifest_names_the_api_it_is_written_for() {
    assert_eq!(routeh_plugin_protocol::API, 1);
    let with = |api: &str| crate::MINIMAL.replace("api = 1", api);
    assert_eq!(
        crate::refusal(&with("api = 2")),
        "api: this router speaks api 1"
    );
    assert_eq!(
        crate::refusal(&with("api = 0")),
        "api: this router speaks api 1"
    );
    assert_eq!(
        crate::refusal(&with("api = \"1\"")),
        "api: must be an integer"
    );
    assert_eq!(crate::refusal(&with("")), "api: required");
}

#[test]
fn a_syntax_error_says_where_it_is() {
    let refusal = crate::refusal("id = \"x\"\nname = \n");
    assert!(
        refusal.starts_with("manifest: line 2, column 8: "),
        "{refusal}"
    );
}
