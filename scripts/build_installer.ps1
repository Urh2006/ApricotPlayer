param(
    [string]$AppVersion = "",
    [string]$ExecutablePath = "",
    [string]$SourceDir = "",
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
else {
    $OutputDir = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($OutputDir)
}

if ($SourceDir) {
    $SourceDir = (Resolve-Path $SourceDir).Path
    $sourceDirExe = Join-Path $SourceDir "ApricotPlayer.exe"
    if (-not (Test-Path $sourceDirExe)) {
        throw "ApricotPlayer.exe was not found in source directory: $SourceDir"
    }
}
elseif (-not (Test-Path $ExecutablePath)) {
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

$compilerArgs = @(
    "/DMyAppVersion=$AppVersion",
    "/DOutputDir=$OutputDir"
)

if ($SourceDir) {
    $compilerArgs += "/DSourceDir=$SourceDir"
}
else {
    $compilerArgs += "/DSourceExe=$ExecutablePath"
}

& $InnoSetupCompiler @compilerArgs $issPath
