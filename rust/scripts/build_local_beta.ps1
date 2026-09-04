param(
    [string]$OutputDir = "",
    [switch]$SkipChecks
)

$ErrorActionPreference = "Stop"
$RustRoot = Split-Path -Parent $PSScriptRoot
$LocalDistRoot = Join-Path $RustRoot "local-dist"
if (-not $OutputDir) {
    $OutputDir = Join-Path $LocalDistRoot "ApricotPlayer2Beta"
}

function Get-FullPath([string]$Path) {
    return [System.IO.Path]::GetFullPath($Path)
}

function Assert-PathInside([string]$Path, [string]$Parent, [string]$Label) {
    $fullPath = Get-FullPath $Path
    $fullParent = (Get-FullPath $Parent).TrimEnd('\', '/') + [System.IO.Path]::DirectorySeparatorChar
    if (-not $fullPath.StartsWith($fullParent, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "$Label must stay inside $fullParent"
    }
    return $fullPath
}

$OutputDir = Assert-PathInside $OutputDir $LocalDistRoot "Local beta output"
$OutputParent = Split-Path -Parent $OutputDir
$StagingDir = Join-Path $OutputParent ("." + (Split-Path -Leaf $OutputDir) + ".staging-" + [Guid]::NewGuid().ToString("N"))
$BackupDir = Join-Path $OutputParent ("." + (Split-Path -Leaf $OutputDir) + ".backup-" + [Guid]::NewGuid().ToString("N"))
Assert-PathInside $StagingDir $LocalDistRoot "Build staging directory" | Out-Null
Assert-PathInside $BackupDir $LocalDistRoot "Build backup directory" | Out-Null

Push-Location $RustRoot
try {
    if (-not $SkipChecks) {
        cargo test --workspace
        if ($LASTEXITCODE -ne 0) { throw "cargo test failed with exit code $LASTEXITCODE" }
        cargo clippy --workspace --all-targets -- -D warnings
        if ($LASTEXITCODE -ne 0) { throw "cargo clippy failed with exit code $LASTEXITCODE" }
    }

    cargo build --release -p apricot-player
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed with exit code $LASTEXITCODE" }

    $BuiltExe = Join-Path $RustRoot "target\release\apricot-player.exe"
    if (-not (Test-Path -LiteralPath $BuiltExe -PathType Leaf)) {
        throw "Rust executable was not produced at $BuiltExe"
    }

    $Metadata = cargo metadata --no-deps --format-version 1 | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0) { throw "cargo metadata failed with exit code $LASTEXITCODE" }
    $Package = $Metadata.packages | Where-Object { $_.name -eq "apricot-player" } | Select-Object -First 1
    if (-not $Package) { throw "apricot-player package metadata was not found" }
    $Commit = (git rev-parse HEAD).Trim()
    if ($LASTEXITCODE -ne 0) { throw "git rev-parse failed with exit code $LASTEXITCODE" }
    $GitStatus = @(git status --porcelain --untracked-files=normal)
    if ($LASTEXITCODE -ne 0) { throw "git status failed with exit code $LASTEXITCODE" }
    $Dirty = $GitStatus.Count -gt 0
    $RustVersion = ((rustc --version) | Select-Object -First 1).Trim()
    if ($LASTEXITCODE -ne 0) { throw "rustc --version failed with exit code $LASTEXITCODE" }

    New-Item -ItemType Directory -Path $StagingDir -Force | Out-Null
    Copy-Item -LiteralPath $BuiltExe -Destination (Join-Path $StagingDir "ApricotPlayer2Beta.exe")
    $NvdaSource = Join-Path (Split-Path -Parent $RustRoot) "vendor\nvda\nvdaControllerClient64.dll"
    if (-not (Test-Path -LiteralPath $NvdaSource -PathType Leaf)) {
        throw "Bundled NVDA Controller Client was not found at $NvdaSource"
    }
    $NvdaDestination = Join-Path $StagingDir "nvda"
    New-Item -ItemType Directory -Path $NvdaDestination -Force | Out-Null
    Copy-Item -LiteralPath $NvdaSource -Destination $NvdaDestination
    $MpvSource = Join-Path (Split-Path -Parent $RustRoot) "vendor\mpv"
    $MpvExecutable = Join-Path $MpvSource "mpv.exe"
    $MpvD3dCompiler = Join-Path $MpvSource "d3dcompiler_43.dll"
    foreach ($RequiredMpvFile in @($MpvExecutable, $MpvD3dCompiler)) {
        if (-not (Test-Path -LiteralPath $RequiredMpvFile -PathType Leaf)) {
            throw "Bundled mpv runtime file was not found at $RequiredMpvFile"
        }
    }
    $MpvDestination = Join-Path $StagingDir "mpv"
    New-Item -ItemType Directory -Path $MpvDestination -Force | Out-Null
    Copy-Item -LiteralPath $MpvExecutable -Destination $MpvDestination
    Copy-Item -LiteralPath $MpvD3dCompiler -Destination $MpvDestination
    $BuildInfo = [ordered]@{
        schema_version = 1
        application_id = "ApricotPlayer.RustBeta"
        product_name = "ApricotPlayer 2 Beta"
        executable_name = "ApricotPlayer2Beta.exe"
        version = [string]$Package.version
        commit = $Commit
        dirty = $Dirty
        built_at_utc = [DateTime]::UtcNow.ToString("o")
        rust = $RustVersion
        data_schema_version = 1
        update_channel = "local-only"
        app_data_directory = "%APPDATA%\ApricotPlayer2Beta"
        bundled_components = [ordered]@{
            nvda_controller_client = "nvda/nvdaControllerClient64.dll"
            mpv = "mpv/mpv.exe"
            mpv_d3d_compiler = "mpv/d3dcompiler_43.dll"
        }
    }
    $BuildInfo | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $StagingDir "build-info.json") -Encoding utf8

    $ManifestFiles = @()
    Get-ChildItem -LiteralPath $StagingDir -File -Recurse | Sort-Object FullName | ForEach-Object {
        $Relative = $_.FullName.Substring($StagingDir.Length).TrimStart('\', '/').Replace('\', '/')
        $ManifestFiles += [ordered]@{
            path = $Relative
            sha256 = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
            bytes = $_.Length
        }
    }
    $Manifest = [ordered]@{
        schema_version = 1
        application_id = "ApricotPlayer.RustBeta"
        files = $ManifestFiles
    }
    $Manifest | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $StagingDir "package-manifest.json") -Encoding utf8

    New-Item -ItemType Directory -Path $OutputParent -Force | Out-Null
    if (Test-Path -LiteralPath $OutputDir) {
        Move-Item -LiteralPath $OutputDir -Destination $BackupDir
    }
    try {
        Move-Item -LiteralPath $StagingDir -Destination $OutputDir
    }
    catch {
        if ((Test-Path -LiteralPath $BackupDir) -and -not (Test-Path -LiteralPath $OutputDir)) {
            Move-Item -LiteralPath $BackupDir -Destination $OutputDir
        }
        throw
    }
    if (Test-Path -LiteralPath $BackupDir) {
        Remove-Item -LiteralPath $BackupDir -Recurse -Force
    }
    Write-Output "LOCAL_BETA_BUILD=$OutputDir"
}
finally {
    Pop-Location
    if (Test-Path -LiteralPath $StagingDir) {
        Remove-Item -LiteralPath $StagingDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}
