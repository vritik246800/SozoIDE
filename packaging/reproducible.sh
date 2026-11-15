#!/usr/bin/env bash
# Two builds of the static Linux binary from two copies of the source in different folders
# must be the same file (TODO 6.4.11): what is downloaded is what the source makes. The paths
# of the build machine are mapped away; the time is fixed by SOURCE_DATE_EPOCH.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET="$(uname -m)-unknown-linux-musl"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
export SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-$(git -C "$ROOT" log -1 --format=%ct)}"
for copy in a b; do
    mkdir -p "$WORK/$copy"
    git -C "$ROOT" archive HEAD | tar -x -C "$WORK/$copy"
    (
        cd "$WORK/$copy"
        RUSTFLAGS="--remap-path-prefix=$WORK/$copy=/build --remap-path-prefix=$HOME/.cargo=/cargo" \
            cargo build --release --locked --target "$TARGET" --quiet
    )
done
a=$(sha256sum "$WORK/a/target/$TARGET/release/sozo" | cut -d' ' -f1)
b=$(sha256sum "$WORK/b/target/$TARGET/release/sozo" | cut -d' ' -f1)
echo "a: $a"
echo "b: $b"
[ "$a" = "$b" ] || { echo "The two builds differ" >&2; exit 1; }
echo "Reproducible: both builds are $a"
