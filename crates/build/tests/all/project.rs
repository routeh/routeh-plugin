use std::path::Path;

use routeh_plugin_build::project::{BuildToml, OUT, Project};

const BUILD: &str = r#"
targets = ["armsr/armv8"]

[service]
package = "awg-ctl"

[[module]]
name = "m"
dir = "kmod/m"
pins_version = true

[qemu]
tests = ["live"]
env = { A = "1" }

[rust]
"riscv64_generic" = "riscv64gc-unknown-linux-musl"

[module_packages]
"nf_conntrack" = "kmod-nf-conntrack"
"#;

fn build(text: &str) -> BuildToml {
    toml::from_str(text).unwrap()
}

#[test]
fn a_plugin_s_repository() {
    let p = Project::new(
        Path::new("/r"),
        crate::manifest("awg", "1.0-r1", &["m"]),
        build(BUILD),
    )
    .unwrap();
    assert_eq!((p.id.as_str(), p.version.as_str()), ("awg", "1.0-r1"));
    assert_eq!(p.service, "awgd");
    assert_eq!(p.service_package(), "awg-ctl");
    assert_eq!(p.modules, ["m"]);
    let m = p.module("m").unwrap();
    assert_eq!(m.subdir, "src");
    assert!(m.pins_version);
    assert!(p.module("n").is_none());
    assert_eq!(
        p.rust_of("aarch64_cortex-a53").as_deref(),
        Some("aarch64-unknown-linux-musl")
    );
    assert_eq!(
        p.rust_of("riscv64_generic").as_deref(),
        Some("riscv64gc-unknown-linux-musl")
    );
    assert_eq!(p.rust_of("mips_24kc"), None);
    assert_eq!(
        p.package_of("udp_tunnel").as_deref(),
        Some("kmod-udptunnel4")
    );
    assert_eq!(
        p.package_of("nf_conntrack").as_deref(),
        Some("kmod-nf-conntrack")
    );
    assert_eq!(p.package_of("nope"), None);
    assert_eq!(p.out(), Path::new("/r").join(OUT));
    assert_eq!(p.build.qemu.tests, ["live"]);
}

#[test]
fn the_service_is_named_by_its_command_by_default() {
    let p = Project::new(
        Path::new("/r"),
        crate::manifest("demo", "1", &[]),
        build("targets = [\"x/y\"]\n"),
    )
    .unwrap();
    assert_eq!(p.service_package(), "demod");
    assert!(p.modules.is_empty());
}

#[test]
fn the_manifest_and_build_toml_must_agree() {
    let e = Project::new(
        Path::new("/r"),
        crate::manifest("demo", "1", &[]),
        build(BUILD),
    )
    .unwrap_err();
    assert!(e.contains("loads"), "{e}");
    let e = Project::new(
        Path::new("/r"),
        crate::manifest("demo", "1", &[]),
        build("targets = []\n"),
    )
    .unwrap_err();
    assert!(e.contains("no targets"));
    let bad_command = "id = \"d\"\nversion = \"1\"\n[service]\ncommand = \"d\"\n";
    let e = Project::new(
        Path::new("/r"),
        bad_command.into(),
        build("targets = [\"x/y\"]\n"),
    )
    .unwrap_err();
    assert!(e.contains("bin/<program>"));
    assert!(
        Project::new(
            Path::new("/r"),
            "version = \"1\"".into(),
            build("targets = [\"x/y\"]\n")
        )
        .is_err()
    );
    assert!(Project::new(Path::new("/r"), "=".into(), build("targets = [\"x/y\"]\n")).is_err());
    assert!(toml::from_str::<BuildToml>("targets = []\nunknown = 1\n").is_err());
}

#[test]
fn opens_a_directory() {
    let dir = crate::scratch("project");
    assert!(Project::open(&dir).is_err());
    std::fs::create_dir_all(dir.join("plugin")).unwrap();
    std::fs::write(
        dir.join("plugin/manifest.toml"),
        crate::manifest("demo", "1", &[]),
    )
    .unwrap();
    assert!(Project::open(&dir).unwrap_err().contains("build.toml"));
    std::fs::write(dir.join("build.toml"), "targets = [\"x/y\"]\n").unwrap();
    assert_eq!(Project::open(&dir).unwrap().id, "demo");
    std::fs::write(dir.join("build.toml"), "targets = 1\n").unwrap();
    assert!(Project::open(&dir).is_err());
}
