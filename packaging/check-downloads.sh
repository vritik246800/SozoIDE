#!/usr/bin/env bash
# After a release: every download link of the site must deliver the files of that version.
# Usage: bash packaging/check-downloads.sh [VERSION]   (defaults to the version in Cargo.toml)
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
VERSION=${1:-$(awk -F'"' '/^version = / { print $2; exit }' "$ROOT/Cargo.toml")}
REPO=https://github.com/vritik246800/SozoIDE
FILES=(macos-arm64.dmg windows-x86_64.msi windows-arm64.msi
    linux-x86_64.deb linux-arm64.deb linux-x86_64.rpm linux-arm64.rpm
    linux-x86_64.tar.gz linux-arm64.tar.gz)

# The site must link exactly these files, no more and no less.
site=$(grep -oE "download\('[a-z0-9_.-]+'\)" "$ROOT/site/src/pages/index.astro" \
    | sed -E "s/download\('(.*)'\)/\1/" | grep -v '^$' | sort -u)
expected=$(printf '%s\n' "${FILES[@]}" | sort -u)
if [[ $site != "$expected" ]]; then
    echo "site/src/pages/index.astro links other files than the release:" >&2
    diff <(echo "$expected") <(echo "$site") >&2 || true
    exit 1
fi

failed=0
for file in "${FILES[@]}"; do
    url="$REPO/releases/latest/download/SozoIDE-$file"
    target=$(curl -s -o /dev/null -w '%{redirect_url}' "$url")
    # HEAD, not GET: GitHub counts every GET as a download, and Info_SozoIDE reads those counts.
    status=$(curl -sIL -o /dev/null -w '%{http_code}' "$url")
    if [[ $target == "$REPO/releases/download/v$VERSION/SozoIDE-$file" && $status == 200 ]]; then
        echo "OK  SozoIDE-$file -> v$VERSION"
    else
        echo "BAD SozoIDE-$file -> ${target:-no redirect} ($status)" >&2
        failed=1
    fi
done
# The files SozoIDE downloads once to count an installation or an update (F20).
for target in macos-arm64 windows-x86_64 windows-arm64 linux-x86_64 linux-arm64; do
    for kind in installed updated; do
        url="$REPO/releases/download/v$VERSION/SozoIDE-$target.$kind.txt"
        if [[ $(curl -sIL -o /dev/null -w '%{http_code}' "$url") == 200 ]]; then
            echo "OK  SozoIDE-$target.$kind.txt"
        else
            echo "BAD SozoIDE-$target.$kind.txt missing from v$VERSION" >&2
            failed=1
        fi
    done
done
exit "$failed"
