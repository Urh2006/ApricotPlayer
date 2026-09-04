param(
    [string]$OutputDir = ""
)

$ErrorActionPreference = "Stop"
$RustRoot = Split-Path -Parent $PSScriptRoot
$LocalRoot = Join-Path $RustRoot ".cargo-local"
if (-not $OutputDir) {
    $OutputDir = Join-Path $LocalRoot "yt-dlp"
}

$Version = "2026.08.19"
$DownloadUrl = "https://github.com/yt-dlp/yt-dlp/releases/download/$Version/yt-dlp.exe"
$ExpectedHash = "66674953fe251b89f4d08c5f0e35e0728679bd67ab3d7d05c0562af101dd3e7a"

function Get-FullPath([string]$Path) {
    return [System.IO.Path]::GetFullPath($Path)
}

function Assert-PathInside([string]$Path, [string]$Parent, [string]$Label) {
    $fullPath = Get-FullPath $Path
    $fullParent = (Get-FullPath $Parent).TrimEnd('\', '/') + [System.IO.Path]::DirectorySeparatorChar
    if (-not $fullPath.StartsWith($fullParent, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "$Label must stay inside $fullParent"
    }
    return $fullPath
}

$OutputDir = Assert-PathInside $OutputDir $LocalRoot "yt-dlp output"
$ExistingExecutable = Join-Path $OutputDir "yt-dlp.exe"
if (Test-Path -LiteralPath $ExistingExecutable -PathType Leaf) {
    $hash = (Get-FileHash -LiteralPath $ExistingExecutable -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($hash -eq $ExpectedHash) {
        Write-Output "YTDLP_READY=$ExistingExecutable"
        exit 0
    }
}

$Token = [Guid]::NewGuid().ToString("N")
$TemporaryRoot = Assert-PathInside (Join-Path $LocalRoot "yt-dlp-staging-$Token") $LocalRoot "yt-dlp staging"
$DownloadedExecutable = Join-Path $TemporaryRoot "yt-dlp.exe"
$StagedOutput = Join-Path $TemporaryRoot "ready"

try {
    New-Item -ItemType Directory -Path $TemporaryRoot -Force | Out-Null
    Invoke-WebRequest -Uri $DownloadUrl -OutFile $DownloadedExecutable
    $hash = (Get-FileHash -LiteralPath $DownloadedExecutable -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($hash -ne $ExpectedHash) {
        throw "yt-dlp SHA-256 verification failed"
    }
    New-Item -ItemType Directory -Path $StagedOutput -Force | Out-Null
    Copy-Item -LiteralPath $DownloadedExecutable -Destination (Join-Path $StagedOutput "yt-dlp.exe")
    Set-Content -LiteralPath (Join-Path $StagedOutput "version.txt") -Value $Version -Encoding ascii
    if (Test-Path -LiteralPath $OutputDir) {
        Remove-Item -LiteralPath $OutputDir -Recurse -Force
    }
    Move-Item -LiteralPath $StagedOutput -Destination $OutputDir
    Write-Output "YTDLP_READY=$(Join-Path $OutputDir 'yt-dlp.exe')"
}
finally {
    if (Test-Path -LiteralPath $TemporaryRoot) {
        Remove-Item -LiteralPath $TemporaryRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
}
