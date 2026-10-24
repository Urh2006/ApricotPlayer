param(
    [string]$AppDataPath = (Join-Path $env:APPDATA "ApricotPlayer")
)

$ErrorActionPreference = "Stop"
$RustRoot = Split-Path -Parent $PSScriptRoot
$ResolvedAppData = (Resolve-Path -LiteralPath $AppDataPath).Path
$TemporaryRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("apricot-rust-data-" + [Guid]::NewGuid().ToString("N"))

try {
    New-Item -ItemType Directory -Path $TemporaryRoot | Out-Null
    Get-ChildItem -LiteralPath $ResolvedAppData -File | ForEach-Object {
        Copy-Item -LiteralPath $_.FullName -Destination $TemporaryRoot
    }
    $env:APRICOT_PYTHON_APP_DATA = $TemporaryRoot
    Push-Location $RustRoot
    try {
        cargo test -p apricot-storage --test python_data_compat -- --ignored --nocapture
        if ($LASTEXITCODE -ne 0) {
            throw "Python data compatibility test failed with exit code $LASTEXITCODE"
        }
    }
    finally {
        Pop-Location
        Remove-Item Env:APRICOT_PYTHON_APP_DATA -ErrorAction SilentlyContinue
    }
    Write-Output "PYTHON_DATA_COMPAT=PASS"
}
finally {
    Remove-Item -LiteralPath $TemporaryRoot -Recurse -Force -ErrorAction SilentlyContinue
}
