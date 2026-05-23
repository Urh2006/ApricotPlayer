param(
    [string]$PythonExe = "python",
    [string]$OutputDir = "$env:USERPROFILE\Downloads",
    [string]$AppName = "ApricotPlayer",
    [ValidateSet("onefile", "onedir")]
    [string]$PackageMode = "onedir"
)

$ErrorActionPreference = "Stop"

$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$workPath = Join-Path $projectRoot "build_wx"

Write-Host "Running pre-build syntax checks..."
$pythonFiles = @(
    (Join-Path $projectRoot "wx_main.py"),
    (Join-Path $projectRoot "main.py")
)
$pythonFiles += Get-ChildItem -Path (Join-Path $projectRoot "apricot") -Recurse -File -Filter "*.py" | ForEach-Object { $_.FullName }
$compileScript = @'
import pathlib
import sys

failed = []
for filename in sys.argv[1:]:
    path = pathlib.Path(filename)
    try:
        compile(path.read_text(encoding='utf-8-sig'), str(path), 'exec')
    except Exception as exc:
        failed.append((str(path), type(exc).__name__, str(exc)))

if failed:
    for filename, exc_type, message in failed:
        print('{}: {}: {}'.format(filename, exc_type, message))
    sys.exit(1)
'@
& $PythonExe -c $compileScript @pythonFiles
if ($LASTEXITCODE -ne 0) {
    throw "CRITICAL: Syntax error detected! Build aborted to prevent publishing broken executables."
}
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
    "--collect-all", "yt_dlp_ejs",
    "--hidden-import", "ctypes",
    "--hidden-import", "email.utils",
    "--hidden-import", "hashlib",
    "--hidden-import", "html",
    "--hidden-import", "shutil",
    "--hidden-import", "socket",
    "--hidden-import", "ssl",
    "--hidden-import", "subprocess",
    "--hidden-import", "tempfile",
    "--hidden-import", "urllib.error",
    "--hidden-import", "urllib.request",
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
$nodeExe = $null

try {
    $nodeCommand = Get-Command "node.exe" -ErrorAction SilentlyContinue
    if ($nodeCommand) {
        $nodeExe = $nodeCommand.Source
    }
}
catch {
    $nodeExe = $null
}

if (Test-Path (Join-Path $mpvDir "mpv.exe")) {
    $args += @("--add-data", "$mpvDir;mpv")
}

if (Test-Path (Join-Path $ffmpegDir "ffmpeg.exe")) {
    $args += @("--add-data", "$ffmpegDir;ffmpeg")
}

if (Test-Path (Join-Path $nvdaDir "nvdaControllerClient64.dll")) {
    $args += @("--add-data", "$nvdaDir;nvda")
}

if (Test-Path (Join-Path $projectRoot "apricot\locales")) {
    $args += @("--add-data", "apricot\locales;apricot\locales")
}
if (Test-Path $assetsDir) {
    $args += @("--add-data", "$assetsDir;assets")
}

if ($nodeExe -and (Test-Path $nodeExe)) {
    $args += @("--add-data", "$nodeExe;node")
}

$args += "wx_main.py"

Push-Location $projectRoot
try {
    & $PythonExe @args
}
finally {
    Pop-Location
}


