#!/usr/bin/env bash
# Installs the package built by build-macos.sh or build-linux.sh the way a user would and checks
# that the `sozo` command answers with the version of Cargo.toml.
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
VERSION=$(awk -F'"' '/^version = / { print $2; exit }' "$ROOT/Cargo.toml")
WORK=$(mktemp -d)

check() {
    local output
    output=$("$1" --version)
    [[ $output == "sozo $VERSION" ]] || { echo "$1 --version printed '$output'" >&2; exit 1; }
    echo "OK: $output ($1)"
}

case $(uname -s) in
Darwin)
    # What the user does: open the DMG, drag the app, and the app links /usr/local/bin/sozo.
    mkdir -p "$WORK/mount" "$WORK/Applications" "$WORK/bin"
    hdiutil attach -nobrowse -readonly -quiet -mountpoint "$WORK/mount" \
        "$ROOT"/target/package/macos/SozoIDE-macos-*.dmg
    trap 'hdiutil detach -quiet "$WORK/mount"' EXIT
    [[ -L $WORK/mount/Applications ]] || { echo "The DMG has no Applications link" >&2; exit 1; }
    ditto "$WORK/mount/SozoIDE.app" "$WORK/Applications/SozoIDE.app"
    codesign --verify --deep --strict "$WORK/Applications/SozoIDE.app"
    ln -s "$WORK/Applications/SozoIDE.app/Contents/MacOS/sozo" "$WORK/bin/sozo"
    check "$WORK/bin/sozo"
    ;;
Linux)
    sudo apt-get install -y "$ROOT"/target/package/linux/SozoIDE-linux-*.deb
    [[ $(command -v sozo) == /usr/bin/sozo ]] || { echo "sozo is not /usr/bin/sozo" >&2; exit 1; }
    check sozo
    tar -xzf "$ROOT"/target/package/linux/SozoIDE-linux-*.tar.gz -C "$WORK"
    check "$WORK/sozo"
    ;;
*)
    echo "Use packaging/test-install.ps1 on Windows." >&2
    exit 1
    ;;
esac
