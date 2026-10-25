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
# No version in the names: the site and the updater download releases/latest/download/<name>.
ARCH=$(uname -m)
[[ $ARCH == aarch64 ]] && ARCH=arm64
cp target/debian/sozoide_"$VERSION"-1_*.deb "$OUT/SozoIDE-linux-$ARCH.deb"
cp target/generate-rpm/sozoide-"$VERSION"-1.*.rpm "$OUT/SozoIDE-linux-$ARCH.rpm"
tar -czf "$OUT/SozoIDE-linux-$ARCH.tar.gz" -C target/release sozo
echo "Created DEB, RPM and tar.gz packages in $OUT"
