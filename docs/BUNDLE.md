# The bundle and the feed

What the router checks before anything of a plugin is put in place, and
what an author's build and a feed write: one crate, `routeh-plugin-bundle`,
for both sides, so that what is made and what is taken cannot differ.
The design is `docs/PLUGINS.md` of the router's repository (The bundle
format, The feed); this is the format.

## The bundle

A tar archive, read and written by the `tar` crate with the bundle's
rules on top (`tar::read`, `tar::write`): regular files and directories
only.

- Refused: a hard or symbolic link, a device, a FIFO or any other kind of
  entry; a path that is not UTF-8, absolute, with a `..`, an empty
  segment or a control character; a path twice, a path under a file.
- A long path is a pax `path` record, a GNU long name or ustar's
  `prefix`, as the `tar` crate reads them.
- At most 1024 files, 32 MiB a file and 64 MiB in all (`tar::Limits`).

```
manifest.toml
bin/…                     the service's programs
modules/<name>.ko         a module the manifest loads, by its name
…                         anything else the plugin needs
SHA256SUMS                the list of digests
SHA256SUMS.sig            its signature
```

- `SHA256SUMS` has a line `<sha256 in lowercase hex>  <path>` (two
  spaces) for every file but itself and its signature, in the order of
  the paths, each once. A file that is not listed, a listed file that is
  not there and a file whose digest differs are refused.
- `SHA256SUMS.sig` is one line, `ed25519 <base64 of the public key>
  <base64 of the 64-byte signature of SHA256SUMS>` (RFC 8032). The key
  must be one the router trusts.
- The manifest is read as `routeh-plugin-protocol` reads it, and must
  name a service command that is in the bundle and a `modules/<name>.ko`
  for each module it loads.
- `Bundle::open` (the router) checks in this order: the archive, the
  signature by a trusted key, the list against every file, the manifest
  and the files it names. Nothing past the signature is read of a bundle
  nobody trusted signed.
- `Bundle::read` (a build) does the same checks but trust: an unsigned
  bundle is taken, a signed one when its signature holds, by any key.

Packing (`pack`) is deterministic: files in the order of their paths,
mode 0644 (0755 under `bin/`), owner 0, time 0. The same files and key
give the same bytes. Without a key it packs an unsigned bundle, which is
how a build makes one; `sign` signs a bundle as it is, replacing any
signature it had. A path is refused when packing as it would be when
read.

## Keys

Each is one line of text:

- a public key: `ed25519 <base64 of 32 bytes> [comment]`;
- a secret key, the author's alone: `ed25519-secret <base64 of the
  32-byte seed>`;
- a signature: `ed25519 <base64 of the public key> <base64 of 64 bytes>`.

Which keys a router trusts, and where it keeps them, is the router's:
it gives them as `TrustedKeys` (`new`, or `with_files` of its `*.pub`
files). `signer_of` says which key signed a line, trusted or not.

Keys, bundles and feeds are made by `routeh-plugin` (routeh-plugin-build:
`keygen`, `sign`, `verify`, `feed`, `publish`). `keygen` never writes
over a file (secret 0600, public 0644).

## The feed

```
<feed>/
  index.json          what the resolver needs
  index.json.sig      its signature: a line like SHA256SUMS.sig
  blobs/<sha256>      each bundle, by the SHA-256 of its bytes in hex
```

```json
{"feed": 1, "serial": 42, "expires": 1798675200,
 "plugins": {
   "awg": [
     {"version": "0.3.2",
      "builds": [
        {"arch": "aarch64_generic",
         "kernel": "6.12.112~67e205f26d0458c4d12f1fd315216398-r1",
         "blob": "sha256:<64 hex>", "size": 123456}]}]}}
```

- `feed` is the format, 1; unknown keys are refused. Each plugin id
  follows the manifest's rule.
- `expires` is seconds since 1970 UTC: an expired index is used and
  warned about (`Index::expired`).
- `serial`: the router keeps the newest one it took and refuses an index
  with a lower one.
- `kernel` is there for a build that carries modules: it fits only that
  `kernel` package version. A build without it fits any kernel.
- No version twice in a plugin, no two builds of a version for the same
  architecture and kernel.
- The resolver (`Index::resolve`): the newest version (runs of digits
  compared as numbers, other runs as text, `feed::newer`) with a build
  for the router's `OPENWRT_ARCH` and, if it has a `kernel`, for the
  installed one. A downloaded blob is checked by its size and digest
  (`Build::check`), then as any bundle.
- The same types are written: `Index::new`, `Index::add` (a build of the
  same version, architecture and kernel replaces the one there; versions
  stay newest first), `Index::to_json`, signed with
  `SecretKey::signature_line`. `Index::parse` reads an index without its
  signature.

## Kernel modules

`modinfo::read` reads a `.ko`'s `.modinfo` section (ELF, 32 or 64 bits,
either byte order): its `vermagic`, `depends`, `name` and `srcversion`,
which `install` compares with the running kernel.
