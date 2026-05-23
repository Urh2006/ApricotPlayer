param(
    [string]$SourceDir = "",
    [string]$OutputPath = ""
)

$ErrorActionPreference = "Stop"
$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path

if (-not $SourceDir) {
    $SourceDir = Join-Path $projectRoot "release-dist\installer-app\ApricotPlayer"
}

if (-not $OutputPath) {
    $OutputPath = Join-Path $projectRoot "release-dist\ApricotPlayer.zip"
}

$SourceDir = (Resolve-Path $SourceDir).Path
$nestedExe = Join-Path $SourceDir "ApricotPlayer\ApricotPlayer.exe"
if (Test-Path $nestedExe) {
    $SourceDir = Join-Path $SourceDir "ApricotPlayer"
    Write-Host "Detected double-nested PyInstaller folder. Adjusting SourceDir to: $SourceDir"
}
$sourceExe = Join-Path $SourceDir "ApricotPlayer.exe"
if (-not (Test-Path $sourceExe)) {
    throw "ApricotPlayer.exe was not found in source directory: $SourceDir"
}

$outputDir = Split-Path -Parent $OutputPath
New-Item -ItemType Directory -Path $outputDir -Force | Out-Null

try {
    if (Test-Path $OutputPath) {
        Remove-Item -LiteralPath $OutputPath -Force
    }
    python (Join-Path $projectRoot "scripts\zip_folder.py") $SourceDir $OutputPath
    Get-Item -LiteralPath $OutputPath
}
catch {
    throw $_
}


