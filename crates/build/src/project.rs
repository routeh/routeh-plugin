//! A plugin's repository as the build reads it: `plugin/manifest.toml`
//! and `build.toml`.
//!
//! ```toml
//! # build.toml
//! targets = ["armsr/armv8", "mediatek/filogic"]   # OpenWrt targets
//!
//! [service]                     # the program of the manifest's command
//! package = "awg-ctl"           # its cargo package; the command's name by default
//! version_env = "AWG_PLUGIN_VERSION"   # set to the plugin's version when built
//!
//! [[module]]                    # each module of [modules] load, in any order
//! name = "amneziawg"
//! dir = "kmod/amneziawg"        # its OpenWrt Makefile, and patches/ beside it
//! subdir = "src"                # where its Kbuild is in its source ("src")
//! version_file = "src/version.h"          # its version: a #define in its source
//! version_define = "WIREGUARD_VERSION"
//! pins_version = true           # the plugin's version is <its version>-r<n>
//!
//! [qemu]                        # `qemu-test`: the workspace's tests run there
//! tests = ["live", "e2e"]
//! env = { AWG_LIVE = "1" }
//!
//! [rust]                        # more OPENWRT_ARCH -> Rust targets
//! [module_packages]             # more modules -> OpenWrt kmod packages
//! ```
//!
//! A module's source is its Makefile's `PKG_SOURCE_URL` at
//! `PKG_SOURCE_VERSION` (git), or, with no `PKG_SOURCE_URL`, the files
//! under `dir` itself.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

pub const MANIFEST: &str = "plugin/manifest.toml";
pub const BUILD_TOML: &str = "build.toml";
/// Everything the build makes and keeps, inside cargo's own `target/`.
pub const OUT: &str = "target/routeh";

/// OpenWrt architectures and the Rust target a service is built for.
const RUST: [(&str, &str); 8] = [
    ("aarch64_generic", "aarch64-unknown-linux-musl"),
    ("aarch64_cortex-a53", "aarch64-unknown-linux-musl"),
    ("aarch64_cortex-a72", "aarch64-unknown-linux-musl"),
    ("aarch64_cortex-a76", "aarch64-unknown-linux-musl"),
    ("arm_cortex-a7_neon-vfpv4", "armv7-unknown-linux-musleabihf"),
    ("arm_cortex-a9_vfpv3-d16", "armv7-unknown-linux-musleabihf"),
    (
        "arm_cortex-a15_neon-vfpv4",
        "armv7-unknown-linux-musleabihf",
    ),
    ("x86_64", "x86_64-unknown-linux-musl"),
];

/// Modules of OpenWrt's kernel and the kmod package each is in: what a
/// module's `depends` may name. A bundle is refused when a dependency of a
/// module is not known, or its package is not in the manifest's `requires`.
const MODULE_PACKAGES: [(&str, &str); 4] = [
    ("udp_tunnel", "kmod-udptunnel4"),
    ("ip6_udp_tunnel", "kmod-udptunnel6"),
    ("libchacha20poly1305", "kmod-crypto-lib-chacha20poly1305"),
    ("libcurve25519-generic", "kmod-crypto-lib-curve25519"),
];

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildToml {
    pub targets: Vec<String>,
    #[serde(default)]
    pub service: ServiceBuild,
    #[serde(default, rename = "module")]
    pub modules: Vec<ModuleBuild>,
    #[serde(default)]
    pub qemu: Qemu,
    #[serde(default)]
    pub rust: BTreeMap<String, String>,
    #[serde(default)]
    pub module_packages: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceBuild {
    pub package: Option<String>,
    pub version_env: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleBuild {
    pub name: String,
    pub dir: String,
    #[serde(default = "src")]
    pub subdir: String,
    pub patches: Option<String>,
    pub version_file: Option<String>,
    pub version_define: Option<String>,
    #[serde(default)]
    pub pins_version: bool,
}

fn src() -> String {
    "src".into()
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Qemu {
    #[serde(default)]
    pub tests: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

/// A plugin's repository.
#[derive(Debug, Clone)]
pub struct Project {
    pub root: PathBuf,
    pub id: String,
    pub version: String,
    /// The manifest as written (no `[modules] kernel`).
    pub manifest: String,
    /// `requires` of the manifest.
    pub requires: Vec<String>,
    /// `[modules] load` of the manifest, in order.
    pub modules: Vec<String>,
    /// The service's program: the command `bin/<name>`.
    pub service: String,
    pub build: BuildToml,
}

impl Project {
    /// The repository at `root`.
    pub fn open(root: &Path) -> Result<Self, String> {
        let read = |name: &str| {
            std::fs::read_to_string(root.join(name)).map_err(|e| format!("{name}: {e}"))
        };
        let manifest = read(MANIFEST)?;
        let build: BuildToml =
            toml::from_str(&read(BUILD_TOML)?).map_err(|e| format!("{BUILD_TOML}: {e}"))?;
        Self::new(root, manifest, build)
    }

    /// From the texts, checked against each other.
    pub fn new(root: &Path, manifest: String, build: BuildToml) -> Result<Self, String> {
        let m: toml::Table = manifest
            .parse()
            .map_err(|e: toml::de::Error| format!("{MANIFEST}: {e}"))?;
        let text = |v: Option<&toml::Value>, what: &str| {
            v.and_then(|x| x.as_str())
                .map(str::to_owned)
                .ok_or(format!("{MANIFEST}: no {what}"))
        };
        let strings = |v: Option<&toml::Value>| -> Vec<String> {
            v.and_then(|x| x.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default()
        };
        let id = text(m.get("id"), "id")?;
        let version = text(m.get("version"), "version")?;
        let command = text(
            m.get("service").and_then(|s| s.get("command")),
            "[service] command",
        )?;
        let service = command
            .strip_prefix("bin/")
            .filter(|n| !n.is_empty() && !n.contains('/'))
            .ok_or(format!(
                "{MANIFEST}: the command {command:?} is not bin/<program>"
            ))?
            .to_owned();
        let modules = strings(m.get("modules").and_then(|x| x.get("load")));
        let requires = strings(m.get("requires"));

        let mut built: Vec<&str> = build.modules.iter().map(|x| x.name.as_str()).collect();
        let mut loaded: Vec<&str> = modules.iter().map(String::as_str).collect();
        built.sort_unstable();
        loaded.sort_unstable();
        if built != loaded {
            return Err(format!(
                "{BUILD_TOML} builds the modules {built:?}, {MANIFEST} loads {loaded:?}"
            ));
        }
        if build.targets.is_empty() {
            return Err(format!("{BUILD_TOML}: no targets"));
        }
        Ok(Self {
            root: root.to_owned(),
            id,
            version,
            manifest,
            requires,
            modules,
            service,
            build,
        })
    }

    /// The Rust target of an OpenWrt architecture.
    pub fn rust_of(&self, arch: &str) -> Option<String> {
        self.build.rust.get(arch).cloned().or_else(|| {
            RUST.iter()
                .find(|(a, _)| *a == arch)
                .map(|(_, r)| (*r).to_owned())
        })
    }

    /// The kmod package of a module of OpenWrt's kernel.
    pub fn package_of(&self, module: &str) -> Option<String> {
        self.build.module_packages.get(module).cloned().or_else(|| {
            MODULE_PACKAGES
                .iter()
                .find(|(m, _)| *m == module)
                .map(|(_, p)| (*p).to_owned())
        })
    }

    pub fn module(&self, name: &str) -> Option<&ModuleBuild> {
        self.build.modules.iter().find(|m| m.name == name)
    }

    /// The cargo package of the service.
    pub fn service_package(&self) -> &str {
        self.build
            .service
            .package
            .as_deref()
            .unwrap_or(&self.service)
    }

    /// `target/routeh` of the repository.
    pub fn out(&self) -> PathBuf {
        self.root.join(OUT)
    }
}
