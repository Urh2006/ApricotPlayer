param(
    [string]$PythonExe = "python",
    [string]$OutputDir = "$env:USERPROFILE\Downloads",
    [string]$AppName = "ApricotPlayer",
    [ValidateSet("onefile", "onedir")]
    [string]$PackageMode = "onefile"
)

$ErrorActionPreference = "Stop"

$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$workPath = Join-Path $projectRoot "build_wx"
$specPath = $projectRoot

$args = @(
    "-m", "PyInstaller",
    "--noconfirm",
    "--clean",
    "--windowed",
    "--name", $AppName,
    "--distpath", $OutputDir,
    "--workpath", $workPath,
    "--specpath", $specPath,
    "--collect-all", "yt_dlp",
    "--exclude-module", "IPython",
    "--exclude-module", "jedi",
    "--exclude-module", "matplotlib",
    "--exclude-module", "numpy",
    "--exclude-module", "PIL",
    "--exclude-module", "tkinter"
)

if ($PackageMode -eq "onefile") {
    $args += "--onefile"
}
else {
    $args += "--onedir"
}

$mpvDir = Join-Path $projectRoot "vendor\mpv"
$ffmpegDir = Join-Path $projectRoot "vendor\ffmpeg"
$nvdaDir = Join-Path $projectRoot "vendor\nvda"
$assetsDir = Join-Path $projectRoot "assets"

if (Test-Path (Join-Path $mpvDir "mpv.exe")) {
    $args += @("--add-data", "$mpvDir;mpv")
}

if (Test-Path (Join-Path $ffmpegDir "ffmpeg.exe")) {
    $args += @("--add-data", "$ffmpegDir;ffmpeg")
}

if (Test-Path (Join-Path $nvdaDir "nvdaControllerClient64.dll")) {
    $args += @("--add-data", "$nvdaDir;nvda")
}

if (Test-Path $assetsDir) {
    $args += @("--add-data", "$assetsDir;assets")
}

$args += "wx_main.py"

Push-Location $projectRoot
try {
    & $PythonExe @args
}
finally {
    Pop-Location
}
