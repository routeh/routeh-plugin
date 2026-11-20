//! `desired` as the plugin reads it: the router's line into the plugin's
//! own types.
use crate::support::{desired, instance};
use routeh_plugin_sdk::Desired;
use serde::Deserialize;

#[test]
fn reads_the_routers_line_as_json_values() {
    let line = desired(
        "log_level = \"info\"",
        &[instance("vpn", "listen_port = 51820\nnote = \"home\"")],
    );
    let read: Desired = Desired::read(line.text.as_bytes()).unwrap();
    assert_eq!(read.digest, line.digest);
    assert_eq!(read.settings, serde_json::json!({"log_level": "info"}));
    let vpn = &read.instances["vpn"];
    assert_eq!(vpn.kind, "tunnel");
    assert_eq!(vpn.device, "tn_vpn");
    assert_eq!(vpn.mark, 257);
    assert_eq!(
        vpn.settings,
        serde_json::json!({"listen_port": 51820, "note": "home"})
    );
}

#[derive(Debug, Deserialize, PartialEq)]
struct Settings {
    log_level: String,
}

#[derive(Debug, Deserialize, PartialEq)]
struct Tunnel {
    listen_port: Option<u16>,
    note: Option<String>,
}

#[test]
fn reads_the_routers_line_into_the_plugins_types() {
    let line = desired(
        "log_level = \"error\"",
        &[instance("a", "listen_port = 1"), instance("b", "")],
    );
    let read: Desired<Settings, Tunnel> = Desired::read(line.text.as_bytes()).unwrap();
    assert_eq!(read.settings.log_level, "error");
    assert_eq!(
        read.instances["a"].settings,
        Tunnel {
            listen_port: Some(1),
            note: None
        }
    );
    assert_eq!(read.instances["b"].settings.listen_port, None);
    let keys: Vec<&str> = read.instances.keys().map(String::as_str).collect();
    assert_eq!(keys, ["a", "b"]);
}

#[test]
fn a_line_that_is_no_desired_is_refused_with_why() {
    let refused = |line: &str| Desired::<serde_json::Value>::read(line.as_bytes()).unwrap_err();
    assert_eq!(refused("not json").to_string(), "not a JSON object");
    assert_eq!(refused("[1]").to_string(), "not a JSON object");
    assert_eq!(
        refused(r#"{"action":{"id":7}}"#).to_string(),
        "not desired: action"
    );
    assert!(
        refused(r#"{"desired":{"settings":{}}}"#)
            .to_string()
            .starts_with("desired: missing field `digest`"),
    );
    // Settings of another shape than the plugin's own are refused too.
    let line = desired("log_level = \"info\"", &[]);
    let error = Desired::<Vec<u8>>::read(line.text.as_bytes()).unwrap_err();
    assert!(error.to_string().starts_with("desired: "), "{error}");
}
