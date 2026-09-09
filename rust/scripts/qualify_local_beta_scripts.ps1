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
    $BundledYtDlp = Join-Path $PackageDir "components\yt-dlp.exe"
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
    if (-not (Test-Path -LiteralPath $BundledYtDlp -PathType Leaf)) {
        throw "Local beta package omitted standalone yt-dlp"
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
    if ($BuildInfo.bundled_components.yt_dlp -ne "components/yt-dlp.exe" -or
        $BuildInfo.bundled_components.yt_dlp_version -ne "2026.08.19") {
        throw "Local beta build metadata omitted the standalone yt-dlp component"
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
    $InstalledYtDlp = Join-Path $InstallRoot "components\yt-dlp.exe"
    if (-not (Test-Path -LiteralPath $InstalledYoutubeHelper -PathType Leaf)) {
        throw "Installed local beta omitted the Rust YouTube helper"
    }
    if (-not (Test-Path -LiteralPath $InstalledYtDlp -PathType Leaf)) {
        throw "Installed local beta omitted standalone yt-dlp"
    }
    $InstalledYtDlpOutput = @(& $InstalledYtDlp --version)
    $InstalledYtDlpExitCode = $LASTEXITCODE
    $InstalledYtDlpVersion = ($InstalledYtDlpOutput | Select-Object -First 1).Trim()
    if ($InstalledYtDlpExitCode -ne 0 -or $InstalledYtDlpVersion -ne "2026.08.19") {
        throw "Installed standalone yt-dlp failed its version check (exit=$InstalledYtDlpExitCode, version='$InstalledYtDlpVersion')"
    }
    $YoutubeProtocolVersion = 4
    $HelloRequest = [ordered]@{
        protocol_version = $YoutubeProtocolVersion
        request_id = 1
        command = [ordered]@{ type = "hello" }
    } | ConvertTo-Json -Compress
    $ShutdownRequest = [ordered]@{
        protocol_version = $YoutubeProtocolVersion
        request_id = 2
        command = [ordered]@{ type = "shutdown" }
    } | ConvertTo-Json -Compress
    $HelperResponses = @($HelloRequest, $ShutdownRequest) | & $InstalledYoutubeHelper
    if ($LASTEXITCODE -ne 0) { throw "Installed Rust YouTube helper did not run" }
    $HelloResponse = $HelperResponses[0] | ConvertFrom-Json
    if ($HelloResponse.status -ne "hello" -or
        $HelloResponse.request_id -ne 1 -or
        "playlist_collections" -notin @($HelloResponse.capabilities)) {
        throw "Installed Rust YouTube helper failed its protocol handshake"
    }
    $FoundationSmoke = Start-Process -FilePath $Executable -ArgumentList "--qualification-smoke" -WindowStyle Hidden -Wait -PassThru
    if ($FoundationSmoke.ExitCode -ne 0) { throw "Installed local beta did not launch correctly" }
    $ProductionHelperCheck = Start-Process -FilePath $Executable -ArgumentList "--qualification-youtube-helper" -WindowStyle Hidden -Wait -PassThru
    if ($ProductionHelperCheck.ExitCode -ne 0) {
        throw "Installed app could not use the Rust YouTube helper through its production process client"
    }
    $ProductionYtDlpCheck = Start-Process -FilePath $Executable -ArgumentList "--qualification-ytdlp" -WindowStyle Hidden -Wait -PassThru
    if ($ProductionYtDlpCheck.ExitCode -ne 0) {
        throw "Installed app could not use standalone yt-dlp through its production adapter"
    }
    $ProductionPlaybackCheck = Start-Process -FilePath $Executable -ArgumentList "--qualification-playback" -WindowStyle Hidden -Wait -PassThru
    if ($ProductionPlaybackCheck.ExitCode -ne 0) {
        throw "Installed app could not play media through its packaged libmpv runtime"
    }
    $ProcessExitDeadline = [DateTime]::UtcNow.AddSeconds(2)
    do {
        $QualificationProcesses = @(Get-CimInstance Win32_Process -Filter "Name = 'ApricotPlayer2Beta.exe'" | Where-Object {
            $_.ExecutablePath -eq $Executable
        })
        if ($QualificationProcesses.Count -eq 0) { break }
        Start-Sleep -Milliseconds 25
    } while ([DateTime]::UtcNow -lt $ProcessExitDeadline)
    if ($QualificationProcesses.Count -ne 0) {
        throw "Qualification app process did not exit after the YouTube helper smoke test"
    }

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
