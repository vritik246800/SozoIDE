$ErrorActionPreference = "Stop"
$Root = Resolve-Path (Join-Path $PSScriptRoot "..")
$Tools = Join-Path $Root "target/tools"
$Wix = Join-Path $Tools "wix314"
$CargoWix = Join-Path $Tools "bin/cargo-wix.exe"

if (-not (Test-Path $CargoWix)) {
    cargo install cargo-wix --root $Tools
}

$WixBin = Join-Path $Wix "bin"
if (-not (Test-Path (Join-Path $WixBin "candle.exe"))) {
    New-Item -ItemType Directory -Force -Path $WixBin | Out-Null
    # WiX 3.14 is the first 3.x release that builds ARM64 installers.
    $Archive = Join-Path $Tools "wix314-binaries.zip"
    if (-not (Test-Path $Archive)) {
        Invoke-WebRequest `
            -Uri "https://github.com/wixtoolset/wix3/releases/download/wix3141rtm/wix314-binaries.zip" `
            -OutFile $Archive
    }
    Expand-Archive -Path $Archive -DestinationPath $WixBin -Force
}

$env:PATH = "$(Join-Path $Tools 'bin');$WixBin;$env:PATH"
$env:WIX = "$Wix\"
Set-Location $Root
# No version in the name: the site and the updater download releases/latest/download/<name>.
$Arch = if ($env:PROCESSOR_ARCHITECTURE -eq "ARM64") { "arm64" } else { "x86_64" }
$Msi = Join-Path $Root "target/wix/SozoIDE-windows-$Arch.msi"
cargo wix --nocapture --output $Msi
Write-Host "Installer created: $Msi"
