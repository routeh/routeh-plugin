//! `qemu-test`: the plugin's modules and its tests on a real OpenWrt,
//! armsr/armv8 in QEMU. Here: what the test needs, built for aarch64. In
//! `openwrt/qemu-test.sh`: the virtual machine.

use std::path::Path;
use std::process::Command;

use crate::cmd::{mkdir, run};
use crate::docker::{self, kmod_dir, service_path};
use crate::project::Project;
use crate::scripts;

pub const TARGET: &str = "armsr/armv8";
pub const RUST: &str = "aarch64-unknown-linux-musl";

pub fn test(p: &Project, release: &str, native: bool) -> Result<(), String> {
    if p.modules.is_empty() && p.build.qemu.tests.is_empty() {
        return Err("nothing to test in QEMU: no modules, no [qemu] tests in build.toml".into());
    }
    let kmod = kmod_dir(p, release, TARGET);
    let built = p
        .modules
        .iter()
        .all(|m| kmod.join(format!("{m}.ko")).exists());
    if !built {
        docker::build_modules(p, release, TARGET, native)?;
    }
    let bin = p.out().join("test-bin");
    mkdir(&bin)?;
    if !service_path(p, RUST).exists() {
        eprintln!("== {} {RUST}", p.service);
        docker::build_service(p, RUST, native)?;
    }
    let copy = |from: &Path, name: &str| {
        std::fs::copy(from, bin.join(name)).map_err(|e| format!("{}: {e}", from.display()))
    };
    copy(&service_path(p, RUST), &p.service)?;

    let tests = &p.build.qemu.tests;
    if !tests.is_empty() {
        eprintln!("== test programs {RUST}");
        let json = docker::cargo(
            p,
            RUST,
            &[
                "test",
                "-q",
                "--no-run",
                "--workspace",
                "--message-format=json",
            ],
            &[],
            native,
        )?;
        let mut found = Vec::new();
        for line in json.lines() {
            let Ok(m) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };
            let (Some(exe), Some(name)) = (m["executable"].as_str(), m["target"]["name"].as_str())
            else {
                continue;
            };
            if tests.iter().any(|t| t == name) {
                copy(Path::new(exe), name)?;
                found.push(name.to_owned());
            }
        }
        if let Some(t) = tests.iter().find(|t| !found.contains(t)) {
            return Err(format!(
                "the test {t} of [qemu] in build.toml was not built"
            ));
        }
    }

    let dir = scripts::write_all(p)?;
    let env: Vec<String> = p
        .build
        .qemu
        .env
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect();
    run(Command::new("sh")
        .arg(dir.join("qemu-test.sh"))
        .env("RELEASE", release)
        .env("KMOD_DIR", &kmod)
        .env("BIN_DIR", &bin)
        .env("CACHE", p.out().join("qemu"))
        .env("MODULES", p.modules.join(" "))
        .env("PACKAGES", p.requires.join(" "))
        .env("TESTS", tests.join(" "))
        .env("TEST_ENV", env.join(" ")))
}
