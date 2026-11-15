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
# Only DLLs every Windows has: no VCRUNTIME140.dll or other redistributable (CLAUDE.md §0.1).
$VsWhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
$DumpBin = & $VsWhere -latest -products * -find "VC\Tools\MSVC\**\dumpbin.exe" | Select-Object -First 1
if (-not $DumpBin) { throw "dumpbin.exe not found" }
$Dlls = & $DumpBin /nologo /dependents (Get-Command sozo).Source |
    Where-Object { $_ -match '^\s+(\S+\.dll)\s*$' } | ForEach-Object { $Matches[1] }
$Missing = $Dlls | Where-Object {
    $_ -notmatch '^(api|ext)-ms-win-' -and -not (Test-Path (Join-Path $env:SystemRoot "System32\$_"))
}
$Redistributable = $Dlls | Where-Object { $_ -match '^(vcruntime|msvcp)\d+' }
if ($Missing -or $Redistributable) { throw "sozo links DLLs a user may not have: $($Missing + $Redistributable)" }
Write-Host "OK: $Output ($((Get-Command sozo).Source))"
