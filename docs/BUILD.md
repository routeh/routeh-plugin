# Building a plugin

Every plugin is built, bundled and tested the same way, by
`routeh-plugin-build`. A plugin's repository depends on it by git and
its `cargo xtask` is one line; the commands are the same on a laptop and
in CI. Releasing is [RELEASE.md](RELEASE.md).

## A plugin's repository

```
plugin/manifest.toml   the manifest; its version is the bundle's
build.toml             what to build: targets, service, modules, QEMU tests
crates/                the service and what it needs
kmod/<module>/         a module's OpenWrt Makefile, and patches/
xtask/                 the one-line wrapper
.cargo/config.toml     xtask = "run -q --package xtask --"
.github/workflows/
  build.yml            calls routeh-plugin's plugin.yml
```

`templates/plugin/` has `build.toml`, `xtask/` and `build.yml` to copy.

```rust
// xtask/src/main.rs
fn main() -> std::process::ExitCode {
    routeh_plugin_build::main()
}
```

A plugin's `Cargo.lock` pins the build; `cargo update -p
routeh-plugin-build` takes a newer one.

## build.toml

```toml
targets = ["armsr/armv8", "mediatek/filogic"]   # OpenWrt targets

[service]                      # the program of the manifest's command bin/<name>
package = "awg-ctl"            # its cargo package; <name> by default
version_env = "AWG_PLUGIN_VERSION"   # set to the plugin's version when built;
                                     # ROUTEH_PLUGIN_VERSION by default

[[module]]                     # one for each module of [modules] load
name = "amneziawg"
dir = "kmod/amneziawg"         # its OpenWrt Makefile; patches/ beside it
subdir = "src"                 # where its Kbuild is in its source ("src")
version_file = "src/version.h" # its version: a #define in its source ...
version_define = "WIREGUARD_VERSION"   # ... of this name; else PKG_VERSION
pins_version = true            # the plugin's version is <its version>-r<n>

[qemu]                         # the workspace's tests run in the guest
tests = ["live", "e2e"]
env = { AWG_LIVE = "1", AWG_REQUIRE = "amneziawg" }

[rust]                         # more OPENWRT_ARCH -> Rust targets
[module_packages]              # more kernel modules -> OpenWrt kmod packages
```

- A module's source is its Makefile's `PKG_SOURCE_URL` at
  `PKG_SOURCE_VERSION` (git), patched by `patches/*.patch`; with no
  `PKG_SOURCE_URL`, the files under `dir` themselves.
- The modules `build.toml` builds and those the manifest loads must be
  the same.
- A module's `depends` must each be one of the plugin's modules or a
  package in the manifest's `requires`.

## Commands

```
cargo xtask build [--openwrt R] [--targets T]   the service, the modules, the bundles, checked
cargo xtask qemu-test [--openwrt R]             the modules and [qemu] tests on OpenWrt
cargo xtask verify FILE... [--trust PUBLIC]     a bundle as the router checks it
cargo xtask keygen SECRET PUBLIC [COMMENT]      a key pair
cargo xtask                                     every command
```

`R` is a release (`25.12.6`) or `stable`; `T` is `all` (the targets of
`build.toml`) or a comma list. Everything is made under `target/routeh/`.

## How it builds

- **The service**: static, musl, for the Rust target of each
  architecture, in `rust-musl-cross` (Docker volume `routeh-cargo`).
- **A module**: with the OpenWrt SDK of the exact release and target, in
  an amd64 image (Docker volume `routeh-sdk`): the module alone, with
  the kernel's make flags, not `make package`. Its kernel package
  version must be the release's, and a module built twice is the same
  file.
- **`--native`**: the same on an x86_64 Linux host with the SDK's
  packages, no Docker: what CI does.
- **The bundle**: the manifest with `[modules] kernel` set, `bin/`,
  `modules/`, and `build.json` (what it was built from and for), packed
  unsigned and checked as the router checks it ([BUNDLE.md](BUNDLE.md)).
  A module is read as the router reads it: its name, vermagic and
  depends.
- **QEMU**: armsr/armv8 of the release (HVF on Apple Silicon, KVM on
  arm64 Linux, emulated elsewhere); the manifest's `requires` installed
  from a local copy of the release's kmods, the modules loaded, each
  `[qemu]` test run in the guest.

The scripts (`kmod-build.sh`, `qemu-test.sh`, the Dockerfile) are in the
crate and written out under `target/routeh/scripts/` when they run.
`ROUTEH_SDK_VOLUME` and `ROUTEH_CARGO_VOLUME` name other Docker volumes.
