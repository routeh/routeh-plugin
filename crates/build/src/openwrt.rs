//! Releases and targets of OpenWrt.

use serde::Serialize;

use crate::bundle;
use crate::cmd::fetch;
use crate::project::Project;

const DOWNLOADS: &str = "https://downloads.openwrt.org";

/// One target of one release, as a build needs it.
#[derive(Debug, Clone, Serialize)]
pub struct Target {
    pub openwrt: String,
    pub target: String,
    /// `OPENWRT_ARCH`.
    pub arch: String,
    pub rust: String,
    /// The version of the package `kernel`: `6.12.112~<hash>-r1`.
    pub kernel: String,
    /// The file name of its bundle.
    pub bundle: String,
}

/// A release by number, or `stable`. Release candidates and snapshots are
/// not built.
pub fn release(r: &str) -> Result<String, String> {
    if r == "stable" {
        let v: serde_json::Value =
            serde_json::from_str(&fetch(&format!("{DOWNLOADS}/.versions.json"))?)
                .map_err(|e| format!(".versions.json: {e}"))?;
        return v["stable_version"]
            .as_str()
            .map(str::to_owned)
            .ok_or("no stable_version".into());
    }
    let numbers = r.split('.').count() == 3
        && r.split('.')
            .all(|p| !p.is_empty() && p.bytes().all(|c| c.is_ascii_digit()));
    if numbers {
        Ok(r.to_owned())
    } else {
        Err(format!("{r:?} is not a release like 25.12.6"))
    }
}

/// The base URL of a release's target.
pub fn target_url(release: &str, target: &str) -> String {
    format!("{DOWNLOADS}/releases/{release}/targets/{target}")
}

/// `all` (the targets of build.toml) or a comma list, of a release.
pub fn resolve(p: &Project, release: &str, which: &str) -> Result<Vec<Target>, String> {
    let names: Vec<String> = if which == "all" {
        p.build.targets.clone()
    } else {
        which
            .split(',')
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .collect()
    };
    let mut out = Vec::new();
    for name in names {
        if name.split('/').count() != 2 {
            return Err(format!("{name:?} is not a target like mediatek/filogic"));
        }
        let url = format!("{}/profiles.json", target_url(release, &name));
        let profiles: serde_json::Value =
            serde_json::from_str(&fetch(&url)?).map_err(|e| format!("{url}: {e}"))?;
        let arch = profiles["arch_packages"]
            .as_str()
            .ok_or(format!("{url}: no arch_packages"))?
            .to_owned();
        let k = &profiles["linux_kernel"];
        let kernel = format!(
            "{}~{}-r{}",
            k["version"].as_str().unwrap_or_default(),
            k["vermagic"].as_str().unwrap_or_default(),
            k["release"].as_str().unwrap_or_default()
        );
        let rust = p.rust_of(&arch).ok_or(format!(
            "{name} is {arch}: add its Rust target to [rust] of build.toml"
        ))?;
        let with_kernel = (!p.modules.is_empty()).then_some(kernel.as_str());
        let bundle = bundle::file_name(&p.id, &p.version, &arch, with_kernel);
        out.push(Target {
            openwrt: release.to_owned(),
            target: name,
            arch,
            rust,
            kernel,
            bundle,
        });
    }
    Ok(out)
}
