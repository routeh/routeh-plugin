//! The only two places anything runs in Docker, each with a `--native`
//! twin for CI:
//!
//! - the OpenWrt SDK, in the image of `openwrt/Dockerfile` (linux/amd64:
//!   the SDK's tools are x86_64), with SDKs kept in the volume `routeh-sdk`
//!   (`ROUTEH_SDK_VOLUME` names another);
//! - cargo for a musl target, in `rust-musl-cross` (its cross toolchain,
//!   whatever toolchain the plugin pins for its host), with the registry
//!   in the volume `routeh-cargo` (`ROUTEH_CARGO_VOLUME`).

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::bundle::sha256_hex;
use crate::cmd::{mkdir, run, stdout, write};
use crate::project::{ModuleBuild, Project};
use crate::scripts;

fn volume(env: &str, default: &str) -> String {
    std::env::var(env).unwrap_or_else(|_| default.to_owned())
}

/// Builds for a musl target go here, apart from the host's builds.
pub const CROSS_TARGET_DIR: &str = "target/cross";

fn cross_image(rust: &str) -> Result<&'static str, String> {
    Ok(match rust {
        "aarch64-unknown-linux-musl" => "ghcr.io/rust-cross/rust-musl-cross:aarch64-musl",
        "armv7-unknown-linux-musleabihf" => "ghcr.io/rust-cross/rust-musl-cross:armv7-musleabihf",
        "x86_64-unknown-linux-musl" => "ghcr.io/rust-cross/rust-musl-cross:x86_64-musl",
        _ => return Err(format!("no musl toolchain for {rust}")),
    })
}

/// `cargo <args> --target <rust>` in the repository, its standard output
/// returned. Paths in it are of the repository either way (the
/// container's /src is mapped back).
pub fn cargo(
    p: &Project,
    rust: &str,
    args: &[&str],
    env: &[(&str, &str)],
    native: bool,
) -> Result<String, String> {
    let mut all: Vec<&str> = args.to_vec();
    all.extend(["--target", rust, "--target-dir", CROSS_TARGET_DIR]);
    let root = p.root.display().to_string();
    if native {
        let mut c = Command::new("cargo");
        c.current_dir(&p.root).args(&all).envs(env.iter().copied());
        return stdout(&mut c);
    }
    let mut c = Command::new("docker");
    c.args(["run", "--rm", "-v", &format!("{root}:/src"), "-w", "/src"])
        .args([
            "-v",
            &format!(
                "{}:/root/.cargo/registry",
                volume("ROUTEH_CARGO_VOLUME", "routeh-cargo")
            ),
        ]);
    // The image's own toolchain, the one with the musl target and its
    // linker: a plugin's rust-toolchain.toml would make rustup fetch the
    // toolchain it names, without that target, in every run.
    c.args(["-e", "RUSTUP_TOOLCHAIN=stable"]);
    for (k, v) in env {
        c.args(["-e", &format!("{k}={v}")]);
    }
    c.arg(cross_image(rust)?).arg("cargo").args(&all);
    Ok(stdout(&mut c)?.replace("\"/src/", &format!("\"{root}/")))
}

/// Where the service of a Rust target is put.
pub fn service_path(p: &Project, rust: &str) -> PathBuf {
    p.out().join("service").join(rust).join(&p.service)
}

/// The service for a Rust target: static, the plugin's version in its
/// `version_env`.
pub fn build_service(p: &Project, rust: &str, native: bool) -> Result<PathBuf, String> {
    let env = p
        .build
        .service
        .version_env
        .as_deref()
        .unwrap_or("ROUTEH_PLUGIN_VERSION");
    let args = [
        "build",
        "-q",
        "--release",
        "-p",
        p.service_package(),
        "--bin",
        &p.service,
    ];
    cargo(p, rust, &args, &[(env, &p.version)], native)?;
    let to = service_path(p, rust);
    mkdir(to.parent().expect("a directory"))?;
    let from = p
        .root
        .join(CROSS_TARGET_DIR)
        .join(rust)
        .join("release")
        .join(&p.service);
    std::fs::copy(&from, &to).map_err(|e| format!("{}: {e}", from.display()))?;
    Ok(to)
}

/// Where a release and target's modules are put: `<name>.ko` and
/// `<name>.json`.
pub fn kmod_dir(p: &Project, release: &str, target: &str) -> PathBuf {
    p.out()
        .join("kmod")
        .join(release)
        .join(target.replace('/', "-"))
}

/// Every module of the plugin for a release and target, by the SDK
/// (openwrt/kmod-build.sh).
pub fn build_modules(p: &Project, release: &str, target: &str, native: bool) -> Result<(), String> {
    let out = kmod_dir(p, release, target);
    mkdir(&out)?;
    let dir = scripts::write_all(p)?;
    for name in &p.modules {
        let m = p.module(name).expect("checked by Project");
        eprintln!("== {target} on {release}: {name}.ko");
        sdk(p, &dir, m, release, target, &out, native)?;
    }
    Ok(())
}

fn module_env(m: &ModuleBuild, root: &str) -> Vec<(String, String)> {
    let patches = m
        .patches
        .clone()
        .unwrap_or_else(|| format!("{}/patches", m.dir));
    vec![
        ("MODULE".into(), m.name.clone()),
        ("MODULE_DIR".into(), format!("{root}/{}", m.dir)),
        ("SUBDIR".into(), m.subdir.clone()),
        ("PATCHES".into(), format!("{root}/{patches}")),
        (
            "VERSION_FILE".into(),
            m.version_file.clone().unwrap_or_default(),
        ),
        (
            "VERSION_DEFINE".into(),
            m.version_define.clone().unwrap_or_default(),
        ),
    ]
}

fn sdk(
    p: &Project,
    scripts: &Path,
    m: &ModuleBuild,
    release: &str,
    target: &str,
    out: &Path,
    native: bool,
) -> Result<(), String> {
    if native {
        let sdk =
            std::env::var("SDK_DIR").unwrap_or_else(|_| p.out().join("sdk").display().to_string());
        let root = p.root.display().to_string();
        return run(Command::new("sh")
            .arg(scripts.join("kmod-build.sh"))
            .env("RELEASE", release)
            .env("TARGET", target)
            .envs(module_env(m, &root))
            .env("SDK_DIR", sdk)
            .env("OUT_DIR", out));
    }
    let context = p.out().join("docker");
    mkdir(&context)?;
    write(&context.join("Dockerfile"), scripts::DOCKERFILE.as_bytes())?;
    let image = format!(
        "routeh-sdk:{}",
        &sha256_hex(scripts::DOCKERFILE.as_bytes())[..12]
    );
    let have = Command::new("docker")
        .args(["image", "inspect", &image])
        .output()
        .is_ok_and(|o| o.status.success());
    if !have {
        eprintln!("== the SDK's image {image}");
        stdout(
            Command::new("docker")
                .args(["build", "-q", "--platform", "linux/amd64", "-t", &image])
                .arg(&context),
        )?;
    }
    let mut c = Command::new("docker");
    c.args(["run", "--rm", "--platform", "linux/amd64"])
        .args(["-v", &format!("{}:/src:ro", p.root.display())])
        .args(["-v", &format!("{}:/scripts:ro", scripts.display())])
        .args(["-v", &format!("{}:/out", out.display())])
        .args([
            "-v",
            &format!("{}:/sdk", volume("ROUTEH_SDK_VOLUME", "routeh-sdk")),
        ])
        .args(["-e", &format!("RELEASE={release}")])
        .args(["-e", &format!("TARGET={target}")])
        .args(["-e", "SDK_DIR=/sdk", "-e", "OUT_DIR=/out"]);
    for (k, v) in module_env(m, "/src") {
        c.args(["-e", &format!("{k}={v}")]);
    }
    run(c.args([&image, "sh", "-eu", "/scripts/kmod-build.sh"]))
}
