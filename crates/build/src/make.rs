//! A target's bundle, made of what the build left in `target/routeh/`:
//! the service, the modules and their `<name>.json`, checked against the
//! manifest and the target before anything is packed. A module is read as
//! the router reads it (`modinfo`): its name, vermagic and depends.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::json;

use routeh_plugin_bundle::modinfo;

use crate::bundle::{self, BUILD, MANIFEST, check_pinned, elf_machine, machine_of, sha256_hex};
use crate::cmd::{mkdir, read, write};
use crate::docker::{kmod_dir, service_path};
use crate::openwrt::Target;
use crate::project::Project;

/// The bundle of a target, unsigned: its path.
pub fn bundle(p: &Project, t: &Target) -> Result<PathBuf, String> {
    let want = machine_of(&t.arch).ok_or(format!("unknown architecture {}", t.arch))?;
    let service = read(&service_path(p, &t.rust)).map_err(|e| format!("{e} (build it first)"))?;
    if elf_machine(&service) != Some(want) {
        return Err(format!("{} is not a program for {}", p.service, t.arch));
    }

    let mut files = BTreeMap::new();
    let mut modules = serde_json::Map::new();
    let kdir = kmod_dir(p, &t.openwrt, &t.target);
    for name in &p.modules {
        let m = p.module(name).expect("checked by Project");
        let ko =
            read(&kdir.join(format!("{name}.ko"))).map_err(|e| format!("{e} (build it first)"))?;
        let meta: serde_json::Value =
            serde_json::from_slice(&read(&kdir.join(format!("{name}.json")))?)
                .map_err(|e| format!("{name}.json: {e}"))?;
        let s = |k: &str| meta[k].as_str().unwrap_or_default().to_owned();
        if s("kernel_package") != t.kernel {
            return Err(format!(
                "{name}.ko is for kernel {}, {} on {} has {}",
                s("kernel_package"),
                t.target,
                t.openwrt,
                t.kernel
            ));
        }
        let info = modinfo::read(&ko).map_err(|e| format!("{name}.ko: {e}"))?;
        if info.name != *name {
            return Err(format!("{name}.ko names itself {}", info.name));
        }
        if info.vermagic != s("vermagic") {
            return Err(format!("{name}.json's vermagic is not {name}.ko's"));
        }
        if elf_machine(&ko) != Some(want) {
            return Err(format!("{name}.ko is not a module for {}", t.arch));
        }
        if m.pins_version {
            check_pinned(&p.version, name, &s("module_version")).map_err(|e| {
                format!("{e}; change `version` in plugin/manifest.toml or {name}'s source")
            })?;
        }
        let depends = info.depends;
        for d in &depends {
            if p.modules.contains(d) {
                continue;
            }
            let package = p.package_of(d).ok_or(format!(
                "{name} depends on {d}: add its package to [module_packages] of build.toml"
            ))?;
            if !p.requires.contains(&package) {
                return Err(format!(
                    "{name} depends on {d} of {package}, which `requires` of plugin/manifest.toml lacks"
                ));
            }
        }
        modules.insert(
            name.clone(),
            json!({
                "version": s("module_version"),
                "vermagic": info.vermagic,
                "build_id": s("build_id"),
                "depends": depends,
                "upstream": s("upstream"),
                "patches": s("patches"),
            }),
        );
        files.insert(format!("modules/{name}.ko"), ko);
    }

    let kernel = (!p.modules.is_empty()).then_some(t.kernel.as_str());
    let manifest = match kernel {
        Some(k) => bundle::with_kernel(&p.manifest, k)?,
        None => p.manifest.clone(),
    };
    let build = json!({
        "plugin": p.id,
        "version": p.version,
        "openwrt": t.openwrt,
        "target": t.target,
        "arch": t.arch,
        "rust": t.rust,
        "kernel_package": kernel,
        "modules": modules,
    });
    files.insert(format!("bin/{}", p.service), service);
    files.insert(
        BUILD.to_owned(),
        serde_json::to_vec_pretty(&build).expect("serializable"),
    );
    files.insert(MANIFEST.to_owned(), manifest.into_bytes());
    let tar =
        routeh_plugin_bundle::pack(files.iter().map(|(p, b)| (p.as_str(), b.as_slice())), None)
            .map_err(|e| e.to_string())?;
    bundle::check(&tar)?;

    let dir = p.out().join("bundles");
    mkdir(&dir)?;
    let path = dir.join(&t.bundle);
    write(&path, &tar)?;
    // For CI: what the bundle is, without opening it.
    let side = json!({
        "file": t.bundle,
        "sha256": sha256_hex(&tar),
        "size": tar.len(),
        "plugin": p.id,
        "version": p.version,
        "openwrt": t.openwrt,
        "target": t.target,
        "arch": t.arch,
        "kernel": kernel,
    });
    let side_path = dir.join(format!("{}.json", t.bundle.trim_end_matches(".tar")));
    write(
        &side_path,
        &serde_json::to_vec_pretty(&side).expect("serializable"),
    )?;
    Ok(path)
}
