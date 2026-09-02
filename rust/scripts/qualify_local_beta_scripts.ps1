$ErrorActionPreference = "Stop"
$RustRoot = Split-Path -Parent $PSScriptRoot
$Token = [Guid]::NewGuid().ToString("N")
$PackageDir = Join-Path $RustRoot "local-dist\qualification-$Token\ApricotPlayer2Beta"
$TemporaryRoot = Join-Path ([System.IO.Path]::GetTempPath()) "apricot-rust-local-beta-$Token"
$InstallRoot = Join-Path $TemporaryRoot "Programs\ApricotPlayer2Beta"
$AppDataPath = Join-Path $TemporaryRoot "Roaming\ApricotPlayer2Beta"
$Sentinel = Join-Path $AppDataPath "preserve-me.txt"
$TamperedPackage = Join-Path $TemporaryRoot "tampered-package"
$TamperedInstall = Join-Path $TemporaryRoot "TamperedPrograms\ApricotPlayer2Beta"

try {
    New-Item -ItemType Directory -Path $AppDataPath -Force | Out-Null
    Set-Content -LiteralPath $Sentinel -Value "private beta data" -Encoding utf8

    & (Join-Path $PSScriptRoot "build_local_beta.ps1") -OutputDir $PackageDir -SkipChecks
    Copy-Item -LiteralPath $PackageDir -Destination $TamperedPackage -Recurse
    Add-Content -LiteralPath (Join-Path $TamperedPackage "build-info.json") -Value "tampered"
    $TamperRejected = $false
    try {
        & (Join-Path $PSScriptRoot "install_local_beta.ps1") -PackageDir $TamperedPackage -InstallRoot $TamperedInstall -SkipBuild -NoShortcut
    }
    catch {
        $TamperRejected = $true
    }
    if (-not $TamperRejected -or (Test-Path -LiteralPath $TamperedInstall)) {
        throw "Tampered local beta package was not rejected before installation"
    }

    & (Join-Path $PSScriptRoot "install_local_beta.ps1") -PackageDir $PackageDir -InstallRoot $InstallRoot -SkipBuild -NoShortcut
    $Executable = Join-Path $InstallRoot "ApricotPlayer2Beta.exe"
    if (-not (Test-Path -LiteralPath $Executable -PathType Leaf)) { throw "Installed executable is missing" }
    $Output = (& $Executable | Out-String)
    if ($LASTEXITCODE -ne 0 -or $Output -notmatch "foundation") { throw "Installed local beta did not launch correctly" }

    Set-Content -LiteralPath (Join-Path $InstallRoot "stale-file.txt") -Value "stale" -Encoding utf8
    & (Join-Path $PSScriptRoot "install_local_beta.ps1") -PackageDir $PackageDir -InstallRoot $InstallRoot -SkipBuild -NoShortcut
    if (Test-Path -LiteralPath (Join-Path $InstallRoot "stale-file.txt")) { throw "Atomic reinstall retained a stale file" }
    if (-not (Test-Path -LiteralPath $Sentinel -PathType Leaf)) { throw "Reinstall changed beta user data" }

    & (Join-Path $PSScriptRoot "uninstall_local_beta.ps1") -InstallRoot $InstallRoot -AppDataPath $AppDataPath -NoShortcut
    if (Test-Path -LiteralPath $InstallRoot) { throw "Uninstall retained the installation" }
    if (-not (Test-Path -LiteralPath $Sentinel -PathType Leaf)) { throw "Default uninstall removed beta user data" }

    & (Join-Path $PSScriptRoot "uninstall_local_beta.ps1") -InstallRoot $InstallRoot -AppDataPath $AppDataPath -NoShortcut -RemoveData
    if (Test-Path -LiteralPath $AppDataPath) { throw "Explicit data removal retained beta user data" }
    Write-Output "LOCAL_BETA_SCRIPTS=PASS"
}
finally {
    Remove-Item -LiteralPath $TemporaryRoot -Recurse -Force -ErrorAction SilentlyContinue
    $QualificationRoot = Split-Path -Parent $PackageDir
    Remove-Item -LiteralPath $QualificationRoot -Recurse -Force -ErrorAction SilentlyContinue
}
