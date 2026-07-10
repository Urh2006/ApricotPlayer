param(
    [Parameter(Mandatory = $true)]
    [string]$Tag,
    [string]$Repo = "Urh2006/ApricotPlayer",
    [string]$Target = "",
    [string]$ExecutablePath = "",
    [string[]]$AssetPaths = @(),
    [string]$Title = "",
    [string]$Notes = "",
    [string]$NotesFile = "",
    [switch]$PreRelease
)

$ErrorActionPreference = "Stop"
$tempNotesFile = $null
$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path

if ($Tag -notmatch '^v\d+\.\d+\.\d+(?:\.\d+)?(?:-(?:alpha|beta|rc)(?:\.\d+)?)?$') {
    throw "Invalid release tag: $Tag"
}
if ($Repo -notmatch '^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$') {
    throw "Invalid GitHub repository name: $Repo"
}

$gitCandidates = @(
    "C:\Program Files\Git\cmd\git.exe",
    "$env:LOCALAPPDATA\Programs\Git\cmd\git.exe",
    (Get-Command git.exe -CommandType Application -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source -ErrorAction SilentlyContinue)
) | Where-Object { $_ -and (Test-Path -LiteralPath $_) }
if (-not $gitCandidates) {
    throw "Git was not found."
}
$git = (Resolve-Path -LiteralPath @($gitCandidates)[0]).Path
$gitSignature = Get-AuthenticodeSignature -LiteralPath $git
if ($gitSignature.Status -ne [System.Management.Automation.SignatureStatus]::Valid) {
    throw "Refusing to run an unsigned or invalid Git executable: $git"
}

if (-not $Target) {
    $Target = (& $git -C $projectRoot rev-parse HEAD 2>$null).Trim()
    if ($LASTEXITCODE -ne 0 -or -not $Target) {
        throw "Could not determine the current commit for the release target. Pass -Target explicitly."
    }
}
$resolvedTarget = (& $git -C $projectRoot rev-parse "$Target^{commit}" 2>$null).Trim()
if ($LASTEXITCODE -ne 0 -or $resolvedTarget -notmatch '^[0-9a-fA-F]{40}$') {
    throw "Release target is not a valid commit: $Target"
}
$Target = $resolvedTarget

if (-not $ExecutablePath) {
    $ExecutablePath = Join-Path $projectRoot "release-dist\ApricotPlayer.exe"
}

if (-not $AssetPaths -or $AssetPaths.Count -eq 0) {
    $AssetPaths = @()
    $installerPath = Join-Path $projectRoot "release-dist\ApricotPlayerSetup.exe"
    if (Test-Path $installerPath) {
        $AssetPaths += $installerPath
    }
    $portableZipPath = Join-Path $projectRoot "release-dist\ApricotPlayer.zip"
    if (Test-Path $portableZipPath) {
        $AssetPaths += $portableZipPath
    }
    if (-not $AssetPaths -or $AssetPaths.Count -eq 0) {
        throw "No default release assets were found. Build ApricotPlayerSetup.exe and ApricotPlayer.zip first, or pass -AssetPaths."
    }
}

if (-not $Title) {
    $Title = $Tag
}

if (-not $Notes -and -not $NotesFile) {
    throw "Release notes are required. Use -Notes or -NotesFile."
}

# Auto-detect pre-release from the tag name (beta/alpha/rc) if not explicitly set
if (-not $PreRelease) {
    if ($Tag -match '-(beta|alpha|rc)[\.\-]?[\d]*$') {
        $PreRelease = $true
        Write-Host "Auto-detected pre-release from tag: $Tag"
    }
}

if ($NotesFile) {
    if (-not (Test-Path $NotesFile)) {
        throw "Notes file not found: $NotesFile"
    }
    $resolvedNotesFile = (Resolve-Path $NotesFile).Path
}
else {
    $tempNotesFile = Join-Path $env:TEMP ("apricotplayer-release-notes-" + [Guid]::NewGuid().ToString() + ".md")
    Set-Content -Path $tempNotesFile -Value $Notes -Encoding UTF8
    $resolvedNotesFile = $tempNotesFile
}

$ghCandidates = @(
    "C:\Program Files\GitHub CLI\gh.exe",
    "$env:LOCALAPPDATA\Programs\GitHub CLI\gh.exe",
    (Get-Command gh.exe -CommandType Application -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source -ErrorAction SilentlyContinue)
) | Where-Object { $_ -and (Test-Path $_) }

if (-not $ghCandidates) {
    throw "GitHub CLI was not found."
}

$gh = @($ghCandidates)[0]
$gh = (Resolve-Path -LiteralPath $gh).Path
$ghSignature = Get-AuthenticodeSignature -LiteralPath $gh
if ($ghSignature.Status -ne [System.Management.Automation.SignatureStatus]::Valid) {
    throw "Refusing to run an unsigned or invalid GitHub CLI executable: $gh"
}

foreach ($assetPath in $AssetPaths) {
    if (-not (Test-Path $assetPath)) {
        throw "Release asset not found: $assetPath"
    }
}

function Invoke-GhChecked {
    param(
        [Parameter(Mandatory = $true)]
        [string[]]$Arguments,
        [switch]$AllowFailure,
        [switch]$Quiet
    )

    try {
        if ($Quiet) {
            & $gh @Arguments *> $null
        }
        else {
            & $gh @Arguments | ForEach-Object { Write-Host $_ }
        }
    }
    catch {
        if (-not $AllowFailure) {
            throw
        }
    }
    $exitCode = $LASTEXITCODE
    if ($exitCode -ne 0 -and -not $AllowFailure) {
        throw "GitHub CLI failed with exit code ${exitCode}: gh $($Arguments -join ' ')"
    }
    return $exitCode
}

$releaseExists = $false
$viewArgs = @("release", "view", $Tag, "--repo", $Repo)
$viewExitCode = Invoke-GhChecked -Arguments $viewArgs -AllowFailure -Quiet
if ($viewExitCode -eq 0) {
    $releaseExists = $true
}

try {
    if ($releaseExists) {
        $editArgs = @("release", "edit", $Tag, "--title", $Title, "--notes-file", $resolvedNotesFile, "--repo", $Repo)
        [void](Invoke-GhChecked -Arguments $editArgs)
        foreach ($assetPath in $AssetPaths) {
            $uploadArgs = @("release", "upload", $Tag, $assetPath, "--clobber", "--repo", $Repo)
            [void](Invoke-GhChecked -Arguments $uploadArgs)
        }
    }
    else {
        $createArgs = @("release", "create", $Tag) + $AssetPaths + @(
            "--title", $Title,
            "--notes-file", $resolvedNotesFile,
            "--target", $Target,
            "--repo", $Repo
        )
        if ($PreRelease) {
            $createArgs += "--prerelease"
        }
        [void](Invoke-GhChecked -Arguments $createArgs)
    }
}
finally {
    if ($tempNotesFile -and (Test-Path $tempNotesFile)) {
        Remove-Item $tempNotesFile -Force -ErrorAction SilentlyContinue
    }
}
