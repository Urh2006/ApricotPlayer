param(
    [string]$AppVersion = "",
    [string]$ExecutablePath = "",
    [string]$OutputDir = "",
    [string]$InnoSetupCompiler = ""
)

$ErrorActionPreference = "Stop"
$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path

if (-not $AppVersion) {
    $mainPath = Join-Path $projectRoot "wx_main.py"
    $versionLine = Select-String -Path $mainPath -Pattern '^APP_VERSION\s*=\s*"([^"]+)"' | Select-Object -First 1
    if (-not $versionLine) {
        throw "Could not read APP_VERSION from wx_main.py."
    }
    $AppVersion = $versionLine.Matches[0].Groups[1].Value
}

if (-not $ExecutablePath) {
    $ExecutablePath = Join-Path $projectRoot "release-dist\ApricotPlayer.exe"
}

if (-not $OutputDir) {
    $OutputDir = Join-Path $projectRoot "release-dist"
}

if (-not (Test-Path $ExecutablePath)) {
    throw "Executable not found: $ExecutablePath"
}

New-Item -ItemType Directory -Path $OutputDir -Force | Out-Null

if (-not $InnoSetupCompiler) {
    $candidates = @(
        (Get-Command "ISCC.exe" -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source -ErrorAction SilentlyContinue),
        "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe",
        "C:\Program Files (x86)\Inno Setup 6\ISCC.exe",
        "C:\Program Files\Inno Setup 6\ISCC.exe"
    ) | Where-Object { $_ -and (Test-Path $_) }

    if (-not $candidates) {
        throw "Inno Setup Compiler (ISCC.exe) was not found. Install Inno Setup 6, then rerun this script."
    }

    $InnoSetupCompiler = @($candidates)[0]
}

$issPath = Join-Path $projectRoot "installer\ApricotPlayer.iss"
if (-not (Test-Path $issPath)) {
    throw "Installer script not found: $issPath"
}

& $InnoSetupCompiler `
    "/DMyAppVersion=$AppVersion" `
    "/DSourceExe=$ExecutablePath" `
    "/DOutputDir=$OutputDir" `
    $issPath
