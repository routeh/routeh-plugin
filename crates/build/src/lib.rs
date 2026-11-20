//! Build, pack, sign and publish routeh router plugins, the same way for
//! every plugin, on a laptop and in CI (`docs/BUILD.md`, `docs/RELEASE.md`).
//!
//! A plugin's repository depends on this crate and its `cargo xtask` is
//! [`main`]; the feed's workflow runs the same commands as the
//! `routeh-plugin` program.
//!
//! - The bundle's and the feed's format is [`routeh_plugin_bundle`], the
//!   router's own; [`bundle`] adds what the build knows (build.json,
//!   names, versions), [`feed`] the runs that fill a feed.
//! - [`project`], [`openwrt`], [`docker`], [`make`], [`qemu`]: a plugin's
//!   repository built for OpenWrt's targets and tested in QEMU.

pub mod bundle;
mod cli;
pub mod cmd;
pub mod docker;
pub mod feed;
pub mod make;
pub mod openwrt;
pub mod project;
pub mod qemu;
pub mod scripts;

pub use cli::main;
pub use routeh_plugin_bundle;
