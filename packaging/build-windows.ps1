$ErrorActionPreference = "Stop"
$Root = Resolve-Path (Join-Path $PSScriptRoot "..")
$Tools = Join-Path $Root "target/tools"
$Wix = Join-Path $Tools "wix"
$CargoWix = Join-Path $Tools "bin/cargo-wix.exe"

if (-not (Test-Path $CargoWix)) {
    cargo install cargo-wix --root $Tools
}

$WixBin = Join-Path $Wix "bin"
if (-not (Test-Path (Join-Path $WixBin "candle.exe"))) {
    New-Item -ItemType Directory -Force -Path $WixBin | Out-Null
    $Archive = Join-Path $Tools "wix311-binaries.zip"
    if (-not (Test-Path $Archive)) {
        Invoke-WebRequest `
            -Uri "https://github.com/wixtoolset/wix3/releases/download/wix3112rtm/wix311-binaries.zip" `
            -OutFile $Archive
    }
    Expand-Archive -Path $Archive -DestinationPath $WixBin -Force
}

$env:PATH = "$(Join-Path $Tools 'bin');$WixBin;$env:PATH"
$env:WIX = "$Wix\"
Set-Location $Root
cargo wix
Write-Host "Installer created under $Root\target\wix"
