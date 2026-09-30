# Builds ApricotPlayer 2.0 in place of the Python version: ApricotPlayer.exe
# with Python's folders, ApricotPlayerSetup.exe from Python's own installer
# script (same AppId, so it installs over ApricotPlayer 1.x), ApricotPlayer.zip
# and SHA-256 sums, in local-dist\stable\<version>. It publishes nothing.
# -Channel local-only (default) takes app updates only from the folder in
# APRICOT_UPDATE_TEST_FEED; -Channel beta takes them from GitHub prereleases.
param(
    [ValidateSet("local-only", "beta")]
    [string]$Channel = "local-only",
    [switch]$SkipChecks,
    [switch]$AllowDirty
)

$ErrorActionPreference = "Stop"
$RustRoot = Split-Path -Parent $PSScriptRoot
$RepoRoot = Split-Path -Parent $RustRoot

Push-Location $RustRoot
try {
    if (-not $AllowDirty) {
        $Status = @(git status --porcelain --untracked-files=no)
        if ($Status.Count -gt 0) { throw "Commit the changes first; a release is built from a clean tree" }
    }
    $Metadata = cargo metadata --no-deps --format-version 1 | ConvertFrom-Json
    $Version = [string]($Metadata.packages | Where-Object { $_.name -eq "apricot-player" } | Select-Object -First 1).version
    if ($Version -notmatch '^\d+\.\d+\.\d+(-(alpha|beta|rc)\.\d+)?$') {
        # Python's parse_version ranks "dev" as a final release.
        throw "Release version $Version must be final or use alpha, beta or rc"
    }
}
finally {
    Pop-Location
}

$ReleaseRoot = Join-Path $RustRoot "local-dist\stable\$Version"
if (Test-Path -LiteralPath $ReleaseRoot) { Remove-Item -LiteralPath $ReleaseRoot -Recurse -Force }
New-Item -ItemType Directory -Path $ReleaseRoot | Out-Null
$PackageDir = Join-Path $ReleaseRoot "ApricotPlayer"

$BuildArguments = @{ OutputDir = $PackageDir; Channel = $Channel; Stable = $true }
if ($SkipChecks) { $BuildArguments.SkipChecks = $true }
& (Join-Path $PSScriptRoot "build_local_beta.ps1") @BuildArguments
if (-not $?) { throw "The package build failed" }
$Executable = Join-Path $PackageDir "ApricotPlayer.exe"
# The executable is a GUI program, so PowerShell must wait for it explicitly.
$Smoke = Start-Process -FilePath $Executable -ArgumentList "--qualification-smoke" -Wait -PassThru -WindowStyle Hidden
if ($Smoke.ExitCode -ne 0) { throw "The qualification smoke test failed" }

Compress-Archive -LiteralPath $PackageDir -DestinationPath (Join-Path $ReleaseRoot "ApricotPlayer.zip") -CompressionLevel Optimal

$Iscc = @(
    (Join-Path ${env:ProgramFiles(x86)} "Inno Setup 6\ISCC.exe"),
    (Join-Path $env:ProgramFiles "Inno Setup 6\ISCC.exe"),
    (Join-Path $env:LOCALAPPDATA "Programs\Inno Setup 6\ISCC.exe")
) | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
if (-not $Iscc) { throw "Inno Setup 6 (ISCC.exe) was not found" }
# Python's installer script unchanged: same AppId, folder, tasks and registry.
& $Iscc "/DMyAppVersion=$Version" "/DSourceDir=$PackageDir" "/DOutputDir=$ReleaseRoot" (Join-Path $RepoRoot "installer\ApricotPlayer.iss") | Out-Null
if ($LASTEXITCODE -ne 0) { throw "ISCC failed with exit code $LASTEXITCODE" }

$Sums = foreach ($Name in "ApricotPlayer.zip", "ApricotPlayerSetup.exe") {
    $Hash = (Get-FileHash -LiteralPath (Join-Path $ReleaseRoot $Name) -Algorithm SHA256).Hash.ToLowerInvariant()
    "$Hash  $Name"
}
[IO.File]::WriteAllLines((Join-Path $ReleaseRoot "SHA256SUMS.txt"), [string[]]$Sums)
Remove-Item -LiteralPath $PackageDir -Recurse -Force
Write-Output "STABLE_VERSION=$Version"
Write-Output "STABLE_FILES=$ReleaseRoot"
