# Builds the distributable ApricotPlayer 2 Beta release files locally:
# the portable zip, the per-user installer and their SHA-256 sums, in
# local-dist\release\<version>. It publishes nothing.
param(
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

$ReleaseRoot = Join-Path $RustRoot "local-dist\release\$Version"
if (Test-Path -LiteralPath $ReleaseRoot) { Remove-Item -LiteralPath $ReleaseRoot -Recurse -Force }
New-Item -ItemType Directory -Path $ReleaseRoot | Out-Null
$PackageDir = Join-Path $ReleaseRoot "ApricotPlayer2Beta"

$BuildArguments = @{ OutputDir = $PackageDir; Channel = "beta" }
if ($SkipChecks) { $BuildArguments.SkipChecks = $true }
& (Join-Path $PSScriptRoot "build_local_beta.ps1") @BuildArguments
if (-not $?) { throw "The package build failed" }
$Executable = Join-Path $PackageDir "ApricotPlayer2Beta.exe"
& $Executable --qualification-smoke
if ($LASTEXITCODE -ne 0) { throw "The qualification smoke test failed" }

# Portable zip: one root folder, as the updater expects.
$Zip = Join-Path $ReleaseRoot "ApricotPlayer2Beta.zip"
Compress-Archive -LiteralPath $PackageDir -DestinationPath $Zip -CompressionLevel Optimal

# Registry section: the same values as Settings "Set default player".
$Placeholder = "C:\APRICOT_APP_DIR\ApricotPlayer2Beta.exe"
$Lines = & $Executable --qualification-media-associations $Placeholder
if ($LASTEXITCODE -ne 0) { throw "Listing the media associations failed" }
$Section = New-Object System.Collections.Generic.List[string]
function Inno([string]$Text) { return $Text.Replace('{', '{{').Replace('"', '""').Replace('C:\APRICOT_APP_DIR', '{app}') }
$Owned = [System.Collections.Generic.List[string]]::new()
foreach ($Line in $Lines) {
    $Subkey, $Name, $Kind, $Data = $Line -split "`t", 4
    # Keys only ApricotPlayer 2 Beta uses are removed whole on uninstall;
    # shared keys lose only their ApricotPlayer 2 Beta value.
    $Root = $null
    if ($Subkey -match '^(Software\\ApricotPlayer2Beta|Software\\Classes\\ApricotPlayer2Beta\.Media|Software\\Classes\\SystemFileAssociations\\[^\\]+\\shell\\ApricotPlayer2Beta)(\\|$)') {
        $Root = $Matches[1]
        if (-not $Owned.Contains($Root)) {
            $Owned.Add($Root)
            $Section.Add("Root: HKCU; Subkey: `"$(Inno $Root)`"; Tasks: mediaassoc; Flags: uninsdeletekey")
        }
    }
    $Entry = "Root: HKCU; Subkey: `"$(Inno $Subkey)`"; ValueType: $Kind; ValueName: `"$(Inno $Name)`""
    if ($Kind -eq "string") { $Entry += "; ValueData: `"$(Inno $Data)`"" }
    $Entry += "; Tasks: mediaassoc"
    if (-not $Root) { $Entry += "; Flags: uninsdeletevalue" }
    $Section.Add($Entry)
}
$RegistryInclude = Join-Path $ReleaseRoot "media-associations.iss"
[IO.File]::WriteAllLines($RegistryInclude, $Section, (New-Object System.Text.UTF8Encoding $true))

$Iscc = @(
    (Join-Path ${env:ProgramFiles(x86)} "Inno Setup 6\ISCC.exe"),
    (Join-Path $env:ProgramFiles "Inno Setup 6\ISCC.exe"),
    (Join-Path $env:LOCALAPPDATA "Programs\Inno Setup 6\ISCC.exe")
) | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
if (-not $Iscc) { throw "Inno Setup 6 (ISCC.exe) was not found" }
& $Iscc "/DMyAppVersion=$Version" "/DSourceDir=$PackageDir" "/DOutputDir=$ReleaseRoot" "/DRegistryInclude=$RegistryInclude" (Join-Path $RepoRoot "installer\ApricotPlayer2Beta.iss") | Out-Null
if ($LASTEXITCODE -ne 0) { throw "ISCC failed with exit code $LASTEXITCODE" }
Remove-Item -LiteralPath $RegistryInclude

$Sums = foreach ($Name in "ApricotPlayer2Beta.zip", "ApricotPlayer2BetaSetup.exe") {
    $Hash = (Get-FileHash -LiteralPath (Join-Path $ReleaseRoot $Name) -Algorithm SHA256).Hash.ToLowerInvariant()
    "$Hash  $Name"
}
[IO.File]::WriteAllLines((Join-Path $ReleaseRoot "SHA256SUMS.txt"), [string[]]$Sums)
Remove-Item -LiteralPath $PackageDir -Recurse -Force
Write-Output "RELEASE_VERSION=$Version"
Write-Output "RELEASE_FILES=$ReleaseRoot"
