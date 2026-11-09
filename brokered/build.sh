#!/usr/bin/env bash
# A brokered connector's server image (connectors spec, Brokered API
# connectors: "one server image per provider, identified in its manifest by
# digest"): the package's program, built static for the microVM, as the
# `/init` of an initramfs (a newc cpio with fixed times and owners), so the
# microVM has no disk at all. Built reproducibly (the pinned toolchain, the
# locked dependencies, build paths remapped), so the digest a manifest names
# is the digest of the image built from the same commit anywhere.
# Usage: brokered/build.sh <package> <out dir>. Writes <out>/<hex>.cpio and
# prints the digest as a manifest names it: sha256:<hex>.
set -euo pipefail
pkg="$1"; out="$2"
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/.." && pwd)"
target="${CARGO_TARGET_DIR:-$root/target}"
cargo_home="${CARGO_HOME:-$HOME/.cargo}"
map="--remap-path-prefix=$root=/magnus --remap-path-prefix=$cargo_home=/cargo --remap-path-prefix=$target=/target"
# ring's C is compiled by the host's gcc (no musl-gcc needed: it uses no libc).
CC_x86_64_unknown_linux_musl="${CC_x86_64_unknown_linux_musl:-gcc}" \
CFLAGS_x86_64_unknown_linux_musl="-ffile-prefix-map=$root=/magnus -ffile-prefix-map=$cargo_home=/cargo -ffile-prefix-map=$target=/target" \
RUSTFLAGS="$map -C strip=symbols" \
  cargo build --release --locked -p "$pkg" --target x86_64-unknown-linux-musl --manifest-path "$root/Cargo.toml" >&2
tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/root"
cp "$target/x86_64-unknown-linux-musl/release/$pkg" "$tmp/root/init"
chmod 0755 "$tmp/root/init"
touch -d @0 "$tmp/root/init"
(cd "$tmp/root" && printf 'init\n' | cpio --reproducible -o -H newc -R 0:0 --quiet) > "$tmp/image.cpio"
h="$(sha256sum "$tmp/image.cpio" | cut -d' ' -f1)"
mkdir -p "$out"
mv "$tmp/image.cpio" "$out/$h.cpio"
echo "sha256:$h"
