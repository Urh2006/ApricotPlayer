//! Delete all `ApricotPlayer` data (2.1.8, Settings, General, Tools and
//! backups). The data folder cannot be deleted while `ApricotPlayer` runs, so
//! after the user confirms, a small script waits for `ApricotPlayer` to exit and
//! then deletes the folder; on the Mac it also deletes `ApricotPlayer`'s
//! Keychain items. Downloads and music in the user's own folders stay.

use std::path::{Path, PathBuf};

use apricot_core::TranslationCatalog;
use apricot_updater::script::{powershell_literal, shell_literal};

/// The action Settings runs after it closes.
pub const ACTION: &str = "delete_all_data";

/// The warning before anything is deleted, with No as the default answer.
#[must_use]
pub fn question(catalog: &TranslationCatalog) -> String {
    catalog.text("delete_all_data_confirm").to_owned()
}

/// The data folder of this build, only when it is the folder `ApricotPlayer`
/// itself uses (named after the application identity, never a drive or a
/// home folder).
fn checked_folder(app_data: &Path) -> Result<PathBuf, String> {
    let expected = apricot_platform::BUILD_IDENTITY.folder_names().0;
    let named = app_data
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case(expected));
    if !app_data.is_absolute() || !named || app_data.components().count() < 4 {
        return Err(format!(
            "refusing to delete an unexpected folder: {}",
            app_data.display()
        ));
    }
    Ok(app_data.to_path_buf())
}

/// Windows: waits for this process, then deletes the folder. `Directory.Delete`
/// removes a junction inside the folder without following it.
#[must_use]
pub fn windows_script(folder: &Path, process_id: u32) -> String {
    [
        "$ErrorActionPreference = 'SilentlyContinue'".to_owned(),
        format!(
            "$folder = {}",
            powershell_literal(&folder.to_string_lossy())
        ),
        format!("Wait-Process -Id {process_id} -Timeout 60"),
        "for ($attempt = 0; $attempt -lt 40 -and (Test-Path -LiteralPath $folder); $attempt++) {"
            .to_owned(),
        "    try { [IO.Directory]::Delete($folder, $true) } catch { Start-Sleep -Milliseconds 500 }"
            .to_owned(),
        "}".to_owned(),
        "Remove-Item -LiteralPath $MyInvocation.MyCommand.Path -Force".to_owned(),
    ]
    .join("\r\n")
        + "\r\n"
}

/// macOS: waits for this process, deletes the Keychain items of this build
/// and then the folder.
#[must_use]
pub fn macos_script(folder: &Path, process_id: u32, keychain_service: &str) -> String {
    [
        format!("folder={}", shell_literal(&folder.to_string_lossy())),
        format!("service={}", shell_literal(keychain_service)),
        "i=0".to_owned(),
        format!(
            "while kill -0 {process_id} 2>/dev/null && [ \"$i\" -lt 300 ]; do sleep 0.2; i=$((i + 1)); done"
        ),
        "i=0".to_owned(),
        "while [ \"$i\" -lt 1000 ] && /usr/bin/security delete-generic-password -s \"$service\" >/dev/null 2>&1; do i=$((i + 1)); done"
            .to_owned(),
        "rm -rf \"$folder\"".to_owned(),
        "rm -f \"$0\"".to_owned(),
    ]
    .join("\n")
        + "\n"
}

/// Starts the script that deletes `app_data` once `ApricotPlayer` has exited.
/// The caller then exits.
///
/// # Errors
/// The folder is not `ApricotPlayer`'s own, or the script could not start.
pub fn schedule(app_data: &Path) -> Result<(), String> {
    let folder = checked_folder(app_data)?;
    let process_id = std::process::id();
    if cfg!(target_os = "macos") {
        let service = apricot_platform::BUILD_IDENTITY.folder_names().0;
        apricot_updater::script::launch_macos_update_script(&macos_script(
            &folder, process_id, service,
        ))
        .map(|_| ())
    } else if cfg!(windows) {
        let script = apricot_updater::script::write_update_script(
            &windows_script(&folder, process_id),
            "apricotplayer-delete-data-",
        )?;
        apricot_updater::script::launch_update_script(&script)
    } else {
        Err("deleting the data folder is not supported on this system".to_owned())
    }
}

impl crate::Shell {
    /// Asks first; on Yes, starts the deletion and exits (macOS frontend).
    pub(crate) fn delete_all_data(&mut self, ui: &mut dyn crate::ShellUi) {
        let catalog = self.catalog();
        if !ui.ask_yes_no(&question(&catalog), catalog.text("yes"), catalog.text("no")) {
            return;
        }
        let scheduled = apricot_platform::discover_app_paths()
            .map_err(|error| error.to_string())
            .and_then(|paths| schedule(&paths.app_data));
        match scheduled {
            Ok(()) => ui.exit(),
            Err(error) => ui.show_error(
                &catalog
                    .text("delete_all_data_failed")
                    .replace("{error}", &error),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data_folder() -> PathBuf {
        let name = apricot_platform::BUILD_IDENTITY.folder_names().0;
        if cfg!(windows) {
            PathBuf::from(r"C:\Users\someone\AppData\Roaming").join(name)
        } else {
            PathBuf::from("/Users/someone/Library/Application Support").join(name)
        }
    }

    #[test]
    fn only_apricotplayers_own_data_folder_may_be_deleted() {
        assert!(checked_folder(&data_folder()).is_ok());
        let root = if cfg!(windows) { r"C:\" } else { "/" };
        for folder in [
            PathBuf::from(root),
            data_folder().with_file_name("Documents"),
            PathBuf::from(apricot_platform::BUILD_IDENTITY.folder_names().0),
            PathBuf::from(root).join(apricot_platform::BUILD_IDENTITY.folder_names().0),
        ] {
            assert!(checked_folder(&folder).is_err(), "{}", folder.display());
        }
    }

    #[test]
    fn the_scripts_wait_for_apricotplayer_and_delete_only_its_folder() {
        let folder = data_folder();
        let windows = windows_script(&folder, 4242);
        assert!(windows.contains("Wait-Process -Id 4242"));
        assert!(windows.contains(&powershell_literal(&folder.to_string_lossy())));
        assert!(windows.contains("[IO.Directory]::Delete($folder, $true)"));
        assert!(!windows.contains("Remove-Item -LiteralPath $folder"));
        let mac = macos_script(&folder, 4242, "ApricotPlayer");
        assert!(mac.contains("kill -0 4242"));
        assert!(mac.contains(&format!(
            "folder={}",
            shell_literal(&folder.to_string_lossy())
        )));
        assert!(mac.contains("delete-generic-password -s \"$service\""));
        assert!(mac.contains("rm -rf \"$folder\""));
    }
}
