param(
    [string]$PythonExe = "python",
    [string]$OutputDir = "$env:USERPROFILE\Downloads",
    [string]$AppName = "ApricotPlayer"
)

$ErrorActionPreference = "Stop"

$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$workPath = Join-Path $projectRoot "build_wx"
$specPath = $projectRoot

$args = @(
    "-m", "PyInstaller",
    "--noconfirm",
    "--clean",
    "--onefile",
    "--windowed",
    "--name", $AppName,
    "--distpath", $OutputDir,
    "--workpath", $workPath,
    "--specpath", $specPath,
    "--collect-all", "yt_dlp",
    "--collect-all", "wx"
)

$mpvDir = Join-Path $projectRoot "vendor\mpv"
$ffmpegDir = Join-Path $projectRoot "vendor\ffmpeg"

if (Test-Path (Join-Path $mpvDir "mpv.exe")) {
    $args += @("--add-data", "$mpvDir;mpv")
}

if (Test-Path (Join-Path $ffmpegDir "ffmpeg.exe")) {
    $args += @("--add-data", "$ffmpegDir;ffmpeg")
}

$args += "wx_main.py"

Push-Location $projectRoot
try {
    & $PythonExe @args
}
finally {
    Pop-Location
}
