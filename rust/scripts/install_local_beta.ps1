param(
    [string]$PackageDir = "",
    [string]$InstallRoot = "",
    [string]$StartMenuRoot = "",
    [switch]$SkipBuild,
    [switch]$NoShortcut
)

$ErrorActionPreference = "Stop"
$RustRoot = Split-Path -Parent $PSScriptRoot
if (-not $PackageDir) { $PackageDir = Join-Path $RustRoot "local-dist\ApricotPlayer2Beta" }
if (-not $InstallRoot) { $InstallRoot = Join-Path $env:LOCALAPPDATA "Programs\ApricotPlayer2Beta" }
if (-not $StartMenuRoot) { $StartMenuRoot = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs" }

function Get-FullPath([string]$Path) {
    return [System.IO.Path]::GetFullPath($Path)
}

function Assert-ChildPath([string]$Path, [string]$Parent, [string]$Label) {
    $fullPath = Get-FullPath $Path
    $fullParent = (Get-FullPath $Parent).TrimEnd('\', '/') + [System.IO.Path]::DirectorySeparatorChar
    if (-not $fullPath.StartsWith($fullParent, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "$Label must stay inside $fullParent"
    }
    return $fullPath
}

function Test-Manifest([string]$Root) {
    $ManifestPath = Join-Path $Root "package-manifest.json"
    if (-not (Test-Path -LiteralPath $ManifestPath -PathType Leaf)) { throw "Local beta package manifest is missing" }
    $Manifest = Get-Content -LiteralPath $ManifestPath -Raw | ConvertFrom-Json
    if ($Manifest.schema_version -ne 1 -or $Manifest.application_id -ne "ApricotPlayer.RustBeta") {
        throw "Local beta package manifest has an unsupported identity or schema"
    }
    $Listed = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::OrdinalIgnoreCase)
    foreach ($File in @($Manifest.files)) {
        $Relative = [string]$File.path
        $PathParts = $Relative -split '[\\/]'
        if (-not $Relative -or [System.IO.Path]::IsPathRooted($Relative) -or $PathParts -contains '..') {
            throw "Unsafe package path in manifest"
        }
        $Full = Assert-ChildPath (Join-Path $Root $Relative) $Root "Package file"
        if (-not (Test-Path -LiteralPath $Full -PathType Leaf)) { throw "Package file is missing: $Relative" }
        $ActualHash = (Get-FileHash -LiteralPath $Full -Algorithm SHA256).Hash
        if ($ActualHash -ne [string]$File.sha256) { throw "Package hash mismatch: $Relative" }
        $Listed.Add($Relative.Replace('\', '/')) | Out-Null
    }
    $Actual = Get-ChildItem -LiteralPath $Root -File -Recurse | Where-Object { $_.Name -ne "package-manifest.json" }
    foreach ($File in $Actual) {
        if (($File.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw "Package contains a reparse point"
        }
        $Relative = $File.FullName.Substring((Get-FullPath $Root).Length).TrimStart('\', '/').Replace('\', '/')
        if (-not $Listed.Contains($Relative)) { throw "Package contains an unlisted file: $Relative" }
    }
    if ($Actual.Count -ne $Listed.Count) { throw "Package manifest file count does not match" }
    if (-not (Test-Path -LiteralPath (Join-Path $Root "ApricotPlayer2Beta.exe") -PathType Leaf)) {
        throw "Local beta executable is missing"
    }
}

if (-not $SkipBuild) {
    & (Join-Path $PSScriptRoot "build_local_beta.ps1") -OutputDir $PackageDir
    if ($LASTEXITCODE -ne 0) { throw "Local beta build failed with exit code $LASTEXITCODE" }
}
$PackageDir = (Resolve-Path -LiteralPath $PackageDir).Path
Test-Manifest $PackageDir

$InstallRoot = Get-FullPath $InstallRoot
if ((Split-Path -Leaf $InstallRoot) -ne "ApricotPlayer2Beta") {
    throw "Install root must end with ApricotPlayer2Beta"
}
$InstallParent = Split-Path -Parent $InstallRoot
$Staging = Assert-ChildPath (Join-Path $InstallParent (".ApricotPlayer2Beta.install-" + [Guid]::NewGuid().ToString("N"))) $InstallParent "Install staging"
$Backup = Assert-ChildPath (Join-Path $InstallParent (".ApricotPlayer2Beta.backup-" + [Guid]::NewGuid().ToString("N"))) $InstallParent "Install backup"

$Running = Get-Process -Name "ApricotPlayer2Beta" -ErrorAction SilentlyContinue
if ($Running) { throw "Close ApricotPlayer 2 Beta before installing a new local build" }

New-Item -ItemType Directory -Path $InstallParent -Force | Out-Null
try {
    Copy-Item -LiteralPath $PackageDir -Destination $Staging -Recurse
    Test-Manifest $Staging
    if (Test-Path -LiteralPath $InstallRoot) {
        Move-Item -LiteralPath $InstallRoot -Destination $Backup
    }
    try {
        Move-Item -LiteralPath $Staging -Destination $InstallRoot
    }
    catch {
        if ((Test-Path -LiteralPath $Backup) -and -not (Test-Path -LiteralPath $InstallRoot)) {
            Move-Item -LiteralPath $Backup -Destination $InstallRoot
        }
        throw
    }
    if (-not $NoShortcut) {
        New-Item -ItemType Directory -Path $StartMenuRoot -Force | Out-Null
        $ShortcutPath = Join-Path $StartMenuRoot "ApricotPlayer 2 Beta.lnk"
        $Shell = New-Object -ComObject WScript.Shell
        $Shortcut = $Shell.CreateShortcut($ShortcutPath)
        $Shortcut.TargetPath = Join-Path $InstallRoot "ApricotPlayer2Beta.exe"
        $Shortcut.WorkingDirectory = $InstallRoot
        $Shortcut.Description = "ApricotPlayer 2 Beta local Rust build"
        $Shortcut.Save()
    }
    if (Test-Path -LiteralPath $Backup) {
        Remove-Item -LiteralPath $Backup -Recurse -Force
    }
    Write-Output "LOCAL_BETA_INSTALL=$InstallRoot"
}
finally {
    if (Test-Path -LiteralPath $Staging) {
        Remove-Item -LiteralPath $Staging -Recurse -Force -ErrorAction SilentlyContinue
    }
}
