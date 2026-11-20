//! The bundle's container: a tar archive, read and written by the `tar`
//! crate, with the bundle's rules on top.
//!
//! Read: regular files and directories only. A link, a device, a FIFO
//! or any other kind of entry, a path that is not UTF-8, absolute, with
//! `..`, an empty segment or a control character, a path twice or under a
//! file are refused, and every size is bounded. Long paths come as the
//! `tar` crate reads them: a pax `path` record, a GNU long name or
//! ustar's `prefix`.
//!
//! Written: the files in the order given, mode 0644 (0755 under `bin/`),
//! owner 0, time 0, so that the same files give the same bytes; each path
//! is refused there as it would be when read.
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;

/// How much an archive may hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub files: usize,
    pub file_bytes: usize,
    pub total_bytes: usize,
}

impl Default for Limits {
    /// 1024 files, 32 MiB a file, 64 MiB in all.
    fn default() -> Self {
        Self {
            files: 1024,
            file_bytes: 32 << 20,
            total_bytes: 64 << 20,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{}", match at { Some(at) => format!("{at}: {reason}"), None => reason.clone() })]
pub struct TarError {
    pub at: Option<String>,
    pub reason: String,
}

fn refuse<T>(at: &str, reason: impl Into<String>) -> Result<T, TarError> {
    Err(TarError {
        at: Some(at.to_owned()),
        reason: reason.into(),
    })
}

fn malformed(error: std::io::Error) -> TarError {
    TarError {
        at: None,
        reason: format!("not a tar archive: {error}"),
    }
}

/// The regular files of the archive by path (without `./`); directories
/// are not kept.
pub fn read(bytes: &[u8], limits: &Limits) -> Result<BTreeMap<String, Vec<u8>>, TarError> {
    let mut archive = tar::Archive::new(bytes);
    let mut files = BTreeMap::new();
    let mut directories = BTreeSet::new();
    let mut total = 0usize;
    for entry in archive.entries().map_err(malformed)? {
        let mut entry = entry.map_err(malformed)?;
        let name = String::from_utf8(entry.path_bytes().into_owned()).or_else(|error| {
            refuse(
                &String::from_utf8_lossy(error.as_bytes()),
                "the path is not UTF-8",
            )
        })?;
        let kind = entry.header().entry_type();
        // A pax header for the whole archive says nothing of a file.
        if kind == tar::EntryType::XGlobalHeader {
            continue;
        }
        let directory = kind.is_dir();
        if !directory && !kind.is_file() {
            let said = match kind {
                tar::EntryType::Link => "a hard link".to_owned(),
                tar::EntryType::Symlink => "a symbolic link".to_owned(),
                tar::EntryType::Char | tar::EntryType::Block => "a device".to_owned(),
                tar::EntryType::Fifo => "a FIFO".to_owned(),
                other => format!("an entry of kind {}", char::from(other.as_byte())),
            };
            return refuse(&name, format!("{said} is not taken"));
        }
        let path = clean(&name)?;
        // Nothing may be under a file, and no file where a directory is.
        let mut parent = String::new();
        let segments: Vec<&str> = path.split('/').collect();
        for segment in &segments[..segments.len() - 1] {
            if !parent.is_empty() {
                parent.push('/');
            }
            parent.push_str(segment);
            if files.contains_key(&parent) {
                return refuse(&name, "under a file");
            }
            directories.insert(parent.clone());
        }
        if directory {
            if files.contains_key(&path) {
                return refuse(&name, "twice in the bundle");
            }
            directories.insert(path);
            continue;
        }
        if files.contains_key(&path) || directories.contains(&path) {
            return refuse(&path, "twice in the bundle");
        }
        if files.len() == limits.files {
            return refuse(&path, format!("more than {} files", limits.files));
        }
        let size = usize::try_from(entry.size()).unwrap_or(usize::MAX);
        if size > limits.file_bytes {
            return refuse(&path, format!("larger than {} bytes", limits.file_bytes));
        }
        total = total.saturating_add(size);
        if total > limits.total_bytes {
            return refuse(
                &path,
                format!("the bundle is larger than {} bytes", limits.total_bytes),
            );
        }
        let mut data = Vec::with_capacity(size);
        entry
            .read_to_end(&mut data)
            .or_else(|_| refuse(&path, "the archive ends inside the file"))?;
        files.insert(path, data);
    }
    Ok(files)
}

/// The path without `./` and a trailing `/`, refused when it would leave
/// the bundle or cannot be a file's name.
fn clean(name: &str) -> Result<String, TarError> {
    if name.starts_with('/') {
        return refuse(name, "an absolute path");
    }
    let trimmed = name.trim_start_matches("./").trim_end_matches('/');
    if trimmed.is_empty() {
        return refuse(name, "an empty segment");
    }
    for segment in trimmed.split('/') {
        match segment {
            "" | "." => return refuse(name, "an empty segment"),
            ".." => return refuse(name, "'..' is not taken"),
            _ if segment.chars().any(char::is_control) => {
                return refuse(name, "a control character");
            }
            _ => {}
        }
    }
    Ok(trimmed.to_owned())
}

/// An archive of these regular files, in the order given. A path is
/// refused unless `read` would take it as it is written.
pub fn write<'a>(
    files: impl IntoIterator<Item = (&'a str, &'a [u8])>,
) -> Result<Vec<u8>, TarError> {
    let mut builder = tar::Builder::new(Vec::new());
    for (path, data) in files {
        if clean(path)? != path {
            return refuse(path, "not a path as the bundle keeps it");
        }
        let mut header = tar::Header::new_ustar();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_size(data.len() as u64);
        header.set_mode(if path.starts_with("bin/") {
            0o755
        } else {
            0o644
        });
        header.set_uid(0);
        header.set_gid(0);
        header.set_mtime(0);
        builder
            .append_data(&mut header, path, data)
            .map_err(|error| TarError {
                at: Some(path.to_owned()),
                reason: error.to_string(),
            })?;
    }
    builder.into_inner().map_err(malformed)
}
