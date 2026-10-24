param(
    [string]$SettingsPath = (Join-Path $env:APPDATA "ApricotPlayer\settings.json")
)

$ErrorActionPreference = "Stop"
$RustRoot = Split-Path -Parent $PSScriptRoot
$ResolvedSettings = (Resolve-Path -LiteralPath $SettingsPath).Path
$TemporaryRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("apricot-rust-settings-" + [Guid]::NewGuid().ToString("N"))
$TemporarySettings = Join-Path $TemporaryRoot "settings.json"

try {
    New-Item -ItemType Directory -Path $TemporaryRoot | Out-Null
    Copy-Item -LiteralPath $ResolvedSettings -Destination $TemporarySettings
    $env:APRICOT_PYTHON_SETTINGS = $TemporarySettings
    Push-Location $RustRoot
    try {
        cargo test -p apricot-storage --test python_settings_compat -- --ignored --nocapture
        if ($LASTEXITCODE -ne 0) {
            throw "Python settings compatibility test failed with exit code $LASTEXITCODE"
        }
    }
    finally {
        Pop-Location
        Remove-Item Env:APRICOT_PYTHON_SETTINGS -ErrorAction SilentlyContinue
    }
    Write-Output "PYTHON_SETTINGS_COMPAT=PASS"
}
finally {
    Remove-Item -LiteralPath $TemporaryRoot -Recurse -Force -ErrorAction SilentlyContinue
}
