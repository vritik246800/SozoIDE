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
cargo build --release --locked
# Completions and the manual page come from the binary itself, always in step with it (F10).
EXTRA="$ROOT/target/completions"
mkdir -p "$EXTRA"
target/release/sozo --completions bash > "$EXTRA/sozo.bash"
target/release/sozo --completions zsh > "$EXTRA/_sozo"
target/release/sozo --completions fish > "$EXTRA/sozo.fish"
target/release/sozo --man | gzip -9n > "$EXTRA/sozo.1.gz"
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
# The .tar.gz is static (musl): it runs on any Linux, whatever its C library (TODO 6.4.11).
# It needs musl-gcc (the musl-tools package) and rustup's musl target; the release workflow
# installs both.
MUSL="$(uname -m)-unknown-linux-musl"
if command -v musl-gcc >/dev/null && rustup target add "$MUSL" >/dev/null 2>&1; then
    cargo build --release --locked --target "$MUSL"
    TARBIN="target/$MUSL/release"
else
    echo "musl-gcc or the $MUSL target is missing: the .tar.gz is linked to this system's glibc" >&2
    TARBIN="target/release"
fi
tar -czf "$OUT/SozoIDE-linux-$ARCH.tar.gz" -C "$TARBIN" sozo -C "$EXTRA" sozo.bash _sozo sozo.fish sozo.1.gz
# The size of the binary, for the release's summary (TODO 6.4.11).
echo "sozo (static) $(wc -c < "$TARBIN/sozo") bytes" > "$ROOT/target/package/size-linux.txt"
echo "Created DEB, RPM and tar.gz packages in $OUT"
