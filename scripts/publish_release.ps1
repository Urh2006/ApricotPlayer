param(
    [Parameter(Mandatory = $true)]
    [string]$Tag,
    [string]$Repo = "Urh2006/ApricotPlayer",
    [string]$ExecutablePath = "",
    [string]$Title = "",
    [string]$Notes = "",
    [string]$NotesFile = ""
)

$ErrorActionPreference = "Stop"
$tempNotesFile = $null
$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path

if (-not $ExecutablePath) {
    $ExecutablePath = Join-Path $projectRoot "release-dist\ApricotPlayer.exe"
}

if (-not $Title) {
    $Title = $Tag
}

if (-not $Notes -and -not $NotesFile) {
    throw "Release notes are required. Use -Notes or -NotesFile."
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
    (Get-Command gh -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source -ErrorAction SilentlyContinue),
    "C:\Program Files\GitHub CLI\gh.exe",
    "$env:LOCALAPPDATA\Programs\GitHub CLI\gh.exe"
) | Where-Object { $_ -and (Test-Path $_) }

if (-not $ghCandidates) {
    throw "GitHub CLI was not found."
}

$gh = @($ghCandidates)[0]

if (-not (Test-Path $ExecutablePath)) {
    throw "Executable not found: $ExecutablePath"
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
        $uploadArgs = @("release", "upload", $Tag, $ExecutablePath, "--clobber", "--repo", $Repo)
        [void](Invoke-GhChecked -Arguments $editArgs)
        [void](Invoke-GhChecked -Arguments $uploadArgs)
    }
    else {
        $createArgs = @("release", "create", $Tag, $ExecutablePath, "--title", $Title, "--notes-file", $resolvedNotesFile, "--repo", $Repo)
        [void](Invoke-GhChecked -Arguments $createArgs)
    }
}
finally {
    if ($tempNotesFile -and (Test-Path $tempNotesFile)) {
        Remove-Item $tempNotesFile -Force -ErrorAction SilentlyContinue
    }
}
