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
    $BundledNvda = Join-Path $PackageDir "nvda\nvdaControllerClient64.dll"
    if (-not (Test-Path -LiteralPath $BundledNvda -PathType Leaf)) {
        throw "Local beta package omitted the NVDA Controller Client"
    }
    $BundledMpv = Join-Path $PackageDir "mpv\mpv.exe"
    $BundledLibMpv = Join-Path $PackageDir "mpv\libmpv-2.dll"
    $BundledMpvD3dCompiler = Join-Path $PackageDir "mpv\d3dcompiler_43.dll"
    $BundledYoutubeHelper = Join-Path $PackageDir "components\apricot-youtube-helper.exe"
    if (-not (Test-Path -LiteralPath $BundledMpv -PathType Leaf)) {
        throw "Local beta package omitted mpv.exe"
    }
    if (-not (Test-Path -LiteralPath $BundledLibMpv -PathType Leaf)) {
        throw "Local beta package omitted libmpv-2.dll"
    }
    if (-not (Test-Path -LiteralPath $BundledMpvD3dCompiler -PathType Leaf)) {
        throw "Local beta package omitted the mpv D3D compiler"
    }
    if (-not (Test-Path -LiteralPath $BundledYoutubeHelper -PathType Leaf)) {
        throw "Local beta package omitted the Rust YouTube helper"
    }
    $BuildInfo = Get-Content -LiteralPath (Join-Path $PackageDir "build-info.json") -Raw | ConvertFrom-Json
    if ($BuildInfo.bundled_components.nvda_controller_client -ne "nvda/nvdaControllerClient64.dll") {
        throw "Local beta build metadata omitted the NVDA Controller Client"
    }
    if ($BuildInfo.bundled_components.mpv -ne "mpv/mpv.exe") {
        throw "Local beta build metadata omitted mpv"
    }
    if ($BuildInfo.bundled_components.libmpv -ne "mpv/libmpv-2.dll") {
        throw "Local beta build metadata omitted libmpv"
    }
    if ($BuildInfo.bundled_components.mpv_d3d_compiler -ne "mpv/d3dcompiler_43.dll") {
        throw "Local beta build metadata omitted the mpv D3D compiler"
    }
    if ($BuildInfo.bundled_components.rusty_ytdl_helper -ne "components/apricot-youtube-helper.exe") {
        throw "Local beta build metadata omitted the Rust YouTube helper"
    }
    if ($BuildInfo.bundled_components.rusty_ytdl_revision -ne "b1c6eb7c83f0d6189f256ed5df50019a5803c734") {
        throw "Local beta build metadata has the wrong Rust YouTube backend revision"
    }
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
    if (-not (Test-Path -LiteralPath (Join-Path $InstallRoot "nvda\nvdaControllerClient64.dll") -PathType Leaf)) {
        throw "Installed local beta omitted the NVDA Controller Client"
    }
    if (-not (Test-Path -LiteralPath (Join-Path $InstallRoot "mpv\mpv.exe") -PathType Leaf)) {
        throw "Installed local beta omitted mpv.exe"
    }
    if (-not (Test-Path -LiteralPath (Join-Path $InstallRoot "mpv\libmpv-2.dll") -PathType Leaf)) {
        throw "Installed local beta omitted libmpv-2.dll"
    }
    if (-not (Test-Path -LiteralPath (Join-Path $InstallRoot "mpv\d3dcompiler_43.dll") -PathType Leaf)) {
        throw "Installed local beta omitted the mpv D3D compiler"
    }
    $InstalledYoutubeHelper = Join-Path $InstallRoot "components\apricot-youtube-helper.exe"
    if (-not (Test-Path -LiteralPath $InstalledYoutubeHelper -PathType Leaf)) {
        throw "Installed local beta omitted the Rust YouTube helper"
    }
    $HelloRequest = '{"protocol_version":1,"request_id":1,"command":{"type":"hello"}}'
    $ShutdownRequest = '{"protocol_version":1,"request_id":2,"command":{"type":"shutdown"}}'
    $HelperResponses = @($HelloRequest, $ShutdownRequest) | & $InstalledYoutubeHelper
    if ($LASTEXITCODE -ne 0) { throw "Installed Rust YouTube helper did not run" }
    $HelloResponse = $HelperResponses[0] | ConvertFrom-Json
    if ($HelloResponse.status -ne "hello" -or $HelloResponse.request_id -ne 1) {
        throw "Installed Rust YouTube helper failed its protocol handshake"
    }
    & $Executable --qualification-smoke
    if ($LASTEXITCODE -ne 0) { throw "Installed local beta did not launch correctly" }

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
