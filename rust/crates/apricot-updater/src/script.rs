//! Python `write_portable_zip_update_script`, `write_installer_update_script`,
//! `write_secure_update_script`, `launch_update_script` and
//! `log_update_event`.

use std::{
    fs::{self, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

use crate::release::PackageNames;

/// Python `UPDATE_RELAUNCH_ARG`.
pub const UPDATE_RELAUNCH_ARG: &str = "--updated-relaunch";
/// Python `UPDATE_LOG_MAX_BYTES` and `UPDATE_LOG_TAIL_BYTES`.
pub const UPDATE_LOG_MAX_BYTES: u64 = 2 * 1024 * 1024;
pub const UPDATE_LOG_TAIL_BYTES: u64 = 512 * 1024;

/// Python `powershell_literal`.
#[must_use]
pub fn powershell_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// What both install scripts need.
#[derive(Clone, Debug)]
pub struct UpdateScriptInput<'a> {
    pub package: &'a PackageNames,
    pub downloaded_path: &'a Path,
    /// The folder of the running executable.
    pub target_dir: &'a Path,
    pub process_id: u32,
    pub log_path: &'a Path,
    pub restart: bool,
    pub expected_sha256: &'a str,
}

fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn restart_value(restart: bool) -> &'static str {
    if restart { "$true" } else { "$false" }
}

const WAIT_FOR_PROCESS: [&str; 7] = [
    "if ($processIdToWait -gt 0) {",
    "    try { Wait-Process -Id $processIdToWait -Timeout 15 -ErrorAction SilentlyContinue } catch { Log \"Wait-Process warning: $($_.Exception.Message)\" }",
    "    try {",
    "        $stillRunning = Get-Process -Id $processIdToWait -ErrorAction SilentlyContinue",
    "        if ($stillRunning) { Log 'ApricotPlayer did not exit; forcing shutdown'; Stop-Process -Id $processIdToWait -Force -ErrorAction SilentlyContinue }",
    "    } catch { Log \"Force shutdown warning: $($_.Exception.Message)\" }",
    "}",
];

/// Python `write_portable_zip_update_script`. The Rust package has several
/// top-level items instead of `_internal`, so every item the zip carries is
/// backed up and restored the same way Python backs up `_internal`.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn portable_zip_update_script(input: &UpdateScriptInput<'_>) -> String {
    let target_exe = input.target_dir.join(input.package.executable);
    let mut lines: Vec<String> = [
        "$ErrorActionPreference = 'Stop'".to_owned(),
        format!("$source = {}", powershell_literal(&text(input.downloaded_path))),
        format!("$targetDir = {}", powershell_literal(&text(input.target_dir))),
        format!("$targetExe = {}", powershell_literal(&text(&target_exe))),
        format!("$log = {}", powershell_literal(&text(input.log_path))),
        format!("$processIdToWait = {}", input.process_id),
        format!("$restart = {}", restart_value(input.restart)),
        format!(
            "$expectedSha256 = {}",
            powershell_literal(&input.expected_sha256.to_lowercase())
        ),
        format!(
            "$packageRoot = {}",
            powershell_literal(input.package.portable_root)
        ),
        format!(
            "$executableName = {}",
            powershell_literal(input.package.executable)
        ),
        "$extractRoot = Join-Path ([IO.Path]::GetTempPath()) ('apricotplayer-portable-' + [Guid]::NewGuid().ToString())".to_owned(),
        "$backupRoot = Join-Path $targetDir ('.apricot-update-backup-' + [Guid]::NewGuid().ToString())".to_owned(),
        "$movedItems = @()".to_owned(),
        "$copiedItems = @()".to_owned(),
        "$replacementStarted = $false".to_owned(),
        "New-Item -ItemType Directory -Path (Split-Path -Parent $log) -Force | Out-Null".to_owned(),
        "function Log($message) { Add-Content -LiteralPath $log -Value ((Get-Date -Format o) + ' ' + $message) -Encoding UTF8 }".to_owned(),
        "Set-Content -LiteralPath $log -Value ((Get-Date -Format o) + ' Starting ApricotPlayer portable update') -Encoding UTF8".to_owned(),
        "Log \"Source: $source\"".to_owned(),
        "Log \"Target directory: $targetDir\"".to_owned(),
        "Start-Sleep -Milliseconds 500".to_owned(),
    ]
    .into();
    lines.extend(WAIT_FOR_PROCESS.iter().map(|line| (*line).to_owned()));
    lines.extend(
        [
            "try {",
            "    New-Item -ItemType Directory -Path $extractRoot -Force | Out-Null",
            "    $sourceHandle = [IO.File]::Open($source, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)",
            "    try {",
            "        if ($expectedSha256) {",
            "            $actualSha256 = (Get-FileHash -InputStream $sourceHandle -Algorithm SHA256).Hash.ToLowerInvariant()",
            "            if ($actualSha256 -ne $expectedSha256) { throw 'Update package changed after verification.' }",
            "            $sourceHandle.Position = 0",
            "        }",
            "        Expand-Archive -LiteralPath $source -DestinationPath $extractRoot -Force",
            "        if ($expectedSha256) {",
            "            $sourceHandle.Position = 0",
            "            $postExtractSha256 = (Get-FileHash -InputStream $sourceHandle -Algorithm SHA256).Hash.ToLowerInvariant()",
            "            if ($postExtractSha256 -ne $expectedSha256) { throw 'Update package changed during extraction.' }",
            "        }",
            "    } finally {",
            "        $sourceHandle.Dispose()",
            "    }",
            "    $sourceAppDir = Join-Path $extractRoot $packageRoot",
            "    if (-not (Test-Path -LiteralPath (Join-Path $sourceAppDir $executableName))) { throw \"$executableName was not found in the expected portable zip folder.\" }",
            "    Log \"Extracted app directory: $sourceAppDir\"",
            "    New-Item -ItemType Directory -Path $backupRoot -Force | Out-Null",
            "    $items = @(Get-ChildItem -LiteralPath $sourceAppDir -Force)",
            "    foreach ($item in $items) {",
            "        $existing = Join-Path $targetDir $item.Name",
            "        if (Test-Path -LiteralPath $existing) { Move-Item -LiteralPath $existing -Destination (Join-Path $backupRoot $item.Name) -Force -ErrorAction Stop; $movedItems += $item.Name }",
            "    }",
            "    $replacementStarted = $true",
            "    foreach ($item in $items) {",
            "        Copy-Item -LiteralPath $item.FullName -Destination $targetDir -Recurse -Force -ErrorAction Stop",
            "        $copiedItems += $item.Name",
            "    }",
            "    if (-not (Test-Path -LiteralPath $targetExe)) { throw \"Updated $executableName is missing after copy.\" }",
            "    if ((Get-Item -LiteralPath $targetExe).Length -lt 1048576) { throw \"Updated $executableName is too small.\" }",
            "} catch {",
            "    Log \"Portable update failed: $($_.Exception.Message)\"",
            "    try {",
            "        if ($replacementStarted) {",
            "            foreach ($name in $copiedItems) { $copied = Join-Path $targetDir $name; if (Test-Path -LiteralPath $copied) { Remove-Item -LiteralPath $copied -Recurse -Force -ErrorAction SilentlyContinue } }",
            "        }",
            "        foreach ($name in $movedItems) { $saved = Join-Path $backupRoot $name; $restored = Join-Path $targetDir $name; if (Test-Path -LiteralPath $restored) { Remove-Item -LiteralPath $restored -Recurse -Force -ErrorAction SilentlyContinue }; if (Test-Path -LiteralPath $saved) { Move-Item -LiteralPath $saved -Destination $restored -Force -ErrorAction SilentlyContinue } }",
            "        if (Test-Path -LiteralPath $backupRoot) { Remove-Item -LiteralPath $backupRoot -Recurse -Force -ErrorAction SilentlyContinue }",
            "    } catch { Log \"Portable rollback warning: $($_.Exception.Message)\" }",
            "    try { if (Test-Path -LiteralPath $extractRoot) { Remove-Item -LiteralPath $extractRoot -Recurse -Force -ErrorAction SilentlyContinue } } catch { }",
            "    exit 1",
            "}",
            "if (Test-Path -LiteralPath $backupRoot) { Remove-Item -LiteralPath $backupRoot -Recurse -Force -ErrorAction SilentlyContinue }",
            "Remove-Item -LiteralPath $source -Force -ErrorAction SilentlyContinue",
            "Remove-Item -LiteralPath $extractRoot -Recurse -Force -ErrorAction SilentlyContinue",
        ]
        .iter()
        .map(|line| (*line).to_owned()),
    );
    lines.push(format!(
        "if ($restart) {{ try {{ Log 'Restarting ApricotPlayer'; Start-Process -FilePath $targetExe -WorkingDirectory $targetDir -ArgumentList {} }} catch {{ Log \"Restart warning: $($_.Exception.Message)\" }} }}",
        powershell_literal(UPDATE_RELAUNCH_ARG)
    ));
    lines.extend(
        [
            "Log 'Update complete'",
            "Start-Sleep -Seconds 2",
            "Remove-Item -LiteralPath $PSCommandPath -Force -ErrorAction SilentlyContinue",
        ]
        .iter()
        .map(|line| (*line).to_owned()),
    );
    lines.join("\n")
}

/// Python `write_installer_update_script`, with the executable and uninstall
/// display name of this build.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn installer_update_script(input: &UpdateScriptInput<'_>) -> String {
    let executable = input.package.executable;
    let display_name = input.package.display_name;
    let program_folder = display_name.to_owned();
    let mut lines: Vec<String> = vec![
        "$ErrorActionPreference = 'Stop'".to_owned(),
        format!("$source = {}", powershell_literal(&text(input.downloaded_path))),
        format!("$installDir = {}", powershell_literal(&text(input.target_dir))),
        format!("$log = {}", powershell_literal(&text(input.log_path))),
        format!("$processIdToWait = {}", input.process_id),
        format!("$restart = {}", restart_value(input.restart)),
        format!(
            "$expectedSha256 = {}",
            powershell_literal(&input.expected_sha256.to_lowercase())
        ),
        format!("$executableName = {}", powershell_literal(executable)),
        format!("$displayName = {}", powershell_literal(display_name)),
        "$installerLog = [IO.Path]::ChangeExtension($log, '.inno.log')".to_owned(),
        "$silentArgs = @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/CLOSEAPPLICATIONS', '/TASKS=desktopicon,mediaassoc', ('/DIR=\"' + $installDir + '\"'), ('/LOG=\"' + $installerLog + '\"'))".to_owned(),
        "$installCandidates = @()".to_owned(),
        "function Normalize-ExecutablePath([string]$path) {".to_owned(),
        "    if (-not $path) { return '' }".to_owned(),
        "    $candidate = $path.Trim().Trim('\"')".to_owned(),
        "    if ($candidate -match '^(.*?\\.exe)') { $candidate = $matches[1] }".to_owned(),
        "    return $candidate".to_owned(),
        "}".to_owned(),
        "function Add-InstallCandidate([string]$path) {".to_owned(),
        "    $candidate = Normalize-ExecutablePath $path".to_owned(),
        "    if ($candidate -and -not ($script:installCandidates -contains $candidate)) { $script:installCandidates += $candidate }".to_owned(),
        "}".to_owned(),
        "function Find-InstalledApricotExe {".to_owned(),
        "    $roots = @(".to_owned(),
        "        'HKLM:\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\*',".to_owned(),
        "        'HKLM:\\Software\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\*',".to_owned(),
        "        'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\*'".to_owned(),
        "    )".to_owned(),
        "    foreach ($root in $roots) {".to_owned(),
        "        try {".to_owned(),
        "            $items = @(Get-ItemProperty -Path $root -ErrorAction SilentlyContinue)".to_owned(),
        "            foreach ($item in $items) {".to_owned(),
        "                if ($item.DisplayName -ne $displayName) { continue }".to_owned(),
        "                if ($item.InstallLocation) {".to_owned(),
        "                    $candidate = Join-Path $item.InstallLocation $executableName".to_owned(),
        "                    if (Test-Path -LiteralPath $candidate) { return $candidate }".to_owned(),
        "                }".to_owned(),
        "                $icon = Normalize-ExecutablePath ([string]$item.DisplayIcon)".to_owned(),
        "                if ($icon -and (Test-Path -LiteralPath $icon)) { return $icon }".to_owned(),
        "            }".to_owned(),
        "        } catch { }".to_owned(),
        "    }".to_owned(),
        "    return $null".to_owned(),
        "}".to_owned(),
        "function Stop-ApricotProcesses([string[]]$dirs) {".to_owned(),
        "    try {".to_owned(),
        "        $normalizedDirs = @($dirs | Where-Object { $_ } | ForEach-Object { try { [IO.Path]::GetFullPath($_).TrimEnd('\\') } catch { $_ } } | Select-Object -Unique)".to_owned(),
        "        Get-CimInstance Win32_Process -Filter \"Name = '$executableName'\" -ErrorAction SilentlyContinue | ForEach-Object {".to_owned(),
        "            $processPath = $_.ExecutablePath".to_owned(),
        "            if (-not $processPath) { return }".to_owned(),
        "            $processDir = Split-Path -Parent $processPath".to_owned(),
        "            try { $processDir = [IO.Path]::GetFullPath($processDir).TrimEnd('\\') } catch { }".to_owned(),
        "            if ($normalizedDirs -contains $processDir) {".to_owned(),
        "                Log \"Stopping ApricotPlayer process $($_.ProcessId) at $processPath\"".to_owned(),
        "                Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue".to_owned(),
        "            }".to_owned(),
        "        }".to_owned(),
        "    } catch { Log \"Process cleanup warning: $($_.Exception.Message)\" }".to_owned(),
        "}".to_owned(),
        "Add-InstallCandidate (Join-Path $installDir $executableName)".to_owned(),
        format!(
            "if ($env:ProgramFiles) {{ Add-InstallCandidate (Join-Path $env:ProgramFiles {}) }}",
            powershell_literal(&format!("{program_folder}\\{executable}"))
        ),
        format!(
            "if (${{env:ProgramFiles(x86)}}) {{ Add-InstallCandidate (Join-Path ${{env:ProgramFiles(x86)}} {}) }}",
            powershell_literal(&format!("{program_folder}\\{executable}"))
        ),
        "New-Item -ItemType Directory -Path (Split-Path -Parent $log) -Force | Out-Null".to_owned(),
        "function Log($message) { Add-Content -LiteralPath $log -Value ((Get-Date -Format o) + ' ' + $message) -Encoding UTF8 }".to_owned(),
        "Set-Content -LiteralPath $log -Value ((Get-Date -Format o) + ' Starting ApricotPlayer installer update') -Encoding UTF8".to_owned(),
        "Log \"Installer: $source\"".to_owned(),
        "Log \"Install directory: $installDir\"".to_owned(),
        "Start-Sleep -Milliseconds 500".to_owned(),
    ];
    lines.extend(WAIT_FOR_PROCESS.iter().map(|line| (*line).to_owned()));
    lines.extend(
        [
            "$knownDirs = @($installCandidates | ForEach-Object { Split-Path -Parent $_ } | Where-Object { $_ } | Select-Object -Unique)",
            "Stop-ApricotProcesses $knownDirs",
            "try {",
            "    $sourceHandle = [IO.File]::Open($source, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)",
            "    try {",
            "        if ($expectedSha256) {",
            "            $actualSha256 = (Get-FileHash -InputStream $sourceHandle -Algorithm SHA256).Hash.ToLowerInvariant()",
            "            if ($actualSha256 -ne $expectedSha256) { throw 'Update package changed after verification.' }",
            "            $sourceHandle.Position = 0",
            "        }",
            "        Log 'Launching installer'",
            "        $process = Start-Process -FilePath $source -ArgumentList $silentArgs -Verb runAs -Wait -PassThru",
            "        if ($process -and $process.ExitCode -ne 0) { throw \"Installer exited with code $($process.ExitCode)\" }",
            "    } finally {",
            "        $sourceHandle.Dispose()",
            "    }",
            "    Log 'Installer completed'",
            "    $installedExe = Find-InstalledApricotExe",
            "    if (-not $installedExe) { $installedExe = Join-Path $installDir $executableName }",
            "    Add-InstallCandidate $installedExe",
            "    if (-not (Test-Path -LiteralPath $installedExe)) { throw \"Installed $executableName was not found at $installedExe\" }",
            "    $installedItem = Get-Item -LiteralPath $installedExe",
            "    if ($installedItem.Length -lt 1048576) { throw \"Installed $executableName is too small.\" }",
            "    Log \"Installed executable: $installedExe size=$($installedItem.Length) modified=$($installedItem.LastWriteTimeUtc.ToString('o'))\"",
            "    Remove-Item -LiteralPath $source -Force -ErrorAction SilentlyContinue",
            "    $knownDirs = @($installCandidates | ForEach-Object { Split-Path -Parent $_ } | Where-Object { $_ } | Select-Object -Unique)",
            "    Stop-ApricotProcesses $knownDirs",
            "    if ($restart) {",
            "        $installedDir = Split-Path -Parent $installedExe",
            "        Log \"Restarting ApricotPlayer from $installedExe\"",
        ]
        .iter()
        .map(|line| (*line).to_owned()),
    );
    lines.push(format!(
        "        Start-Process -FilePath $installedExe -WorkingDirectory $installedDir -ArgumentList {}",
        powershell_literal(UPDATE_RELAUNCH_ARG)
    ));
    lines.extend(
        [
            "    }",
            "    Log 'Update complete'",
            "} catch {",
            "    Log \"Installer update failed: $($_.Exception.Message)\"",
            "    exit 1",
            "}",
            "Start-Sleep -Seconds 2",
            "Remove-Item -LiteralPath $PSCommandPath -Force -ErrorAction SilentlyContinue",
        ]
        .iter()
        .map(|line| (*line).to_owned()),
    );
    lines.join("\n")
}

/// Python `write_secure_update_script`: a new UTF-8 file with a byte order
/// mark in the temporary folder.
///
/// # Errors
/// The file-system error text.
pub fn write_update_script(script: &str, prefix: &str) -> Result<PathBuf, String> {
    let file = tempfile::Builder::new()
        .prefix(prefix)
        .suffix(".ps1")
        .tempfile()
        .map_err(|error| error.to_string())?;
    let (mut handle, path) = file.keep().map_err(|error| error.to_string())?;
    handle
        .write_all(b"\xEF\xBB\xBF")
        .and_then(|()| handle.write_all(script.as_bytes()))
        .map_err(|error| error.to_string())?;
    Ok(path)
}

/// Python `trusted_powershell_executable`.
#[must_use]
pub fn trusted_powershell_executable() -> Option<PathBuf> {
    let system_root =
        std::env::var_os("SystemRoot").map_or_else(|| PathBuf::from("C:\\Windows"), PathBuf::from);
    let program_files = std::env::var_os("ProgramFiles")
        .map_or_else(|| PathBuf::from("C:\\Program Files"), PathBuf::from);
    [
        system_root
            .join("System32")
            .join("WindowsPowerShell")
            .join("v1.0")
            .join("powershell.exe"),
        program_files.join("PowerShell").join("7").join("pwsh.exe"),
    ]
    .into_iter()
    .find(|candidate| candidate.is_file())
}

/// Python `launch_update_script` arguments.
#[must_use]
pub fn update_script_arguments(powershell: &Path, script: &Path) -> Vec<String> {
    let mut arguments = vec!["-NoProfile".to_owned()];
    if powershell
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case("powershell.exe"))
    {
        arguments.extend(["-ExecutionPolicy".to_owned(), "Bypass".to_owned()]);
    }
    arguments.extend(["-File".to_owned(), text(script)]);
    arguments
}

/// Python `launch_update_script`: a hidden detached `PowerShell`.
///
/// # Errors
/// `PowerShell was not found` or the spawn error.
pub fn launch_update_script(script: &Path) -> Result<(), String> {
    let powershell =
        trusted_powershell_executable().ok_or_else(|| "PowerShell was not found".to_owned())?;
    let mut command = std::process::Command::new(&powershell);
    command.args(update_script_arguments(&powershell, script));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// Python `log_update_event`: appends one UTC line, and first keeps only the
/// last 512 KiB when the log passed 2 MiB. Errors are ignored.
pub fn log_update_event(log_path: &Path, message: &str) {
    let _ = append_update_log(log_path, message, chrono::Utc::now());
}

fn append_update_log(
    log_path: &Path,
    message: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> std::io::Result<()> {
    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent)?;
    }
    if fs::metadata(log_path).is_ok_and(|metadata| metadata.len() > UPDATE_LOG_MAX_BYTES) {
        let mut source = fs::File::open(log_path)?;
        let length = source.seek(SeekFrom::End(0))?;
        source.seek(SeekFrom::Start(
            length.saturating_sub(UPDATE_LOG_TAIL_BYTES),
        ))?;
        let mut tail = Vec::new();
        source.read_to_end(&mut tail)?;
        drop(source);
        let text = format!(
            "Older update log entries were truncated.\n{}",
            String::from_utf8_lossy(&tail)
        );
        fs::write(log_path, text)?;
    }
    let line = format!("{} {message}\n", now.format("%Y-%m-%dT%H:%M:%S%.6f+00:00"));
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?
        .write_all(line.as_bytes())
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use chrono::TimeZone;

    use super::{
        UpdateScriptInput, append_update_log, installer_update_script, portable_zip_update_script,
        powershell_literal, update_script_arguments, write_update_script,
    };
    use crate::release::RUST_BETA_PACKAGE;

    fn input<'a>(folder: &'a Path, log: &'a Path, source: &'a Path) -> UpdateScriptInput<'a> {
        UpdateScriptInput {
            package: &RUST_BETA_PACKAGE,
            downloaded_path: source,
            target_dir: folder,
            process_id: 42,
            log_path: log,
            restart: true,
            expected_sha256: "ABC",
        }
    }

    #[test]
    fn literals_quote_like_python() {
        assert_eq!(powershell_literal("it's"), "'it''s'");
    }

    #[test]
    fn portable_script_backs_up_every_packaged_item_and_relaunches() {
        let folder = Path::new("C:\\Apps\\O'Neil");
        let log = Path::new("C:\\Data\\updater.log");
        let source = Path::new("C:\\Temp\\ApricotPlayer2Beta.zip");
        let script = portable_zip_update_script(&input(folder, log, source));
        assert!(script.starts_with("$ErrorActionPreference = 'Stop'\n"));
        assert!(script.contains("$targetDir = 'C:\\Apps\\O''Neil'"));
        assert!(script.contains("$targetExe = 'C:\\Apps\\O''Neil\\ApricotPlayer2Beta.exe'"));
        assert!(script.contains("$processIdToWait = 42"));
        assert!(script.contains("$expectedSha256 = 'abc'"));
        assert!(script.contains("$packageRoot = 'ApricotPlayer2Beta'"));
        assert!(script.contains("Expand-Archive -LiteralPath $source"));
        assert!(script.contains(
            "Move-Item -LiteralPath $existing -Destination (Join-Path $backupRoot $item.Name)"
        ));
        assert!(script.contains("-ArgumentList '--updated-relaunch'"));
        assert!(script.ends_with(
            "Remove-Item -LiteralPath $PSCommandPath -Force -ErrorAction SilentlyContinue"
        ));
    }

    #[test]
    fn installer_script_uses_the_build_names() {
        let folder = Path::new("C:\\Apps\\Beta");
        let log = Path::new("C:\\Data\\updater.log");
        let source = Path::new("C:\\Temp\\ApricotPlayer2BetaSetup.exe");
        let script = installer_update_script(&input(folder, log, source));
        assert!(script.contains("$executableName = 'ApricotPlayer2Beta.exe'"));
        assert!(script.contains("$displayName = 'ApricotPlayer 2 Beta'"));
        assert!(script.contains("'/VERYSILENT', '/SUPPRESSMSGBOXES'"));
        assert!(script.contains(
            "Join-Path $env:ProgramFiles 'ApricotPlayer 2 Beta\\ApricotPlayer2Beta.exe'"
        ));
        assert!(script.contains("-Verb runAs -Wait -PassThru"));
        assert!(!script.contains("'ApricotPlayer.exe'"));
    }

    #[test]
    fn scripts_are_written_with_a_byte_order_mark_and_launched_hidden() {
        let path = write_update_script("Log 'x'", "apricotplayer-test-").expect("script");
        let bytes = fs::read(&path).expect("read");
        fs::remove_file(&path).expect("remove");
        assert_eq!(&bytes[..3], b"\xEF\xBB\xBF");
        assert!(
            path.file_name()
                .expect("name")
                .to_string_lossy()
                .starts_with("apricotplayer-test-")
        );
        assert_eq!(
            update_script_arguments(Path::new("C:\\W\\powershell.exe"), Path::new("s.ps1")),
            ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", "s.ps1"]
        );
        assert_eq!(
            update_script_arguments(Path::new("C:\\P\\pwsh.exe"), Path::new("s.ps1")),
            ["-NoProfile", "-File", "s.ps1"]
        );
    }

    #[test]
    fn update_log_appends_and_keeps_the_tail() {
        let folder = tempfile::tempdir().expect("folder");
        let log = folder.path().join("updater.log");
        let now = chrono::Utc.with_ymd_and_hms(2026, 9, 29, 18, 0, 0).unwrap();
        append_update_log(&log, "first", now).expect("append");
        assert_eq!(
            fs::read_to_string(&log).expect("log"),
            "2026-09-29T18:00:00.000000+00:00 first\n"
        );
        fs::write(&log, vec![b'a'; 3 * 1024 * 1024]).expect("big");
        append_update_log(&log, "next", now).expect("append");
        let text = fs::read_to_string(&log).expect("log");
        assert!(text.starts_with("Older update log entries were truncated.\naaa"));
        assert!(text.ends_with("a2026-09-29T18:00:00.000000+00:00 next\n"));
        assert_eq!(text.len(), 41 + 512 * 1024 + 38);
    }
}
