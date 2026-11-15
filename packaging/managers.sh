#!/usr/bin/env bash
# The manifests of the package managers for a release (TODO 6.4.10, Q08), written next to the
# packages from their SHA256SUMS.txt: Scoop (sozoide.json, installable by its URL), winget (the
# three files a pull request to microsoft/winget-pkgs takes), the AUR (PKGBUILD of sozoide-bin)
# and Homebrew (a cask for a tap). Publishing them in those places needs the owner's accounts:
# PACKAGING.md §8 says how.
#
#   bash packaging/managers.sh VERSION FOLDER     (FOLDER has the packages and SHA256SUMS.txt)
set -euo pipefail
VERSION=${1:?version}
DIR=${2:?folder}
[[ $VERSION =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$ ]] || { echo "Not a version: $VERSION" >&2; exit 2; }
REPO="https://github.com/vritik246800/SozoIDE"
URL="$REPO/releases/download/v$VERSION"
sum() {
    local hash
    hash=$(awk -v name="$1" '$2 == name || $2 == "*" name { print $1 }' "$DIR/SHA256SUMS.txt")
    [[ $hash =~ ^[0-9a-f]{64}$ ]] || { echo "No checksum for $1" >&2; exit 1; }
    echo "$hash"
}
OUT="$DIR/managers"
mkdir -p "$OUT/winget"

cat > "$OUT/sozoide.json" <<EOF
{
  "version": "$VERSION",
  "description": "A terminal IDE: explorer, editor, terminals, Git and SQL in one window",
  "homepage": "$REPO",
  "license": "Apache-2.0",
  "architecture": {
    "64bit": { "url": "$URL/SozoIDE-windows-x86_64.msi", "hash": "$(sum SozoIDE-windows-x86_64.msi)" },
    "arm64": { "url": "$URL/SozoIDE-windows-arm64.msi", "hash": "$(sum SozoIDE-windows-arm64.msi)" }
  },
  "extract_dir": "SozoIDE",
  "bin": "bin\\\\sozo.exe",
  "checkver": { "github": "$REPO" }
}
EOF

ID="Vritik246800.SozoIDE"
cat > "$OUT/winget/$ID.yaml" <<EOF
PackageIdentifier: $ID
PackageVersion: $VERSION
DefaultLocale: en-US
ManifestType: version
ManifestVersion: 1.6.0
EOF
cat > "$OUT/winget/$ID.locale.en-US.yaml" <<EOF
PackageIdentifier: $ID
PackageVersion: $VERSION
PackageLocale: en-US
Publisher: vritik246800
PackageName: SozoIDE
License: Apache-2.0
ShortDescription: A terminal IDE with an explorer, an editor, terminals, Git and SQL
PackageUrl: $REPO
ManifestType: defaultLocale
ManifestVersion: 1.6.0
EOF
cat > "$OUT/winget/$ID.installer.yaml" <<EOF
PackageIdentifier: $ID
PackageVersion: $VERSION
InstallerType: wix
Scope: user
Installers:
  - Architecture: x64
    InstallerUrl: $URL/SozoIDE-windows-x86_64.msi
    InstallerSha256: $(sum SozoIDE-windows-x86_64.msi | tr a-f A-F)
  - Architecture: arm64
    InstallerUrl: $URL/SozoIDE-windows-arm64.msi
    InstallerSha256: $(sum SozoIDE-windows-arm64.msi | tr a-f A-F)
ManifestType: installer
ManifestVersion: 1.6.0
EOF

PKGVER=${VERSION//-/_}
cat > "$OUT/PKGBUILD" <<EOF
# Maintainer: vritik246800
pkgname=sozoide-bin
pkgver=$PKGVER
pkgrel=1
pkgdesc="A terminal IDE: explorer, editor, terminals, Git and SQL in one window"
arch=('x86_64' 'aarch64')
url="$REPO"
license=('Apache-2.0')
optdepends=('git: Source Control' 'curl: updates' 'ffmpeg: videos' 'tmux: shells that keep running')
provides=('sozo')
source_x86_64=("SozoIDE-linux-x86_64-$VERSION.tar.gz::$URL/SozoIDE-linux-x86_64.tar.gz")
source_aarch64=("SozoIDE-linux-arm64-$VERSION.tar.gz::$URL/SozoIDE-linux-arm64.tar.gz")
sha256sums_x86_64=('$(sum SozoIDE-linux-x86_64.tar.gz)')
sha256sums_aarch64=('$(sum SozoIDE-linux-arm64.tar.gz)')

package() {
  install -Dm755 sozo "\$pkgdir/usr/bin/sozo"
  [[ -f sozo.1.gz ]] && install -Dm644 sozo.1.gz "\$pkgdir/usr/share/man/man1/sozo.1.gz"
}
EOF

cat > "$OUT/sozoide.rb" <<EOF
cask "sozoide" do
  version "$VERSION"
  sha256 "$(sum SozoIDE-macos-arm64.dmg)"

  url "$URL/SozoIDE-macos-arm64.dmg"
  name "SozoIDE"
  desc "Terminal IDE: explorer, editor, terminals, Git and SQL in one window"
  homepage "$REPO"

  depends_on arch: :arm64

  app "SozoIDE.app"
  binary "#{appdir}/SozoIDE.app/Contents/MacOS/sozo"
end
EOF
echo "Manifests in $OUT"
