param(
    [string]$OutputDir = ""
)

$ErrorActionPreference = "Stop"
$RustRoot = Split-Path -Parent $PSScriptRoot
$LocalRoot = Join-Path $RustRoot ".cargo-local"
if (-not $OutputDir) {
    $OutputDir = Join-Path $LocalRoot "libmpv"
}

$ArchiveUrl = "https://github.com/shinchiro/mpv-winbuild-cmake/releases/download/20260903/mpv-dev-x86_64-20260903-git-69e63f425a.7z"
$ExpectedArchiveHash = "fac135c68a35b7639e39d72c0c365104edbaebdea39a0dfdd8c36e8c8e80faef"
$ExpectedDllHash = "673e6397920ab64a9c5b3a618f7f16d38854efe72b58665f1f84e4e873b763a4"

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

$OutputDir = Assert-PathInside $OutputDir $LocalRoot "libmpv output"
$ExistingDll = Join-Path $OutputDir "libmpv-2.dll"
if (Test-Path -LiteralPath $ExistingDll -PathType Leaf) {
    $hash = (Get-FileHash -LiteralPath $ExistingDll -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($hash -eq $ExpectedDllHash) {
        Write-Output "LIBMPV_READY=$ExistingDll"
        exit 0
    }
}

$SevenZip = @(
    (Get-Command 7z.exe -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source -First 1),
    "$env:ProgramFiles\7-Zip\7z.exe"
) | Where-Object { $_ -and (Test-Path -LiteralPath $_ -PathType Leaf) } | Select-Object -First 1
if (-not $SevenZip) {
    throw "7-Zip was not found; install 7-Zip before preparing libmpv"
}

$Token = [Guid]::NewGuid().ToString("N")
$TemporaryRoot = Assert-PathInside (Join-Path $LocalRoot "libmpv-staging-$Token") $LocalRoot "libmpv staging"
$Archive = Join-Path $TemporaryRoot "libmpv.7z"
$Extracted = Join-Path $TemporaryRoot "extracted"
$StagedOutput = Join-Path $TemporaryRoot "ready"

try {
    New-Item -ItemType Directory -Path $Extracted -Force | Out-Null
    Invoke-WebRequest -Uri $ArchiveUrl -OutFile $Archive
    $archiveHash = (Get-FileHash -LiteralPath $Archive -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($archiveHash -ne $ExpectedArchiveHash) {
        throw "libmpv archive SHA-256 verification failed"
    }
    & $SevenZip x $Archive "-o$Extracted" -y | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw "7-Zip failed with exit code $LASTEXITCODE"
    }
    $Dll = Get-ChildItem -LiteralPath $Extracted -Filter "libmpv-2.dll" -File -Recurse |
        Select-Object -First 1
    if (-not $Dll) {
        throw "libmpv-2.dll was not present in the verified archive"
    }
    $dllHash = (Get-FileHash -LiteralPath $Dll.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($dllHash -ne $ExpectedDllHash) {
        throw "libmpv DLL SHA-256 verification failed"
    }
    New-Item -ItemType Directory -Path $StagedOutput -Force | Out-Null
    Copy-Item -LiteralPath $Dll.FullName -Destination (Join-Path $StagedOutput "libmpv-2.dll")
    if (Test-Path -LiteralPath $OutputDir) {
        Remove-Item -LiteralPath $OutputDir -Recurse -Force
    }
    Move-Item -LiteralPath $StagedOutput -Destination $OutputDir
    Write-Output "LIBMPV_READY=$(Join-Path $OutputDir 'libmpv-2.dll')"
}
finally {
    if (Test-Path -LiteralPath $TemporaryRoot) {
        Remove-Item -LiteralPath $TemporaryRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
}
