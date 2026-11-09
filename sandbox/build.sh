#!/usr/bin/env bash
# Build the sandbox guest: kernel image and root filesystem, into sandbox/out.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$here/.."
out="$here/out"; cache="$here/cache"
mkdir -p "$out" "$cache"

kernel_url="https://s3.amazonaws.com/spec.ccfc.min/firecracker-ci/v1.15/x86_64/vmlinux-6.1.155"
if [ ! -f "$out/vmlinux" ]; then
  curl -fsSL -o "$out/vmlinux.part" "$kernel_url"
  mv "$out/vmlinux.part" "$out/vmlinux"
fi
if [ -f "$here/vmlinux.sha256" ]; then
  (cd "$out" && sha256sum -c "$here/vmlinux.sha256")
else
  (cd "$out" && sha256sum vmlinux > "$here/vmlinux.sha256")
fi

cargo build --release -p magnus-guest --target x86_64-unknown-linux-musl --manifest-path "$root/Cargo.toml"
cp "$root/target/x86_64-unknown-linux-musl/release/magnus-guest" "$cache/magnus-guest"
cp "$here/Dockerfile" "$cache/Dockerfile"
cp "$here/sitecustomize.py" "$cache/sitecustomize.py"
cp "$here/check_tools.py" "$cache/check_tools.py"
DOCKER_BUILDKIT=1 docker build --target export --output "type=local,dest=$out" "$cache"
ls -la "$out"
