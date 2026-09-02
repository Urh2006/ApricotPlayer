//! Per-user Windows startup registration with isolated stable and beta identities.

#![cfg_attr(windows, allow(unsafe_code))]

use std::path::Path;

use crate::{ApplicationIdentity, PlatformError};

pub fn startup_value_name(identity: ApplicationIdentity) -> &'static str {
    match identity {
        ApplicationIdentity::Stable => "ApricotPlayer",
        ApplicationIdentity::RustBeta => "ApricotPlayer 2 Beta",
    }
}

/// Builds the quoted command stored in the current-user startup registry.
///
/// # Errors
///
/// Returns an error when the executable path is relative or contains a quote.
pub fn startup_command(executable: &Path) -> Result<String, PlatformError> {
    if !executable.is_absolute() {
        return Err(PlatformError::Operation(
            "startup executable path must be absolute".to_owned(),
        ));
    }
    let path = executable.to_string_lossy();
    if path.contains('"') {
        return Err(PlatformError::Operation(
            "startup executable path contains an invalid quote".to_owned(),
        ));
    }
    Ok(format!("\"{path}\""))
}

/// Adds or removes this identity's current-user startup value.
///
/// # Errors
///
/// Returns an error for an unsafe executable path or a rejected registry operation.
pub fn sync_startup_registration(
    identity: ApplicationIdentity,
    executable: &Path,
    enabled: bool,
) -> Result<(), PlatformError> {
    let command = startup_command(executable)?;
    sync_startup_registration_platform(startup_value_name(identity), &command, enabled)
}

#[cfg(windows)]
fn sync_startup_registration_platform(
    value_name: &str,
    command: &str,
    enabled: bool,
) -> Result<(), PlatformError> {
    use windows::{
        Win32::{
            Foundation::ERROR_FILE_NOT_FOUND,
            System::Registry::{
                HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE,
                REG_SZ, RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegSetValueExW,
            },
        },
        core::{PCWSTR, PWSTR},
    };

    struct RegistryKey(HKEY);
    impl Drop for RegistryKey {
        fn drop(&mut self) {
            unsafe {
                let _ = RegCloseKey(self.0);
            }
        }
    }

    let subkey = wide(r"Software\Microsoft\Windows\CurrentVersion\Run");
    let mut key = HKEY::default();
    let status = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            None,
            PWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_QUERY_VALUE | KEY_SET_VALUE,
            None,
            &raw mut key,
            None,
        )
    };
    registry_result(status, "open current-user startup registry")?;
    let key = RegistryKey(key);
    let name = wide(value_name);
    if enabled {
        let bytes = utf16_bytes(command);
        let status =
            unsafe { RegSetValueExW(key.0, PCWSTR(name.as_ptr()), None, REG_SZ, Some(&bytes)) };
        registry_result(status, "write current-user startup value")
    } else {
        let status = unsafe { RegDeleteValueW(key.0, PCWSTR(name.as_ptr())) };
        if status == ERROR_FILE_NOT_FOUND {
            Ok(())
        } else {
            registry_result(status, "remove current-user startup value")
        }
    }
}

#[cfg(windows)]
fn registry_result(
    status: windows::Win32::Foundation::WIN32_ERROR,
    operation: &str,
) -> Result<(), PlatformError> {
    if status.is_ok() {
        Ok(())
    } else {
        Err(PlatformError::Operation(format!(
            "{operation}: {}",
            windows::core::Error::from(status)
        )))
    }
}

#[cfg(not(windows))]
fn sync_startup_registration_platform(
    _value_name: &str,
    _command: &str,
    _enabled: bool,
) -> Result<(), PlatformError> {
    Ok(())
}

#[cfg(windows)]
fn utf16_bytes(value: &str) -> Vec<u8> {
    value
        .encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(u16::to_le_bytes)
        .collect()
}

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::ApplicationIdentity;

    use super::{startup_command, startup_value_name};

    #[test]
    fn beta_startup_identity_cannot_replace_stable_registration() {
        assert_ne!(
            startup_value_name(ApplicationIdentity::Stable),
            startup_value_name(ApplicationIdentity::RustBeta)
        );
    }

    #[test]
    fn startup_command_is_absolute_and_quoted() {
        let command = startup_command(Path::new(r"C:\Program Files\Apricot\Apricot.exe"))
            .expect("valid startup command");
        assert_eq!(command, r#""C:\Program Files\Apricot\Apricot.exe""#);
        assert!(startup_command(Path::new("Apricot.exe")).is_err());
    }
}
