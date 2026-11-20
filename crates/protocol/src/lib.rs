//! The contract between the routeh router and a plugin: a plugin's
//! manifest and the types of its settings (what `install` checks before a
//! bundle is put in place, and what validation reads to check a tunnel's
//! section of the document), and the channel between the router and the
//! plugin's service. Data only: no plugin code runs here, and nothing
//! reads a bundle or the router. See `docs/PROTOCOL.md` of this
//! repository and, for the design, `docs/PLUGINS.md` and
//! `docs/TUNNELS.md` of the router's.
pub mod base64;
pub mod channel;
mod manifest;
pub mod names;
mod read;
mod schema;
pub mod sha256;
mod toml_text;
mod value;

pub use manifest::{
    DeclaredAction, FormSection, Manifest, Mtu, Page, Service, StatusType, StatusValue, TunnelKind,
};
pub use schema::{Field, FieldType, Protocol, Reference, Role, Schema, SettingError, Width};
pub use toml_text::toml_string;
pub use value::{Referenced, Settings, Value};

pub use names::FieldKey;
use names::PluginId;
use std::collections::BTreeMap;

/// The version of the manifest's schema this crate reads: a manifest
/// names it as `api`, and one written for another is refused.
pub const API: u64 = 1;

/// The manifest's file, in a bundle and in an installed plugin's
/// directory. Where those directories are is the router's.
pub const MANIFEST_FILE: &str = "manifest.toml";

/// The keys every tunnel has: no plugin may have one of them as its id,
/// since a kind is a table beside them (`[tunnels.<key>.<id>]`).
pub const TUNNEL_KEYS: [&str; 4] = ["name", "policy", "mtu", "check"];

/// A manifest that `install` refuses, at the path of what it is about
/// (`tunnel.settings.jc.max`).
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{at}: {reason}")]
pub struct ManifestError {
    pub at: String,
    pub reason: String,
}

impl ManifestError {
    pub(crate) fn new(at: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            at: at.into(),
            reason: reason.into(),
        }
    }
}

/// The installed plugins' manifests, by id.
#[derive(Clone, Debug, Default)]
pub struct Catalog {
    plugins: BTreeMap<PluginId, Manifest>,
}

impl Catalog {
    /// One manifest of each id.
    pub fn new(manifests: impl IntoIterator<Item = Manifest>) -> Result<Self, ManifestError> {
        let mut plugins = BTreeMap::new();
        for manifest in manifests {
            let id = manifest.id.clone();
            if plugins.insert(id.clone(), manifest).is_some() {
                return Err(ManifestError::new(
                    id.as_str(),
                    "two manifests have this id",
                ));
            }
        }
        Ok(Self { plugins })
    }

    pub fn ids(&self) -> impl Iterator<Item = &PluginId> {
        self.plugins.keys()
    }

    pub fn get(&self, id: &str) -> Option<&Manifest> {
        self.plugins.get(id)
    }

    /// The kind of tunnel the plugin `id` provides, if it is installed and
    /// provides one.
    pub fn tunnel_kind(&self, id: &str) -> Option<&TunnelKind> {
        self.get(id)?.tunnel.as_ref()
    }
}
