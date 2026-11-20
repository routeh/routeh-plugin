//! The shell scripts and the Dockerfile the build runs, in the crate so
//! that a plugin's repository holds no copy of them. Written out under
//! `target/routeh/scripts/` before they run.

use std::path::PathBuf;

use crate::cmd::{mkdir, write};
use crate::project::Project;

/// A module built with the OpenWrt SDK of a release and target.
pub const KMOD_BUILD: &str = include_str!("../openwrt/kmod-build.sh");
/// A plugin's modules and tests on OpenWrt in QEMU.
pub const QEMU_TEST: &str = include_str!("../openwrt/qemu-test.sh");
/// The SDK's host.
pub const DOCKERFILE: &str = include_str!("../openwrt/Dockerfile");

/// The scripts written out; their directory.
pub fn write_all(p: &Project) -> Result<PathBuf, String> {
    let dir = p.out().join("scripts");
    mkdir(&dir)?;
    write(&dir.join("kmod-build.sh"), KMOD_BUILD.as_bytes())?;
    write(&dir.join("qemu-test.sh"), QEMU_TEST.as_bytes())?;
    Ok(dir)
}
