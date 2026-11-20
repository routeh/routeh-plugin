use std::collections::BTreeMap;

use routeh_plugin_build::bundle::*;

use crate::{files, pack, unsigned};

const KERNEL: &str = "6.12.112~67e205f2-r1";

#[test]
fn an_unsigned_bundle_checks() {
    let info = check(&unsigned("demo", "1.0-r1", "aarch64_generic", Some(KERNEL))).unwrap();
    assert_eq!(info.plugin, "demo");
    assert_eq!(info.version, "1.0-r1");
    assert_eq!(info.arch, "aarch64_generic");
    assert_eq!(info.kernel.as_deref(), Some(KERNEL));
    assert_eq!(info.signer, None);
    assert!(info.to_string().ends_with("unsigned"));
    let none = check(&unsigned("demo", "1.0", "x86_64", None)).unwrap();
    assert_eq!(none.kernel, None);
}

#[test]
fn a_signed_bundle_names_its_key() {
    let dir = crate::scratch("bundle-keys");
    let key = routeh_plugin_build::routeh_plugin_bundle::SecretKey::generate().unwrap();
    let (secret, public) = (dir.join("k"), dir.join("k.pub"));
    std::fs::write(&secret, key.to_line()).unwrap();
    std::fs::write(&public, key.public().to_line("ci")).unwrap();
    let read = read_secret(secret.to_str().unwrap()).unwrap();
    assert_eq!(read.public(), key.public());
    assert_eq!(read_public(public.to_str().unwrap()).unwrap(), key.public());
    assert!(read_secret(public.to_str().unwrap()).is_err());
    assert!(read_public("/nonexistent").is_err());
    assert!(read_secret("/nonexistent").is_err());

    let b = unsigned("demo", "1.0", "x86_64", Some(KERNEL));
    let signed = routeh_plugin_build::routeh_plugin_bundle::sign(&b, &key).unwrap();
    let info = check(&signed).unwrap();
    assert_eq!(info.signer, Some(key.public()));
    assert!(
        info.to_string()
            .ends_with(&format!("signed by {}", key_text(&key.public())))
    );
    assert!(check(b"not a bundle").is_err());
}

fn bundle_of(f: BTreeMap<String, Vec<u8>>) -> Result<Info, String> {
    check(&pack(&f))
}

#[test]
fn the_manifest_and_build_json_must_agree() {
    let base = || files("demo", "1.0", "x86_64", Some(KERNEL));
    let mut f = base();
    f.remove("modules/m.ko");
    assert!(bundle_of(f).unwrap_err().contains("modules/m.ko"));
    let mut f = base();
    f.remove("bin/demod");
    assert!(bundle_of(f).unwrap_err().contains("command"));
    let mut f = base();
    f.insert(MANIFEST.into(), b"id = 1".to_vec());
    assert!(bundle_of(f).unwrap_err().starts_with("manifest.toml"));
    let mut f = base();
    f.remove(MANIFEST);
    assert!(bundle_of(f).is_err());
    let mut f = base();
    f.insert(
        BUILD.into(),
        br#"{"plugin":"demo","version":"2.0","arch":"x86_64"}"#.to_vec(),
    );
    assert!(bundle_of(f).unwrap_err().contains("disagree"));
    let mut f = base();
    f.insert(
        BUILD.into(),
        format!(r#"{{"plugin":"demo","version":"1.0","kernel_package":"{KERNEL}"}}"#).into_bytes(),
    );
    assert!(bundle_of(f).unwrap_err().contains("arch"));
    let mut f = base();
    f.insert(BUILD.into(), b"[".to_vec());
    assert!(bundle_of(f).unwrap_err().starts_with("build.json"));
}

#[test]
fn the_kernel_goes_into_modules() {
    let m = crate::manifest("demo", "1.0", &["m"]);
    let out = with_kernel(&m, KERNEL).unwrap();
    let v: toml::Table = out.parse().unwrap();
    assert_eq!(v["modules"]["kernel"].as_str(), Some(KERNEL));
    assert!(
        with_kernel(&out, "x")
            .unwrap_err()
            .contains("the build does")
    );
    assert!(with_kernel(&crate::manifest("demo", "1.0", &[]), "x").is_err());
    assert!(with_kernel("modules = { load = [] }\n", "x").is_err());
    assert!(with_kernel("=", "x").is_err());
}

#[test]
fn names_and_versions() {
    assert_eq!(kernel_dir(KERNEL), "6.12.112-1-67e205f2");
    assert_eq!(kernel_dir("6.6~x"), "6.6-x");
    assert_eq!(
        file_name("awg", "3.1-r1", "aarch64_generic", Some(KERNEL)),
        "awg-3.1-r1-aarch64_generic-6.12.112-1-67e205f2.tar"
    );
    assert_eq!(file_name("x", "1", "x86_64", None), "x-1-x86_64.tar");
    assert_eq!(
        numeric_version("3.1.20261113-02").as_deref(),
        Some("3.1.20261113.2")
    );
    assert_eq!(numeric_version("v1"), None);
    assert!(check_pinned("3.1.20261113.2-r1", "m", "3.1.20261113-02").is_ok());
    assert!(check_pinned("3.1.20261113.2-r12", "m", "3.1.20261113-02").is_ok());
    assert!(check_pinned("3.1.20261113.3-r1", "m", "3.1.20261113-02").is_err());
    assert!(check_pinned("3.1.20261113.2", "m", "3.1.20261113-02").is_err());
    assert!(check_pinned("3.1.20261113.2-rx", "m", "3.1.20261113-02").is_err());
    assert!(check_pinned("1-r1", "m", "").is_err());
}

#[test]
fn elf_machines() {
    assert_eq!(machine_of("aarch64_cortex-a53"), Some(183));
    assert_eq!(machine_of("arm_cortex-a7_neon-vfpv4"), Some(40));
    assert_eq!(machine_of("x86_64"), Some(62));
    assert_eq!(machine_of("i386_pentium4"), Some(3));
    assert_eq!(machine_of("i386_pentium-mmx"), Some(3));
    assert_eq!(machine_of("mipsel_24kc"), Some(8));
    assert_eq!(machine_of("riscv64_generic"), None);
    let mut le = vec![0x7f, b'E', b'L', b'F', 2, 1];
    le.resize(20, 0);
    le[18] = 183;
    assert_eq!(elf_machine(&le), Some(183));
    let mut be = le.clone();
    be[5] = 2;
    be[18] = 0;
    be[19] = 8;
    assert_eq!(elf_machine(&be), Some(8));
    assert_eq!(elf_machine(b"#!/bin/sh"), None);
}
