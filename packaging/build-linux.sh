#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
if [[ $(uname -s) != Linux ]]; then
    echo "The Linux packages must be built on Linux." >&2
    exit 1
fi
command -v cargo >/dev/null || { echo "Missing required tool: cargo" >&2; exit 1; }
TOOLS="$ROOT/target/tools"
mkdir -p "$TOOLS"
export PATH="$TOOLS/bin:$PATH"

cd "$ROOT"
VERSION=$(awk -F'"' '/^version = / { print $2; exit }' Cargo.toml)
cargo build --release
if ! command -v cargo-deb >/dev/null; then
    cargo install cargo-deb --root "$TOOLS"
fi
if ! command -v cargo-generate-rpm >/dev/null; then
    cargo install cargo-generate-rpm --root "$TOOLS"
fi

OUT="$ROOT/target/package/linux"
rm -rf "$OUT"
mkdir -p "$OUT"
cargo deb --no-build
cargo generate-rpm
cp target/debian/*.deb "$OUT/"
cp target/generate-rpm/*.rpm "$OUT/"
tar -czf "$OUT/SozoIDE-$VERSION-$(uname -m)-linux.tar.gz" -C target/release sozo
echo "Created DEB, RPM and tar.gz packages in $OUT"
