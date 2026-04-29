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
    $OutputPath = Join-Path $projectRoot "release-dist\ApricotPlayerPortable.zip"
}

$SourceDir = (Resolve-Path $SourceDir).Path
$sourceExe = Join-Path $SourceDir "ApricotPlayer.exe"
if (-not (Test-Path $sourceExe)) {
    throw "ApricotPlayer.exe was not found in source directory: $SourceDir"
}

$outputDir = Split-Path -Parent $OutputPath
New-Item -ItemType Directory -Path $outputDir -Force | Out-Null

$tempRoot = Join-Path ([IO.Path]::GetTempPath()) ("ApricotPlayerPortable-" + [Guid]::NewGuid().ToString())
$portableRoot = Join-Path $tempRoot "ApricotPlayer"

try {
    New-Item -ItemType Directory -Path $tempRoot -Force | Out-Null
    Copy-Item -LiteralPath $SourceDir -Destination $portableRoot -Recurse -Force
    if (Test-Path $OutputPath) {
        Remove-Item -LiteralPath $OutputPath -Force
    }
    Compress-Archive -Path $portableRoot -DestinationPath $OutputPath -Force
    Get-Item -LiteralPath $OutputPath
}
finally {
    if (Test-Path $tempRoot) {
        Remove-Item -LiteralPath $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
}
