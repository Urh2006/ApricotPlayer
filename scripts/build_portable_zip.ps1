param(
    [string]$SourceDir = "",
    [string]$OutputPath = "",
    [string]$PythonExe = "python"
)

$ErrorActionPreference = "Stop"
$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$pythonCommand = Get-Command $PythonExe -CommandType Application -ErrorAction SilentlyContinue
if (-not $pythonCommand) {
    throw "Python executable was not found: $PythonExe"
}
$PythonExe = $pythonCommand.Source
$pythonSignature = Get-AuthenticodeSignature -LiteralPath $PythonExe
if ($pythonSignature.Status -ne [System.Management.Automation.SignatureStatus]::Valid) {
    throw "Refusing to package with an unsigned or invalid Python executable: $PythonExe"
}

if (-not $SourceDir) {
    $SourceDir = Join-Path $projectRoot "release-dist\installer-app\ApricotPlayer"
}

if (-not $OutputPath) {
    $OutputPath = Join-Path $projectRoot "release-dist\ApricotPlayer.zip"
}

$SourceDir = (Resolve-Path $SourceDir).Path
$nestedExe = Join-Path $SourceDir "ApricotPlayer\ApricotPlayer.exe"
if (Test-Path $nestedExe) {
    $SourceDir = Join-Path $SourceDir "ApricotPlayer"
    Write-Host "Detected double-nested PyInstaller folder. Adjusting SourceDir to: $SourceDir"
}
$sourceExe = Join-Path $SourceDir "ApricotPlayer.exe"
if (-not (Test-Path $sourceExe)) {
    throw "ApricotPlayer.exe was not found in source directory: $SourceDir"
}

$outputDir = Split-Path -Parent $OutputPath
New-Item -ItemType Directory -Path $outputDir -Force | Out-Null

try {
    if (Test-Path $OutputPath) {
        Remove-Item -LiteralPath $OutputPath -Force
    }
    & $PythonExe (Join-Path $projectRoot "scripts\zip_folder.py") $SourceDir $OutputPath
    Get-Item -LiteralPath $OutputPath
}
catch {
    throw $_
}


