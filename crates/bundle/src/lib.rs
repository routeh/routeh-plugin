//! A routeh plugin's bundle and feed, one implementation for both sides:
//! the router opens them (`Bundle::open`, `feed::Index::open`), an
//! author's build and a feed make and sign them (`pack`, `sign`,
//! `feed::Index`). Pure: everything is given as bytes; nothing reads a
//! router or a file. See `docs/BUNDLE.md`.
#![forbid(unsafe_code)]
mod bundle;
pub mod feed;
mod keys;
pub mod modinfo;
pub mod tar;

pub use bundle::{Bundle, BundleError, MANIFEST, SIGNATURE, SUMS, module_path, pack, sign};
pub use keys::{KeyError, PublicKey, SecretKey, Signature, TrustedKeys, signer_of};
