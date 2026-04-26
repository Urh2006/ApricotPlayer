param(
    [Parameter(Mandatory = $true)]
    [string]$Tag,
    [string]$Repo = "Urh2006/ApricotPlayer",
    [string]$ExecutablePath = "$env:USERPROFILE\Downloads\ApricotPlayer.exe",
    [string]$Title = "",
    [string]$Notes = ""
)

$ErrorActionPreference = "Stop"

if (-not $Title) {
    $Title = $Tag
}

if (-not $Notes) {
    $Notes = "Release $Tag"
}

$ghCandidates = @(
    (Get-Command gh -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source -ErrorAction SilentlyContinue),
    "C:\Program Files\GitHub CLI\gh.exe",
    "$env:LOCALAPPDATA\Programs\GitHub CLI\gh.exe"
) | Where-Object { $_ -and (Test-Path $_) }

if (-not $ghCandidates) {
    throw "GitHub CLI was not found."
}

$gh = $ghCandidates[0]

if (-not (Test-Path $ExecutablePath)) {
    throw "Executable not found: $ExecutablePath"
}

$releaseExists = $false
try {
    & $gh release view $Tag --repo $Repo | Out-Null
    $releaseExists = $true
}
catch {
    $releaseExists = $false
}

if ($releaseExists) {
    & $gh release upload $Tag $ExecutablePath --clobber --repo $Repo
}
else {
    & $gh release create $Tag $ExecutablePath --title $Title --notes $Notes --repo $Repo
}
