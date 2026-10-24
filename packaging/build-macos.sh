#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
if [[ $(uname -s) != Darwin ]]; then
    echo "The macOS package must be built on macOS." >&2
    exit 1
fi
for tool in cargo sips iconutil pkgbuild; do
    command -v "$tool" >/dev/null || { echo "Missing required tool: $tool" >&2; exit 1; }
done

cd "$ROOT"
VERSION=$(awk -F'"' '/^version = / { print $2; exit }' Cargo.toml)
# Built natively: Apple silicon only (the release runner, macos-latest, is arm64).
ARCH=$(uname -m)
OUT="$ROOT/target/package/macos"
APP="$OUT/SozoIDE.app"
STAGING="$OUT/pkg-root"
rm -rf "$OUT"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" \
    "$STAGING/Applications" "$STAGING/usr/local/bin"

cargo build --release
cp target/release/sozo "$APP/Contents/MacOS/sozo"
cp packaging/macos/launcher.sh "$APP/Contents/MacOS/SozoIDE"
chmod +x "$APP/Contents/MacOS/SozoIDE" "$APP/Contents/MacOS/sozo"
sed -e "s/SOZO_VERSION/$VERSION/g" packaging/macos/Info.plist > "$APP/Contents/Info.plist"

ICONSET="$OUT/SozoIDE.iconset"
mkdir -p "$ICONSET"
for size in 16 32 128 256 512; do
    double=$((size * 2))
    sips -z "$size" "$size" assets/Sozo.png --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
    sips -z "$double" "$double" assets/Sozo.png --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/SozoIDE.icns"

cp -R "$APP" "$STAGING/Applications/"
cp target/release/sozo "$STAGING/usr/local/bin/sozo"
chmod +x "$STAGING/usr/local/bin/sozo"
pkgbuild --root "$STAGING" \
    --identifier com.vritikvalabdas.sozoide \
    --version "$VERSION" \
    --install-location / \
    "$OUT/SozoIDE-$VERSION-$ARCH.pkg"
echo "Created $OUT/SozoIDE-$VERSION-$ARCH.pkg"
