# Installs the MSI built by build-windows.ps1 the way a user would (for this user, no
# administrator) and checks that a new terminal finds `sozo` on the PATH.
$ErrorActionPreference = "Stop"
$Root = Resolve-Path (Join-Path $PSScriptRoot "..")
$Version = (Select-String -Path (Join-Path $Root "Cargo.toml") -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$Msi = Get-Item (Join-Path $Root "target/wix/SozoIDE-windows-*.msi")
$Log = Join-Path $Root "target/wix/install.log"

$Process = Start-Process msiexec.exe -ArgumentList "/i `"$($Msi.FullName)`" /qn /l*v `"$Log`"" -Wait -PassThru
if ($Process.ExitCode -ne 0) {
    Get-Content $Log -Tail 40
    throw "msiexec failed ($($Process.ExitCode))"
}

# A new terminal reads the user PATH from the registry, where the installer wrote it.
$Bin = Join-Path $env:LOCALAPPDATA "SozoIDE\bin"
$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
if (($UserPath -split ";" | ForEach-Object { $_.TrimEnd("\") }) -notcontains $Bin) {
    throw "$Bin is not on the user PATH: $UserPath"
}
$env:PATH = "$UserPath;$env:PATH"
$Output = & sozo --version
if ($Output -ne "sozo $Version") { throw "sozo --version printed '$Output'" }
# A data file read from the build machine's cargo registry exists only on the runner (ERRORS.md E10).
$Text = [Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes((Get-Command sozo).Source))
$Found = [regex]::Match($Text, '\.cargo[\\/]registry[\\/]src[\\/][^\x00 ]*\.(yaml|yml|toml|json|txt|dat|bin)')
if ($Found.Success) { throw "sozo reads data files from the build machine: $($Found.Value)" }
Write-Host "OK: $Output ($((Get-Command sozo).Source))"
