//! A feed of plugins: `index.json`, signed by `index.json.sig`, says
//! which builds of which versions there are, each a blob addressed by its
//! SHA-256 (`blobs/<hex>`). The same types are read by the router
//! (`Index::open`, `Index::resolve`) and written by a feed (`Index::add`,
//! `Index::to_json`), so the two cannot differ. The resolver needs no
//! solver: the newest version with a build for the router.
use crate::keys::{Signature, TrustedKeys};
use routeh_plugin_protocol::names::PluginId;
use routeh_plugin_protocol::sha256::{hex, sha256};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

/// The format of `index.json` this crate reads and writes.
pub const FORMAT: u64 = 1;
pub const INDEX: &str = "index.json";
pub const INDEX_SIG: &str = "index.json.sig";

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct IndexError(pub String);

fn refuse<T>(reason: impl Into<String>) -> Result<T, IndexError> {
    Err(IndexError(reason.into()))
}

/// `index.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Index {
    pub feed: u64,
    pub serial: u64,
    /// Seconds since 1970 UTC.
    pub expires: u64,
    /// By plugin id: its versions, newest first.
    pub plugins: BTreeMap<String, Vec<Version>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Version {
    pub version: String,
    /// In the order of architecture and kernel.
    pub builds: Vec<Build>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Build {
    /// `OPENWRT_ARCH`.
    pub arch: String,
    /// The `kernel` package version its modules are for; `None` for a
    /// plugin that loads no module.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kernel: Option<String>,
    /// `sha256:` and the SHA-256 of the bundle in lowercase hex.
    pub blob: String,
    pub size: u64,
}

impl Build {
    /// A build of these bytes: the signed bundle.
    pub fn of(arch: &str, kernel: Option<&str>, bundle: &[u8]) -> Self {
        Self {
            arch: arch.to_owned(),
            kernel: kernel.map(str::to_owned),
            blob: format!("sha256:{}", hex(&sha256(bundle))),
            size: bundle.len() as u64,
        }
    }
    /// The SHA-256 of the bundle in hex.
    pub fn digest(&self) -> &str {
        self.blob.strip_prefix("sha256:").unwrap_or(&self.blob)
    }
    /// Where the feed has its bundle, relative to the feed's address.
    pub fn path(&self) -> String {
        format!("blobs/{}", self.digest())
    }
    /// The downloaded blob is this build's.
    pub fn check(&self, bytes: &[u8]) -> Result<(), String> {
        if bytes.len() as u64 != self.size {
            return Err(format!(
                "the blob is {} bytes, not {}",
                bytes.len(),
                self.size
            ));
        }
        if hex(&sha256(bytes)) != self.digest() {
            return Err("the blob is not the digest the index says".into());
        }
        Ok(())
    }
}

/// What a build has to fit: the router's architecture (`OPENWRT_ARCH` of
/// `/etc/os-release`), its installed `kernel` package version, and its
/// release and board for what is said when nothing fits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Router {
    pub arch: String,
    pub kernel: String,
    pub release: String,
    pub board: String,
}

/// The build the resolver chose, and its version.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resolved<'a> {
    pub version: &'a str,
    pub build: &'a Build,
}

impl Index {
    /// An index of no plugin.
    pub fn new(serial: u64, expires: u64) -> Self {
        Self {
            feed: FORMAT,
            serial,
            expires,
            plugins: BTreeMap::new(),
        }
    }

    /// Checks the signature by a trusted key, then that the index is no
    /// older than the last one this router took (`seen`), then reads it.
    pub fn open(
        text: &[u8],
        signature: &str,
        trusted: &TrustedKeys,
        seen: Option<u64>,
    ) -> Result<Self, IndexError> {
        let (by, signature) = Signature::parse(signature)
            .map_err(|error| IndexError(format!("{INDEX_SIG}: {error}")))?;
        if !trusted.trusts(&by) {
            return refuse(format!(
                "{INDEX_SIG}: signed by a key the router does not trust"
            ));
        }
        if !by.verifies(text, &signature) {
            return refuse(format!("{INDEX_SIG}: the signature does not match {INDEX}"));
        }
        let index = Self::parse(text)?;
        if let Some(seen) = seen.filter(|seen| index.serial < *seen) {
            return refuse(format!(
                "{INDEX}: serial {} is older than {seen}, which this router has seen",
                index.serial
            ));
        }
        Ok(index)
    }

    /// Reads an index and checks it, its signature aside: the format,
    /// every plugin id, every blob's form, no version twice and no two
    /// builds of a version for the same architecture and kernel.
    pub fn parse(text: &[u8]) -> Result<Self, IndexError> {
        let index: Self = serde_json::from_slice(text)
            .map_err(|error| IndexError(format!("{INDEX}: {error}")))?;
        if index.feed != FORMAT {
            return refuse(format!(
                "{INDEX}: feed format {} is not {FORMAT}",
                index.feed
            ));
        }
        for (id, versions) in &index.plugins {
            if PluginId::new(id.as_str()).is_err() {
                return refuse(format!("{INDEX}: {id} is not a plugin id"));
            }
            let mut seen = BTreeSet::new();
            for version in versions {
                let at = format!("{INDEX}: {id} {}", version.version);
                if !seen.insert(&version.version) {
                    return refuse(format!("{at}: the version twice"));
                }
                let mut fits = BTreeSet::new();
                for build in &version.builds {
                    let digest = build.blob.strip_prefix("sha256:").filter(|digest| {
                        digest.len() == 64
                            && digest
                                .bytes()
                                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                    });
                    if digest.is_none() {
                        return refuse(format!(
                            "{at}: a blob is `sha256:` and 64 hexadecimal digits"
                        ));
                    }
                    if !fits.insert((&build.arch, &build.kernel)) {
                        return refuse(format!(
                            "{at}: two builds for {}{}",
                            build.arch,
                            build
                                .kernel
                                .as_ref()
                                .map(|k| format!(" kernel {k}"))
                                .unwrap_or_default()
                        ));
                    }
                }
            }
        }
        Ok(index)
    }

    /// The text of `index.json`: what is signed and served.
    pub fn to_json(&self) -> Vec<u8> {
        let mut text = serde_json::to_vec_pretty(self).expect("an index is JSON");
        text.push(b'\n');
        text
    }

    /// A build of `id` `version` added: one of the same architecture and
    /// kernel is replaced, which it returns. Versions stay newest first,
    /// builds in order.
    pub fn add(&mut self, id: &str, version: &str, build: Build) -> Option<Build> {
        let versions = self.plugins.entry(id.to_owned()).or_default();
        let at = match versions.iter().position(|v| v.version == version) {
            Some(at) => at,
            None => {
                versions.push(Version {
                    version: version.to_owned(),
                    builds: Vec::new(),
                });
                versions.len() - 1
            }
        };
        let builds = &mut versions[at].builds;
        let replaced = builds
            .iter()
            .position(|b| b.arch == build.arch && b.kernel == build.kernel)
            .map(|at| builds.remove(at));
        builds.push(build);
        builds.sort();
        versions.sort_by(|a, b| newer(&b.version, &a.version));
        replaced
    }

    /// The index is past its time at `now` (seconds since 1970): it is
    /// still used, and the person is warned.
    pub fn expired(&self, now: u64) -> bool {
        now > self.expires
    }

    /// The newest version of `id` with a build for the router's
    /// architecture and, when its build carries modules, for exactly its
    /// kernel; otherwise why there is none.
    pub fn resolve(&self, id: &str, router: &Router) -> Result<Resolved<'_>, IndexError> {
        let versions = self
            .plugins
            .get(id)
            .ok_or_else(|| IndexError(format!("{id} is not in the feed")))?;
        let for_arch: Vec<Resolved<'_>> = versions
            .iter()
            .flat_map(|v| {
                v.builds.iter().map(|build| Resolved {
                    version: &v.version,
                    build,
                })
            })
            .filter(|r| r.build.arch == router.arch)
            .collect();
        if for_arch.is_empty() {
            return refuse(format!("no build of {id} for {}", router.arch));
        }
        for_arch
            .into_iter()
            .filter(|r| {
                r.build
                    .kernel
                    .as_ref()
                    .is_none_or(|kernel| *kernel == router.kernel)
            })
            .max_by(|one, other| newer(one.version, other.version))
            .ok_or_else(|| {
                IndexError(format!(
                    "no build of {id} for kernel {} of {} {}",
                    router.kernel, router.release, router.board
                ))
            })
    }
}

/// How `one` is ordered against `other` as versions: runs of digits are
/// compared as numbers, other runs as text, and a version that goes on
/// is the newer one.
pub fn newer(one: &str, other: &str) -> Ordering {
    let (mut one, mut other) = (runs(one), runs(other));
    loop {
        match (one.next(), other.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(a), Some(b)) => {
                let order = match (a.parse::<u128>(), b.parse::<u128>()) {
                    (Ok(a), Ok(b)) => a.cmp(&b),
                    _ => a.cmp(b),
                };
                if order != Ordering::Equal {
                    return order;
                }
            }
        }
    }
}

/// The runs of digits and of everything else.
fn runs(text: &str) -> impl Iterator<Item = &str> {
    let mut rest = text;
    std::iter::from_fn(move || {
        let first = rest.chars().next()?;
        let digit = first.is_ascii_digit();
        let end = rest
            .find(|c: char| c.is_ascii_digit() != digit)
            .unwrap_or(rest.len());
        let (run, tail) = rest.split_at(end);
        rest = tail;
        Some(run)
    })
}
