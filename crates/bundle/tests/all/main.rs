//! Every test of the crate in one binary: each file beside this one is a
//! module of it.
mod bundle;
mod feed;
mod keys;
mod modinfo;
mod tar;
mod versions;

use routeh_plugin_bundle::SecretKey;

/// A key made from a fixed seed, so that what it signs is the same in
/// every run.
pub fn key(seed: u8) -> SecretKey {
    SecretKey::from_seed([seed; 32])
}

/// The manifest of a plugin with a module.
pub const MANIFEST: &str = "id = \"stand_in\"\nname = \"Stand-in\"\nversion = \"1.0.0\"\napi = 1\n\n[modules]\nload = [\"dummy\"]\nkernel = \"6.12.112~67e205f26d0458c4d12f1fd315216398-r1\"\n\n[service]\ncommand = \"bin/stand-in\"\ncapabilities = [\"CAP_NET_ADMIN\"]\n";

#[path = "../support/elf.rs"]
mod elf_support;
pub use elf_support::{DUMMY_MODINFO, dummy_ko, elf};
