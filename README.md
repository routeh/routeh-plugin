# routeh-plugin

Plugins of the routeh router. A plugin extends the router without
changing it: a signed bundle of a manifest, one service and, if it needs
them, kernel modules. The router tells the service what to be over a
Unix socket; the service says what it is.

This repository holds what every plugin shares: the contract, the SDK a
service is written with, the bundle format, and the tools that build and
release a plugin.

| Crate | What |
|---|---|
| `routeh-plugin-protocol` | the contract: the manifest, its settings' types, the channel. The router's side too |
| `routeh-plugin-sdk` | a plugin's service in Rust: `converge` and the rest is done |
| `routeh-plugin-bundle` | the bundle and the feed: read by the router, written by builds and feeds |
| `routeh-plugin-build` | a plugin built for OpenWrt, tested in QEMU, released; its `cargo xtask` |

## Docs

- [PROTOCOL.md](docs/PROTOCOL.md): the channel and the manifest, for
  any plugin in any language
- [SDK.md](docs/SDK.md): writing a plugin in Rust
- [BUILD.md](docs/BUILD.md): a plugin's repository, `build.toml`, building
  and testing
- [RELEASE.md](docs/RELEASE.md): to the managed feed or a plugin's own S3
- [BUNDLE.md](docs/BUNDLE.md): the bundle, keys and the feed, byte by byte

## Layout

```
crates/               the four crates
.github/workflows/    plugin.yml and feed.yml (reusable), ci.yml
templates/plugin/     what a plugin's repository adds
templates/feed/       the feed's repository
docs/
```

## Here

```
make test       the tests of every crate
make lint       rustfmt and clippy, warnings denied
make coverage   at least 90% of lines in each crate
```
