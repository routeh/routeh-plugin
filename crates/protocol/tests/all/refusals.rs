//! What `install` refuses, each at the place of the manifest it is about.
use crate::{AWG, MINIMAL, kind, minimal, refusal};

#[test]
fn is_toml_with_no_key_it_does_not_know() {
    assert!(refusal("id = ").starts_with("manifest: "));
    // `colour` after `[service]` is a key of the service.
    assert_eq!(
        refusal(&minimal("colour = \"red\"\n")),
        "service.colour: unknown key"
    );
    assert_eq!(
        refusal(&format!("colour = \"red\"\n{MINIMAL}")),
        "colour: unknown key"
    );
    assert_eq!(refusal(&minimal("[unknown]\n")), "unknown: unknown key");
    assert_eq!(
        refusal(&MINIMAL.replace("command", "commands")),
        "service.command: required"
    );
    assert_eq!(
        refusal(&MINIMAL.replace("\"Demo\"", "3")),
        "name: must be a string"
    );
}

#[test]
fn names_itself() {
    assert_eq!(
        refusal(&MINIMAL.replace("\"demo\"", "\"Demo\"")),
        "id: a plugin's id is 1 to 16 lowercase letters, digits or underscores, starting with a letter"
    );
    for key in ["name", "policy", "mtu", "check"] {
        assert_eq!(
            refusal(&MINIMAL.replace("\"demo\"", &format!("\"{key}\""))),
            format!("id: \"{key}\" is a key every tunnel has")
        );
    }
    assert_eq!(
        refusal(&MINIMAL.replace("\"Demo\"", "\"\"")),
        "name: a display name is 1 to 64 characters, not only whitespace, without control or invisible format characters"
    );
    for version in ["", "1 0", ".1", &"1".repeat(33)] {
        assert_eq!(
            refusal(&MINIMAL.replace("\"1.0\"", &format!("\"{version}\""))),
            "version: 1 to 32 letters, digits, '.', '+', '~' or '-', starting with a letter or digit",
            "{version:?}"
        );
    }
    assert_eq!(
        refusal("name = \"x\"\nversion = \"1\"\napi = 1\n[service]\ncommand = \"x\"\n"),
        "id: required"
    );
}

#[test]
fn requires_firmware_packages_and_modules_by_name() {
    assert_eq!(
        refusal(&MINIMAL.replace("[service]", "requires = [\"Kmod\"]\n[service]")),
        "requires: \"Kmod\" is not a package name"
    );
    assert_eq!(
        refusal(&MINIMAL.replace("[service]", "requires = [\"a\", \"a\"]\n[service]")),
        "requires: \"a\" is named twice"
    );
    assert_eq!(
        refusal(&minimal("[modules]\nload = [\"amnezia/wg\"]\n")),
        "modules.load: \"amnezia/wg\" is not a module name"
    );
    assert_eq!(
        refusal(&minimal("[modules]\nload = [\"x\", \"x\"]\n")),
        "modules.load: \"x\" is named twice"
    );
    assert_eq!(refusal(&minimal("[modules]\n")), "modules.load: required");
    // Modules are built for one kernel: the `kernel` package's version.
    assert_eq!(
        refusal(&minimal("[modules]\nload = [\"x\"]\n")),
        "modules.kernel: required"
    );
    assert_eq!(
        refusal(&minimal("[modules]\nload = [\"x\"]\nkernel = \"6.12 x\"\n")),
        "modules.kernel: the version of the kernel package: 1 to 128 letters, digits, '.', '+', '~', '_' or '-'"
    );
}

#[test]
fn runs_one_program_of_its_bundle() {
    for command in ["/bin/sh", "../bin/x", "bin//x", "bin/./x", "bin/x y", ""] {
        assert_eq!(
            refusal(&MINIMAL.replace("\"bin/demo\"", &format!("\"{command}\""))),
            "service.command: a path inside the bundle: segments of letters, digits, '.', '_' or '-', not '.' or '..'",
            "{command:?}"
        );
    }
    assert_eq!(
        refusal(&minimal("capabilities = [\"CAP_EVERYTHING\"]\n")),
        "service.capabilities: \"CAP_EVERYTHING\" is not a capability of Linux"
    );
    assert_eq!(
        refusal(&minimal(
            "capabilities = [\"CAP_NET_RAW\", \"CAP_NET_RAW\"]\n"
        )),
        "service.capabilities: \"CAP_NET_RAW\" is named twice"
    );
    for (within, reason) in [
        ("\"0s\"", "service.settle_within: 1s to 1m"),
        ("\"2m\"", "service.settle_within: 1s to 1m"),
        (
            "\"10\"",
            "service.settle_within: a duration is a whole number and s, m or h (\"25s\")",
        ),
    ] {
        assert_eq!(
            refusal(&minimal(&format!("settle_within = {within}\n"))),
            reason
        );
    }
}

#[test]
fn a_tunnel_kind_has_its_device_and_its_mtu() {
    let without = |line: &str| AWG.replace(line, "");
    assert_eq!(
        refusal(&without("link_kind = \"amneziawg\"\n")),
        "tunnel.link_kind: required"
    );
    assert_eq!(
        refusal(&AWG.replace("\"amneziawg\"\nmtu", "\"Amnezia WG\"\nmtu")),
        "tunnel.link_kind: 1 to 16 lowercase letters, digits, '_' or '-', starting with a letter"
    );
    assert_eq!(
        refusal(&without(
            "mtu = { default = 1380, min = 1280, max = 1420 }\n"
        )),
        "tunnel.mtu: required"
    );
    for (mtu, reason) in [
        (
            "{ default = 1380, min = 1400, max = 1420 }",
            "tunnel.mtu: min ≤ default ≤ max",
        ),
        (
            "{ default = 1380, min = 1280, max = 1300 }",
            "tunnel.mtu: min ≤ default ≤ max",
        ),
        (
            "{ default = 60, min = 60, max = 1420 }",
            "tunnel.mtu: 68 to 65535",
        ),
        ("{ default = 1380, min = 1280 }", "tunnel.mtu.max: required"),
        (
            "{ default = 1380, min = 1280, max = 1420, step = 4 }",
            "tunnel.mtu.step: unknown key",
        ),
    ] {
        assert_eq!(
            refusal(&AWG.replace("{ default = 1380, min = 1280, max = 1420 }", mtu)),
            reason,
            "{mtu}"
        );
    }
}

#[test]
fn every_field_is_in_one_section_of_the_form() {
    let settings = "a = { type = \"bool\", label = \"A\" }\nb = { type = \"bool\", label = \"B\" }";
    assert_eq!(
        refusal(&kind(settings, &["a"])),
        "tunnel.form: b is in no section"
    );
    assert_eq!(
        refusal(&kind(settings, &["a", "b", "a"])),
        "tunnel.form: a is in two sections"
    );
    assert_eq!(
        refusal(&kind(settings, &["a", "b", "c"])),
        "tunnel.form: c is not a setting"
    );
    assert_eq!(
        refusal(&minimal(
            "[settings]\na = { type = \"bool\", label = \"A\" }\n"
        )),
        "form: a is in no section"
    );
    assert_eq!(
        refusal(&kind(settings, &["a", "b"]).replace("title = \"All\"", "title = \"\"")),
        "tunnel.form.title: a display name is 1 to 64 characters, not only whitespace, without control or invisible format characters"
    );
}

#[test]
fn declares_status_values_once_with_a_type_of_ours() {
    let status = |kind: &str| {
        minimal(&format!(
            "\n[[status]]\nkey = \"up\"\nlabel = \"Up\"\ntype = \"{kind}\"\n"
        ))
    };
    for kind in ["text", "count", "bytes", "duration", "time", "address"] {
        assert!(
            routeh_plugin_protocol::Manifest::parse(&status(kind)).is_ok(),
            "{kind}"
        );
    }
    assert_eq!(
        refusal(&status("percent")),
        "status.type: \"percent\" is not one of text, count, bytes, duration, time, address"
    );
    assert_eq!(
        refusal(&format!(
            "{}\n[[status]]\nkey = \"up\"\nlabel = \"Again\"\ntype = \"count\"\n",
            status("count")
        )),
        "status.key: up is declared twice"
    );
    assert_eq!(
        refusal(&status("count").replace("\"up\"", "\"Up\"")),
        "status.key: a key is 1 to 32 lowercase letters, digits or underscores, starting with a letter"
    );
}

#[test]
fn a_page_has_a_title_and_an_icon_of_ours() {
    let page = |lines: &str| minimal(&format!("\n[page]\n{lines}\n"));
    let base = "title = \"Home\"\nicon = \"shield\"";
    assert!(routeh_plugin_protocol::Manifest::parse(&page(base)).is_ok());
    assert!(routeh_plugin_protocol::Manifest::parse(&page("title = \"Home\"")).is_ok());
    assert_eq!(
        refusal(&page(&base.replace("\"shield\"", "\"Shield!\""))),
        "page.icon: 1 to 32 lowercase letters, digits or '-'"
    );
    assert_eq!(refusal(&page("icon = \"shield\"")), "page.title: required");
    assert_eq!(
        refusal(&page("title = \" \"")),
        "page.title: a display name is 1 to 64 characters, not only whitespace, without control or invisible format characters"
    );
    // A page is drawn by the router: nothing of the plugin serves it.
    for gone in [
        "socket = \"page.sock\"",
        "place = \"navigation\"",
        "key = \"home\"",
    ] {
        let key = gone.split(' ').next().unwrap();
        assert_eq!(
            refusal(&page(&format!("{base}\n{gone}"))),
            format!("page.{key}: unknown key")
        );
    }
    assert_eq!(
        refusal(&minimal("\n[[pages]]\ntitle = \"Home\"\n")),
        "pages: unknown key"
    );
}
