//! Every test of the crate in one binary: each file beside this one is a
//! module of it.
mod bundle;
mod feed;
mod project;

use std::collections::BTreeMap;
use std::path::PathBuf;

use routeh_plugin_build::bundle::{BUILD, MANIFEST, with_kernel};

/// A manifest the router installs: one service, `modules` loaded.
pub fn manifest(id: &str, version: &str, modules: &[&str]) -> String {
    let api = routeh_plugin_protocol::API;
    let mut m = format!(
        "id = \"{id}\"\nname = \"Demo\"\nversion = \"{version}\"\napi = {api}\n\n[service]\ncommand = \"bin/{id}d\"\n"
    );
    if !modules.is_empty() {
        let load: Vec<String> = modules.iter().map(|m| format!("\"{m}\"")).collect();
        m.push_str(&format!("\n[modules]\nload = [{}]\n", load.join(", ")));
    }
    m
}

/// The files of a bundle of `id` `version` for `arch`, with a module for
/// `kernel` when there is one.
pub fn files(
    id: &str,
    version: &str,
    arch: &str,
    kernel: Option<&str>,
) -> BTreeMap<String, Vec<u8>> {
    let mut f = BTreeMap::new();
    let mut m = manifest(id, version, if kernel.is_some() { &["m"] } else { &[] });
    if let Some(k) = kernel {
        m = with_kernel(&m, k).unwrap();
        f.insert("modules/m.ko".to_owned(), b"\x7fELF module".to_vec());
    }
    let build = serde_json::json!({
        "plugin": id, "version": version, "arch": arch, "kernel_package": kernel,
    });
    f.insert(format!("bin/{id}d"), b"\x7fELF program".to_vec());
    f.insert(BUILD.to_owned(), serde_json::to_vec(&build).unwrap());
    f.insert(MANIFEST.to_owned(), m.into_bytes());
    f
}

/// An unsigned bundle.
pub fn unsigned(id: &str, version: &str, arch: &str, kernel: Option<&str>) -> Vec<u8> {
    pack(&files(id, version, arch, kernel))
}

/// A bundle of `files`, unsigned.
pub fn pack(files: &BTreeMap<String, Vec<u8>>) -> Vec<u8> {
    routeh_plugin_build::routeh_plugin_bundle::pack(
        files.iter().map(|(p, b)| (p.as_str(), b.as_slice())),
        None,
    )
    .unwrap()
}

/// A new, empty directory of its own.
pub fn scratch(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("routeh-plugin-build-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
