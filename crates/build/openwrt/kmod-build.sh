# One kernel module of a plugin, built with the OpenWrt SDK of RELEASE and
# TARGET. Run by `cargo xtask kmod` (routeh-plugin-build): in the image of
# its Dockerfile (amd64), or with --native on an x86_64 Linux host that
# has the SDK's packages (CI). Writes $MODULE.ko and $MODULE.json.
#
#   RELEASE, TARGET  the OpenWrt release and target
#   MODULE           the module's name: what Kbuild makes, $MODULE.ko
#   MODULE_DIR       its OpenWrt Makefile's directory
#   SUBDIR           where its Kbuild is in its source (src)
#   PATCHES          a directory of *.patch applied to a git source, if any
#   VERSION_FILE     its version: a file of its source with a #define ...
#   VERSION_DEFINE   ... of this name; else the Makefile's PKG_VERSION
#   SDK_DIR          SDKs and sources, kept between builds
#   OUT_DIR          where $MODULE.ko and $MODULE.json go
#
# The source is the Makefile's PKG_SOURCE_URL at PKG_SOURCE_VERSION (git),
# or, with no PKG_SOURCE_URL, MODULE_DIR itself.
set -eu
SUBDIR=${SUBDIR:-src}
PATCHES=${PATCHES:-}
VERSION_FILE=${VERSION_FILE:-}
VERSION_DEFINE=${VERSION_DEFINE:-}
# A fixed path, not mktemp: the path of the sources goes into the module,
# and a module built twice has to be the same file (the same build id).
work=/tmp/routeh-kmod/$MODULE
rm -rf "$work" && mkdir -p "$work"
makefile="$MODULE_DIR/Makefile"
[ -f "$makefile" ] || { echo "no $makefile" >&2; exit 1; }

base="https://downloads.openwrt.org/releases/$RELEASE/targets/$TARGET"
sums=$(curl -fsS "$base/sha256sums")
name=$(echo "$sums" | awk '/openwrt-sdk-.*\.tar\.zst$/ { sub(/^\*/, "", $2); print $2 }')
sum=$(echo "$sums" | awk -v n="*$name" '$2 == n { print $1 }')
[ -n "$name" ] && [ -n "$sum" ] || { echo "no SDK at $base" >&2; exit 1; }

mkdir -p "$SDK_DIR" "$OUT_DIR"
cd "$SDK_DIR"
if ! echo "$sum  $name" | sha256sum -c --status 2>/dev/null; then
  echo "downloading $name" >&2
  curl -fsS -o "$name" "$base/$name"
  echo "$sum  $name" | sha256sum -c --status || { echo "SDK checksum mismatch" >&2; exit 1; }
fi
dir="$SDK_DIR/${name%.tar.zst}"
[ -d "$dir" ] || tar --zstd -xf "$name"
cd "$dir"
# A fresh SDK has no .config: without one OpenWrt knows no target, and so no
# kernel version (include/kernel-version.mk stops).
if [ ! -f .config ]; then
  make defconfig > "$work/defconfig.log" 2>&1 || { tail -30 "$work/defconfig.log" >&2; exit 1; }
fi

# The source, and what it is.
var() { sed -n "s/^$1:=//p" "$makefile"; }
url=$(var PKG_SOURCE_URL)
patches=$( { [ -n "$PATCHES" ] && cat "$PATCHES"/*.patch 2>/dev/null; } | sha256sum | cut -c1-12)
if [ -n "$url" ]; then
  commit=$(var PKG_SOURCE_VERSION)
  [ -n "$commit" ] || { echo "$makefile: PKG_SOURCE_URL without PKG_SOURCE_VERSION" >&2; exit 1; }
  src="$SDK_DIR/src/$MODULE-$commit-$patches"
  if [ ! -d "$src" ]; then
    rm -rf "$src.tmp"
    git clone -q "$url" "$src.tmp"
    git -C "$src.tmp" -c advice.detachedHead=false checkout -q "$commit"
    [ "$(git -C "$src.tmp" rev-parse HEAD)" = "$commit" ] || { echo "not at $commit" >&2; exit 1; }
    if [ -n "$PATCHES" ]; then
      for p in "$PATCHES"/*.patch; do [ -e "$p" ] && git -C "$src.tmp" apply "$p"; done
    fi
    mv "$src.tmp" "$src"
  fi
  upstream=$commit
else
  src=$MODULE_DIR
  upstream="local:$(cd "$src" && find . -type f | LC_ALL=C sort | xargs sha256sum | sha256sum | cut -c1-12)"
  patches=""
fi
cp -r "$src/$SUBDIR" "$work/mod"

# The module alone, against the SDK's kernel, with the SDK's toolchain and
# the kernel make flags OpenWrt uses for its own kmods (KERNEL_MAKE of
# include/kernel.mk). Not `make package/...`: that builds and packs every
# kmod of package/kernel/linux the module's package depends on.
cat > "$work/kmod.mk" <<'MK'
TOPDIR:=$(CURDIR)
export TOPDIR
include rules.mk
include $(INCLUDE_DIR)/kernel.mk
build:
	+$(KERNEL_MAKE) M=$(MOD) modules
val.%:
	@echo $($*)
MK
val() { make --no-print-directory -s -f "$work/kmod.mk" "val.$1"; }
if ! make --no-print-directory -s -f "$work/kmod.mk" build MOD="$work/mod" -j"$(nproc)" > "$work/build.log" 2>&1; then
  tail -60 "$work/build.log" >&2; exit 1
fi
ko="$work/mod/$MODULE.ko"
[ -f "$ko" ] || { echo "no $MODULE.ko built" >&2; exit 1; }
got="$(val LINUX_VERSION)~$(val LINUX_VERMAGIC)-r$(val LINUX_RELEASE)"

# The release's own kernel package must have the same version: else the
# hash is not the firmware's and the module must not be shipped.
want=$(curl -fsS "$base/profiles.json" | python3 -c 'import sys,json; k=json.load(sys.stdin)["linux_kernel"]; print("%s~%s-r%s" % (k["version"], k["vermagic"], k["release"]))')
[ "$want" = "$got" ] || { echo "kernel package version $got is not the release's $want" >&2; exit 1; }

if [ -n "$VERSION_FILE" ]; then
  version=$(sed -n "s/^#define $VERSION_DEFINE \"\(.*\)\"$/\1/p" "$src/$VERSION_FILE")
  [ -n "$version" ] || { echo "no #define $VERSION_DEFINE in $VERSION_FILE" >&2; exit 1; }
else
  version=$(var PKG_VERSION)
fi

cp "$ko" "$OUT_DIR/$MODULE.ko"
# OpenWrt's kernels strip version, description and srcversion from modules
# (CONFIG_MODULE_STRIPPED); the build id stays in the file.
build_id=$(readelf -n "$OUT_DIR/$MODULE.ko" | sed -n 's/.*Build ID: //p')
[ -n "$build_id" ] || { echo "no build id in $MODULE.ko" >&2; exit 1; }
cat > "$OUT_DIR/$MODULE.json" <<JSON
{
  "release": "$RELEASE",
  "target": "$TARGET",
  "kernel_package": "$got",
  "vermagic": "$(modinfo -F vermagic "$OUT_DIR/$MODULE.ko")",
  "build_id": "$build_id",
  "module_version": "$version",
  "depends": "$(modinfo -F depends "$OUT_DIR/$MODULE.ko")",
  "upstream": "$upstream",
  "patches": "$patches"
}
JSON
