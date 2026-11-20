//! A plugin's bundle: an archive of the plugin's files, `SHA256SUMS`
//! listing the digest of every one of them and `SHA256SUMS.sig`, the
//! signature of that list. Nothing of a bundle is used before all of it
//! is checked.
//!
//! An author's build makes a bundle unsigned (`pack` without a key);
//! whoever releases it signs it (`sign`). The router takes only a bundle
//! signed by a key it trusts (`Bundle::open`).
use crate::keys::{KeyError, PublicKey, SecretKey, Signature, TrustedKeys};
use crate::tar::{self, Limits};
use routeh_plugin_protocol::Manifest;
use routeh_plugin_protocol::sha256::{hex, sha256, unhex};
use std::collections::BTreeMap;

/// The list of digests.
pub const SUMS: &str = "SHA256SUMS";
/// Its signature.
pub const SIGNATURE: &str = "SHA256SUMS.sig";
pub const MANIFEST: &str = routeh_plugin_protocol::MANIFEST_FILE;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{at}: {reason}")]
pub struct BundleError {
    pub at: String,
    pub reason: String,
}

fn refuse<T>(at: &str, reason: impl Into<String>) -> Result<T, BundleError> {
    Err(BundleError {
        at: at.to_owned(),
        reason: reason.into(),
    })
}

impl From<tar::TarError> for BundleError {
    fn from(error: tar::TarError) -> Self {
        Self {
            at: error.at.unwrap_or_else(|| "the bundle".into()),
            reason: error.reason,
        }
    }
}

/// A bundle whose digests hold, whose signature (if any) holds, with its
/// manifest read.
#[derive(Debug)]
pub struct Bundle {
    manifest: Manifest,
    files: BTreeMap<String, Vec<u8>>,
    signer: Option<PublicKey>,
}

impl Bundle {
    /// What the router installs: a bundle signed by a key it trusts.
    /// Checks, in order: the archive, the signature of the list by a
    /// trusted key, the list against every file, then the manifest and
    /// that the files it names are there. Nothing past the signature is
    /// read of a bundle nobody trusted signed.
    pub fn open(bytes: &[u8], trusted: &TrustedKeys) -> Result<Self, BundleError> {
        Self::check(bytes, Some(trusted))
    }

    /// The same checks but trust: an unsigned bundle is taken, a signed
    /// one when its signature holds, by whatever key. What a build checks
    /// before it signs or releases a bundle.
    pub fn read(bytes: &[u8]) -> Result<Self, BundleError> {
        Self::check(bytes, None)
    }

    fn check(bytes: &[u8], trusted: Option<&TrustedKeys>) -> Result<Self, BundleError> {
        let mut files = tar::read(bytes, &Limits::default())?;
        let signature = files.remove(SIGNATURE);
        let Some(sums) = files.remove(SUMS) else {
            return refuse(SUMS, "not in the bundle");
        };
        let signer = match signature {
            Some(line) => {
                let (signer, signature) = Signature::parse(&String::from_utf8_lossy(&line))
                    .or_else(|KeyError(reason)| refuse(SIGNATURE, reason))?;
                if trusted.is_some_and(|t| !t.trusts(&signer)) {
                    return refuse(SIGNATURE, "signed by a key the router does not trust");
                }
                if !signer.verifies(&sums, &signature) {
                    return refuse(SIGNATURE, "the signature does not match SHA256SUMS");
                }
                Some(signer)
            }
            None if trusted.is_some() => return refuse(SIGNATURE, "not in the bundle"),
            None => None,
        };
        let listed = list(&sums)?;
        for (path, bytes) in &files {
            match listed.get(path) {
                None => return refuse(path, "not in SHA256SUMS"),
                Some(digest) if *digest != sha256(bytes) => {
                    return refuse(path, "not the digest SHA256SUMS lists");
                }
                Some(_) => {}
            }
        }
        if let Some(missing) = listed.keys().find(|path| !files.contains_key(*path)) {
            return refuse(missing, "listed in SHA256SUMS but not in the bundle");
        }
        let Some(text) = files.get(MANIFEST) else {
            return refuse(MANIFEST, "not in the bundle");
        };
        let text = std::str::from_utf8(text).or_else(|_| refuse(MANIFEST, "not UTF-8"))?;
        let manifest =
            Manifest::parse(text).or_else(|error| refuse(MANIFEST, error.to_string()))?;
        if !files.contains_key(&manifest.service.command) {
            return refuse(
                &manifest.service.command,
                "the service's command is not in the bundle",
            );
        }
        for module in &manifest.modules {
            let path = module_path(module);
            if !files.contains_key(&path) {
                return refuse(&path, "a module the manifest loads is not in the bundle");
            }
        }
        Ok(Self {
            manifest,
            files,
            signer,
        })
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }
    /// Every file but the list and its signature, by path.
    pub fn files(&self) -> &BTreeMap<String, Vec<u8>> {
        &self.files
    }
    /// The key that signed it; `None` for an unsigned bundle (`read`
    /// only: `open` takes none).
    pub fn signer(&self) -> Option<&PublicKey> {
        self.signer.as_ref()
    }
    /// The `.ko` of a module the manifest loads.
    pub fn module(&self, name: &str) -> Option<&[u8]> {
        self.files.get(&module_path(name)).map(Vec::as_slice)
    }
    /// A file is installed executable when it is a program: under `bin/`.
    pub fn executable(&self, path: &str) -> bool {
        path.starts_with("bin/")
    }
}

/// Where a bundle has a module's `.ko`.
pub fn module_path(name: &str) -> String {
    format!("modules/{name}.ko")
}

/// `SHA256SUMS`: a line `<64 lowercase hex>  <path>` for each file, in
/// the order of paths, each once.
fn list(sums: &[u8]) -> Result<BTreeMap<String, [u8; 32]>, BundleError> {
    let text = std::str::from_utf8(sums).or_else(|_| refuse(SUMS, "not UTF-8"))?;
    let mut listed = BTreeMap::new();
    let mut last: Option<&str> = None;
    for (number, line) in text.lines().enumerate() {
        let number = number + 1;
        let entry = line
            .split_once("  ")
            .filter(|(digest, _)| !digest.bytes().any(|b| b.is_ascii_uppercase()))
            .and_then(|(digest, path)| Some((<[u8; 32]>::try_from(unhex(digest)?).ok()?, path)))
            .filter(|(_, path)| !path.is_empty());
        let Some((digest, path)) = entry else {
            return refuse(
                SUMS,
                format!("line {number} is not `<sha256 in hex>  <path>`"),
            );
        };
        if last.is_some_and(|last| last >= path) {
            return refuse(SUMS, format!("line {number} is not in the order of paths"));
        }
        last = Some(path);
        listed.insert(path.to_owned(), digest);
    }
    Ok(listed)
}

/// A bundle of these files (the manifest among them): the list of their
/// digests, signed by `key` when there is one. The same files and key give
/// the same bytes. Nothing is checked here but the paths: `Bundle::read`
/// checks what was packed.
pub fn pack<'a>(
    files: impl IntoIterator<Item = (&'a str, &'a [u8])>,
    key: Option<&SecretKey>,
) -> Result<Vec<u8>, BundleError> {
    let mut sorted: BTreeMap<&str, &[u8]> = BTreeMap::new();
    for (path, bytes) in files {
        if path == SUMS || path == SIGNATURE {
            return refuse(path, "a name the bundle keeps for itself");
        }
        if sorted.insert(path, bytes).is_some() {
            return refuse(path, "twice in the bundle");
        }
    }
    let sums: String = sorted
        .iter()
        .map(|(path, bytes)| format!("{}  {path}\n", hex(&sha256(bytes))))
        .collect();
    let signature = key.map(|key| key.signature_line(sums.as_bytes()));
    let mut entries: Vec<(&str, &[u8])> = sorted.into_iter().collect();
    entries.push((SUMS, sums.as_bytes()));
    if let Some(signature) = &signature {
        entries.push((SIGNATURE, signature.as_bytes()));
    }
    Ok(tar::write(entries)?)
}

/// The bundle signed by `key`, whether it was signed before or not: its
/// files as they are, a new signature. A bundle `read` refuses is not
/// signed.
pub fn sign(bundle: &[u8], key: &SecretKey) -> Result<Vec<u8>, BundleError> {
    let bundle = Bundle::read(bundle)?;
    pack(
        bundle
            .files()
            .iter()
            .map(|(p, b)| (p.as_str(), b.as_slice())),
        Some(key),
    )
}
