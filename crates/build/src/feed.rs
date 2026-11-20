//! A feed's runs: the format (`index.json`, its signature, `blobs/`) is
//! `routeh_plugin_bundle::feed`; here is what fills it.
//!
//! - The managed feed (`run`): its repository is the only writer
//!   (`docs/RELEASE.md`). A run signs the bundles plugins proposed
//!   (`incoming/<id>/*.tar`, each plugin's branch), stores the new ones in
//!   `blobs/`, and rebuilds the index from every blob and `feed.toml`. The
//!   index is derived, never edited: a run that saw everything is right.
//! - A plugin's own feed (`publish`): the plugin is the only writer; its
//!   bundles are added to the index it has, which is all a run needs.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use routeh_plugin_bundle::feed::{Build, INDEX, INDEX_SIG, Index, Version};
use routeh_plugin_bundle::{SecretKey, signer_of};
use serde::Deserialize;

use crate::bundle::{self, key_text, sha256_hex};

pub const BLOBS: &str = "blobs";

/// `feed.toml`: what the feed shows of each plugin.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(transparent)]
pub struct Policy {
    pub plugins: BTreeMap<String, PluginPolicy>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginPolicy {
    /// The newest versions shown for each architecture and kernel; all
    /// when not set.
    pub keep: Option<usize>,
    /// Versions not shown. Their blobs stay.
    #[serde(default)]
    pub yank: Vec<String>,
}

impl Policy {
    pub fn parse(text: &str) -> Result<Self, String> {
        let p: Self = toml::from_str(text).map_err(|e| format!("feed.toml: {e}"))?;
        if let Some((id, _)) = p.plugins.iter().find(|(_, x)| x.keep == Some(0)) {
            return Err(format!(
                "feed.toml: [{id}] keep = 0 shows nothing; yank its versions"
            ));
        }
        Ok(p)
    }

    /// `feed.toml` at `path`; no file is no policy.
    pub fn read(path: &Path) -> Result<Self, String> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }
}

/// The feed's index, checked against its signature by `key`; `None` for a
/// feed with no index yet.
pub fn read_index(dir: &Path, key: &SecretKey) -> Result<Option<Index>, String> {
    let (index, sig) = (dir.join(INDEX), dir.join(INDEX_SIG));
    if !index.exists() {
        return Ok(None);
    }
    let text = std::fs::read(&index).map_err(|e| format!("{}: {e}", index.display()))?;
    let line = std::fs::read_to_string(&sig).map_err(|e| format!("{}: {e}", sig.display()))?;
    let by = signer_of(&line, &text).map_err(|e| format!("{INDEX_SIG}: {e}"))?;
    if by != key.public() {
        return Err(format!(
            "{INDEX} is signed by {}, not by this key: another key's feed is not written over",
            key_text(&by)
        ));
    }
    Index::parse(&text).map(Some).map_err(|e| e.to_string())
}

/// What a run did.
#[derive(Debug, Default)]
pub struct Report {
    /// Bundles signed into `blobs/` by this run.
    pub added: Vec<String>,
    /// Bundles refused, with why. The run went on without them.
    pub refused: Vec<String>,
    /// Whether the index was written (and signed): it changed, or it was
    /// asked to be signed again.
    pub written: bool,
    pub serial: u64,
}

/// One run of the managed feed in `dir`: the bundles of `incoming/<id>/`
/// signed into `blobs/`, the index rebuilt from all blobs and `policy`, and
/// written when it changed or when `refresh` (a new `expires`).
///
/// A bundle of a version, architecture and kernel the feed holds already,
/// with other bytes (the same version built again), replaces the blob it
/// holds: the index has one build of each.
pub fn run(
    dir: &Path,
    key: &SecretKey,
    incoming: Option<&Path>,
    policy: &Policy,
    refresh: bool,
    expires_days: u64,
    now: u64,
) -> Result<Report, String> {
    let old = read_index(dir, key)?;
    let blobs = dir.join(BLOBS);
    std::fs::create_dir_all(&blobs).map_err(|e| format!("{}: {e}", blobs.display()))?;
    let mut held = held(&blobs, key)?;
    let mut report = Report::default();

    if let Some(incoming) = incoming {
        for (id, path) in proposed(incoming)? {
            match take(&blobs, key, &id, &path, &mut held) {
                Ok(Some(line)) => report.added.push(line),
                Ok(None) => {}
                Err(e) => report.refused.push(format!("{}: {e}", path.display())),
            }
        }
    }

    let mut index = Index::new(0, 0);
    for ((plugin, version, _, _), build) in held {
        index.add(&plugin, &version, build);
    }
    apply(&mut index.plugins, policy);

    let unchanged = old.as_ref().is_some_and(|o| o.plugins == index.plugins);
    if unchanged && !refresh {
        report.serial = old.map_or(0, |o| o.serial);
        return Ok(report);
    }
    index.serial = old.map_or(0, |o| o.serial) + 1;
    index.expires = now + expires_days * 86400;
    write_index(dir, key, &index)?;
    report.written = true;
    report.serial = index.serial;
    Ok(report)
}

fn write_index(dir: &Path, key: &SecretKey, index: &Index) -> Result<(), String> {
    let text = index.to_json();
    write(&dir.join(INDEX), &text)?;
    write(&dir.join(INDEX_SIG), key.signature_line(&text).as_bytes())
}

/// A plugin's own feed (`docs/RELEASE.md`, A plugin's own bucket): its
/// bundles signed into `blobs/` of `dir` and added to the index there,
/// which is read, checked against `key`, and written again, signed. The
/// plugin is its feed's only writer, so the index is added to, not
/// rebuilt: `dir` need hold the index only, not the blobs it names. A
/// build of the same version, architecture and kernel is replaced. With
/// no bundles the index is signed again with a new expires.
///
/// `dir/blobs/` then holds the new blobs only: what is to be uploaded.
pub fn publish(
    dir: &Path,
    key: &SecretKey,
    bundles: &[PathBuf],
    expires_days: u64,
    now: u64,
) -> Result<Report, String> {
    let old = read_index(dir, key)?;
    let mut index = old.clone().unwrap_or_else(|| Index::new(0, 0));
    let blobs = dir.join(BLOBS);
    std::fs::create_dir_all(&blobs).map_err(|e| format!("{}: {e}", blobs.display()))?;
    let mut report = Report::default();
    for path in bundles {
        let shown = path.display();
        let bytes = std::fs::read(path).map_err(|e| format!("{shown}: {e}"))?;
        let info = bundle::check(&bytes).map_err(|e| format!("{shown}: {e}"))?;
        let signed = routeh_plugin_bundle::sign(&bytes, key).map_err(|e| e.to_string())?;
        let build = Build::of(&info.arch, info.kernel.as_deref(), &signed);
        write(&blobs.join(build.digest()), &signed)?;
        report.added.push(added(&info, &build));
        index.add(&info.plugin, &info.version, build);
    }
    index.serial = old.map_or(0, |o| o.serial) + 1;
    index.expires = now + expires_days * 86400;
    write_index(dir, key, &index)?;
    report.written = true;
    report.serial = index.serial;
    Ok(report)
}

fn added(info: &bundle::Info, build: &Build) -> String {
    let mut line = format!("{} {} {}", info.plugin, info.version, info.arch);
    if let Some(k) = &info.kernel {
        line.push_str(&format!(" kernel {k}"));
    }
    line.push_str(&format!(": {}", build.path()));
    line
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// What a blob is the build of: plugin, version, architecture, kernel.
type Of = (String, String, String, Option<String>);

/// The blobs the feed holds, each checked: its name is its SHA-256, it is
/// signed by `key`, no two are of the same build.
fn held(blobs: &Path, key: &SecretKey) -> Result<BTreeMap<Of, Build>, String> {
    let mut names: Vec<PathBuf> = std::fs::read_dir(blobs)
        .map_err(|e| format!("{}: {e}", blobs.display()))?
        .map(|e| e.map(|e| e.path()).map_err(|e| e.to_string()))
        .collect::<Result<_, _>>()?;
    names.sort();
    let mut held = BTreeMap::new();
    for path in names {
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        if name.starts_with('.') {
            continue;
        }
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        if sha256_hex(&bytes) != name {
            return Err(format!("blobs/{name}: not the SHA-256 of its bytes"));
        }
        let info = bundle::check(&bytes).map_err(|e| format!("blobs/{name}: {e}"))?;
        if info.signer != Some(key.public()) {
            return Err(format!("blobs/{name}: not signed by the feed's key"));
        }
        let build = Build::of(&info.arch, info.kernel.as_deref(), &bytes);
        let of = (info.plugin, info.version, info.arch, info.kernel);
        if let Some(other) = held.insert(of, build) {
            return Err(format!(
                "blobs/{name} and {}: the same build twice",
                other.path()
            ));
        }
    }
    Ok(held)
}

/// `incoming/<id>/*.tar`, in order.
fn proposed(incoming: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    let mut out = Vec::new();
    let read = |p: &Path| std::fs::read_dir(p).map_err(|e| format!("{}: {e}", p.display()));
    for plugin in read(incoming)? {
        let plugin = plugin.map_err(|e| e.to_string())?;
        if !plugin.file_type().map_err(|e| e.to_string())?.is_dir() {
            continue;
        }
        let id = plugin.file_name().to_string_lossy().into_owned();
        for file in read(&plugin.path())? {
            let path = file.map_err(|e| e.to_string())?.path();
            if path.extension().is_some_and(|x| x == "tar") {
                out.push((id.clone(), path));
            }
        }
    }
    out.sort();
    Ok(out)
}

/// A proposed bundle signed into `blobs/`; `None` when the feed holds it.
fn take(
    blobs: &Path,
    key: &SecretKey,
    id: &str,
    path: &Path,
    held: &mut BTreeMap<Of, Build>,
) -> Result<Option<String>, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let info = bundle::check(&bytes)?;
    if info.plugin != id {
        return Err(format!("a bundle of {}, proposed as {id}'s", info.plugin));
    }
    let signed = routeh_plugin_bundle::sign(&bytes, key).map_err(|e| e.to_string())?;
    let build = Build::of(&info.arch, info.kernel.as_deref(), &signed);
    let of = (
        info.plugin.clone(),
        info.version.clone(),
        info.arch.clone(),
        info.kernel.clone(),
    );
    if held.get(&of).is_some_and(|b| b.blob == build.blob) {
        return Ok(None);
    }
    write(&blobs.join(build.digest()), &signed)?;
    let mut line = added(&info, &build);
    if let Some(old) = held.insert(of, build) {
        std::fs::remove_file(blobs.join(old.digest()))
            .map_err(|e| format!("{}: {e}", old.path()))?;
        line.push_str(&format!(", replacing {}", old.path()));
    }
    Ok(Some(line))
}

/// `yank` and `keep` of the policy.
fn apply(plugins: &mut BTreeMap<String, Vec<Version>>, policy: &Policy) {
    for (id, versions) in plugins.iter_mut() {
        let Some(p) = policy.plugins.get(id) else {
            continue;
        };
        versions.retain(|v| !p.yank.contains(&v.version));
        if let Some(keep) = p.keep {
            // Newest first: count each architecture and kernel's builds.
            let mut seen: BTreeMap<(String, Option<String>), usize> = BTreeMap::new();
            for v in versions.iter_mut() {
                v.builds.retain(|b| {
                    let n = seen.entry((b.arch.clone(), b.kernel.clone())).or_default();
                    *n += 1;
                    *n <= keep
                });
            }
        }
        versions.retain(|v| !v.builds.is_empty());
    }
    plugins.retain(|_, v| !v.is_empty());
}

/// The (architecture, kernel) pairs of `wanted` that the index has no build
/// of `plugin`'s `version` for. A build with no kernel serves every kernel.
pub fn missing<'a>(
    index: Option<&Index>,
    plugin: &str,
    version: &str,
    wanted: &'a [(String, Option<String>)],
) -> Vec<&'a (String, Option<String>)> {
    let builds: Vec<&Build> = index
        .into_iter()
        .flat_map(|i| i.plugins.get(plugin).into_iter().flatten())
        .filter(|v| v.version == version)
        .flat_map(|v| &v.builds)
        .collect();
    wanted
        .iter()
        .filter(|(arch, kernel)| {
            !builds
                .iter()
                .any(|b| &b.arch == arch && (b.kernel.is_none() || b.kernel == *kernel))
        })
        .collect()
}
