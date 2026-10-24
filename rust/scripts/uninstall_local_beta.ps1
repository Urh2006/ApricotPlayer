param(
    [string]$InstallRoot = "",
    [string]$AppDataPath = "",
    [string]$StartMenuRoot = "",
    [switch]$RemoveData,
    [switch]$NoShortcut
)

$ErrorActionPreference = "Stop"
if (-not $InstallRoot) { $InstallRoot = Join-Path $env:LOCALAPPDATA "Programs\ApricotPlayer2Beta" }
if (-not $AppDataPath) { $AppDataPath = Join-Path $env:APPDATA "ApricotPlayer2Beta" }
if (-not $StartMenuRoot) { $StartMenuRoot = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs" }

function Get-SafeBetaPath([string]$Path, [string]$Label) {
    $full = [System.IO.Path]::GetFullPath($Path)
    if ((Split-Path -Leaf $full) -ne "ApricotPlayer2Beta") {
        throw "$Label must end with ApricotPlayer2Beta"
    }
    $parent = Split-Path -Parent $full
    if (-not $parent -or $full -eq [System.IO.Path]::GetPathRoot($full)) {
        throw "$Label is not safe to remove"
    }
    return $full
}

$InstallRoot = Get-SafeBetaPath $InstallRoot "Install root"
$AppDataPath = Get-SafeBetaPath $AppDataPath "App-data path"
$Running = Get-Process -Name "ApricotPlayer2Beta" -ErrorAction SilentlyContinue
if ($Running) { throw "Close ApricotPlayer 2 Beta before uninstalling it" }

if (-not $NoShortcut) {
    $ShortcutPath = Join-Path $StartMenuRoot "ApricotPlayer 2 Beta.lnk"
    Remove-Item -LiteralPath $ShortcutPath -Force -ErrorAction SilentlyContinue
}
if (Test-Path -LiteralPath $InstallRoot) {
    Remove-Item -LiteralPath $InstallRoot -Recurse -Force
}
if ($RemoveData -and (Test-Path -LiteralPath $AppDataPath)) {
    Remove-Item -LiteralPath $AppDataPath -Recurse -Force
}
Write-Output "LOCAL_BETA_UNINSTALLED=$InstallRoot"
Write-Output "LOCAL_BETA_DATA_PRESERVED=$(-not $RemoveData)"
