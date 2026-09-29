//! Per-user Windows startup and media player registration with isolated stable
//! and beta identities.

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

/// Registry key name and display name for this identity's media player
/// registration. The beta never writes to the stable app's keys.
pub const fn media_association_names(
    identity: ApplicationIdentity,
) -> (&'static str, &'static str) {
    match identity {
        ApplicationIdentity::Stable => ("ApricotPlayer", "ApricotPlayer"),
        ApplicationIdentity::RustBeta => ("ApricotPlayer2Beta", "ApricotPlayer 2 Beta"),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RegistryData {
    /// A `REG_SZ` value.
    Text(String),
    /// An empty `REG_NONE` value, used for `OpenWithProgids`.
    Empty,
}

/// One current-user registry value written by media player registration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegistryWrite {
    pub subkey: String,
    pub name: String,
    pub data: RegistryData,
}

/// Extensions Python checks before it treats the registration as complete.
const REQUIRED_ASSOCIATION_EXTENSIONS: &[&str] =
    &[".mp3", ".mp4", ".mkv", ".m4a", ".flac", ".wav", ".webm"];

/// Lists the registry values that register this build as a media player, in the
/// same order as Python's `register_media_associations_current_user`.
///
/// # Errors
///
/// Returns an error when the executable path is relative or contains a quote.
pub fn media_association_writes(
    identity: ApplicationIdentity,
    executable: &Path,
) -> Result<Vec<RegistryWrite>, PlatformError> {
    let quoted = startup_command(executable)?;
    let command = format!("{quoted} \"%1\"");
    let icon = format!("{},0", executable.to_string_lossy());
    let (key_name, display_name) = media_association_names(identity);
    let prog_id = format!("{key_name}.Media");
    let capabilities = format!(r"Software\{key_name}\Capabilities");
    let play_with = format!("Play with {display_name}");
    let mut writes = Vec::new();
    let mut add = |subkey: String, name: &str, data: RegistryData| {
        writes.push(RegistryWrite {
            subkey,
            name: name.to_owned(),
            data,
        });
    };
    let text = |value: &str| RegistryData::Text(value.to_owned());
    add(
        r"Software\RegisteredApplications".to_owned(),
        key_name,
        text(&capabilities),
    );
    add(capabilities.clone(), "ApplicationName", text(display_name));
    add(
        capabilities.clone(),
        "ApplicationDescription",
        text("Accessible media player, YouTube player, downloader, podcast and RSS player"),
    );
    let mut extensions: Vec<_> = crate::local_media::MEDIA_EXTENSIONS
        .iter()
        .map(|extension| format!(".{extension}"))
        .collect();
    extensions.sort();
    for extension in &extensions {
        add(
            format!(r"{capabilities}\FileAssociations"),
            extension,
            text(&prog_id),
        );
        add(
            format!(r"Software\Classes\{extension}\OpenWithProgids"),
            &prog_id,
            RegistryData::Empty,
        );
        let base = format!(r"Software\Classes\SystemFileAssociations\{extension}\shell\{key_name}");
        add(base.clone(), "MUIVerb", text(&play_with));
        add(base.clone(), "Icon", text(&icon));
        add(format!(r"{base}\command"), "", text(&command));
    }
    add(
        format!(r"Software\Classes\{prog_id}"),
        "",
        text(&format!("{display_name} media file")),
    );
    add(
        format!(r"Software\Classes\{prog_id}\DefaultIcon"),
        "",
        text(&icon),
    );
    add(
        format!(r"Software\Classes\{prog_id}\shell\open\command"),
        "",
        text(&command),
    );
    for media_kind in ["audio", "video"] {
        let base =
            format!(r"Software\Classes\SystemFileAssociations\{media_kind}\shell\{key_name}");
        add(base.clone(), "MUIVerb", text(&play_with));
        add(base.clone(), "Icon", text(&icon));
        add(format!(r"{base}\command"), "", text(&command));
    }
    Ok(writes)
}

/// Returns whether this executable is already registered as a media player for
/// the current user or the machine, using Python's completeness rule.
pub fn media_association_registration_complete(
    identity: ApplicationIdentity,
    executable: &Path,
) -> bool {
    let (key_name, _) = media_association_names(identity);
    let expected = executable.to_string_lossy().to_lowercase();
    let command_matches = |command: Option<String>| {
        command.is_some_and(|command| {
            let command = command.to_lowercase();
            command.contains(&expected) && command.contains("%1")
        })
    };
    registry_roots().into_iter().any(|root| {
        let registered = read_registry_text(root, r"Software\RegisteredApplications", key_name)
            .is_some_and(|value| !value.is_empty());
        let command = read_registry_text(
            root,
            &format!(r"Software\Classes\{key_name}.Media\shell\open\command"),
            "",
        );
        registered
            && command_matches(command)
            && REQUIRED_ASSOCIATION_EXTENSIONS.iter().all(|extension| {
                command_matches(read_registry_text(
                    root,
                    &format!(
                        r"Software\Classes\SystemFileAssociations\{extension}\shell\{key_name}\command"
                    ),
                    "",
                ))
            })
    })
}

/// Registers this build as a current-user media player and tells Explorer that
/// file associations changed.
///
/// # Errors
///
/// Returns an error for an unsafe executable path or a rejected registry write.
pub fn register_media_associations(
    identity: ApplicationIdentity,
    executable: &Path,
) -> Result<(), PlatformError> {
    let writes = media_association_writes(identity, executable)?;
    register_media_associations_platform(&writes)
}

/// Opens the Windows Default apps page.
///
/// # Errors
///
/// Returns an error when Windows cannot open the page.
pub fn open_default_apps_settings() -> Result<(), PlatformError> {
    shell_open_platform("ms-settings:defaultapps", "")
}

/// Opens the Default Programs control panel, Python's fallback when the
/// Default apps page or the registration fails.
///
/// # Errors
///
/// Returns an error when Windows cannot open the control panel.
pub fn open_default_programs_control_panel() -> Result<(), PlatformError> {
    shell_open_platform("control.exe", "/name Microsoft.DefaultPrograms")
}

#[cfg(windows)]
#[derive(Clone, Copy)]
enum RegistryRoot {
    CurrentUser,
    LocalMachine,
}

#[cfg(windows)]
const fn registry_roots() -> [RegistryRoot; 2] {
    [RegistryRoot::CurrentUser, RegistryRoot::LocalMachine]
}

#[cfg(not(windows))]
#[derive(Clone, Copy)]
enum RegistryRoot {}

#[cfg(not(windows))]
const fn registry_roots() -> [RegistryRoot; 0] {
    []
}

#[cfg(windows)]
fn read_registry_text(root: RegistryRoot, subkey: &str, name: &str) -> Option<String> {
    use windows::{
        Win32::System::Registry::{
            HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW,
        },
        core::PCWSTR,
    };

    let root = match root {
        RegistryRoot::CurrentUser => HKEY_CURRENT_USER,
        RegistryRoot::LocalMachine => HKEY_LOCAL_MACHINE,
    };
    let subkey = wide(subkey);
    let name = wide(name);
    let mut size = 0_u32;
    let status = unsafe {
        RegGetValueW(
            root,
            PCWSTR(subkey.as_ptr()),
            PCWSTR(name.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&raw mut size),
        )
    };
    if status.is_err() || size == 0 {
        return None;
    }
    let mut buffer = vec![0_u16; (size as usize).div_ceil(2)];
    let status = unsafe {
        RegGetValueW(
            root,
            PCWSTR(subkey.as_ptr()),
            PCWSTR(name.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&raw mut size),
        )
    };
    if status.is_err() {
        return None;
    }
    let length = buffer
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(buffer.len());
    Some(String::from_utf16_lossy(&buffer[..length]))
}

#[cfg(not(windows))]
fn read_registry_text(root: RegistryRoot, _subkey: &str, _name: &str) -> Option<String> {
    match root {}
}

const WINDOWS_VERSION_KEY: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";

/// Python `platform.platform()` on Windows, for example
/// `Windows-11-10.0.26200-SP0`.
pub fn windows_platform_description() -> String {
    #[cfg(windows)]
    {
        let machine = RegistryRoot::LocalMachine;
        let major = read_registry_number(machine, WINDOWS_VERSION_KEY, "CurrentMajorVersionNumber");
        let minor = read_registry_number(machine, WINDOWS_VERSION_KEY, "CurrentMinorVersionNumber");
        let build = read_registry_text(machine, WINDOWS_VERSION_KEY, "CurrentBuildNumber");
        platform_description(major, minor, build.as_deref())
    }
    #[cfg(not(windows))]
    {
        platform_description(None, None, None)
    }
}

fn platform_description(major: Option<u32>, minor: Option<u32>, build: Option<&str>) -> String {
    let (Some(major), Some(minor), Some(build)) = (major, minor, build) else {
        return std::env::consts::OS.to_owned();
    };
    let release = if major == 10 && build.parse::<u32>().is_ok_and(|build| build >= 22_000) {
        "11".to_owned()
    } else {
        major.to_string()
    };
    format!("Windows-{release}-{major}.{minor}.{build}-SP0")
}

#[cfg(windows)]
fn read_registry_number(root: RegistryRoot, subkey: &str, name: &str) -> Option<u32> {
    use windows::{
        Win32::System::Registry::{
            HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RegGetValueW,
        },
        core::PCWSTR,
    };

    let root = match root {
        RegistryRoot::CurrentUser => HKEY_CURRENT_USER,
        RegistryRoot::LocalMachine => HKEY_LOCAL_MACHINE,
    };
    let subkey = wide(subkey);
    let name = wide(name);
    let mut value = 0_u32;
    let mut size = u32::try_from(size_of::<u32>()).ok()?;
    let status = unsafe {
        RegGetValueW(
            root,
            PCWSTR(subkey.as_ptr()),
            PCWSTR(name.as_ptr()),
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut value).cast()),
            Some(&raw mut size),
        )
    };
    status.is_ok().then_some(value)
}

#[cfg(windows)]
fn register_media_associations_platform(writes: &[RegistryWrite]) -> Result<(), PlatformError> {
    use windows::{
        Win32::{
            System::Registry::{
                HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_NONE, REG_OPTION_NON_VOLATILE, REG_SZ,
                RegCloseKey, RegCreateKeyExW, RegSetValueExW,
            },
            UI::Shell::{SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify},
        },
        core::{PCWSTR, PWSTR},
    };

    for write in writes {
        let subkey = wide(&write.subkey);
        let mut key = HKEY::default();
        let status = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(subkey.as_ptr()),
                None,
                PWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                None,
                &raw mut key,
                None,
            )
        };
        registry_result(status, "open media player registry key")?;
        let name = wide(&write.name);
        let status = match &write.data {
            RegistryData::Text(value) => {
                let bytes = utf16_bytes(value);
                unsafe { RegSetValueExW(key, PCWSTR(name.as_ptr()), None, REG_SZ, Some(&bytes)) }
            }
            RegistryData::Empty => unsafe {
                RegSetValueExW(key, PCWSTR(name.as_ptr()), None, REG_NONE, None)
            },
        };
        unsafe {
            let _ = RegCloseKey(key);
        }
        registry_result(status, "write media player registry value")?;
    }
    unsafe {
        SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None);
    }
    Ok(())
}

#[cfg(not(windows))]
fn register_media_associations_platform(_writes: &[RegistryWrite]) -> Result<(), PlatformError> {
    Ok(())
}

#[cfg(windows)]
fn shell_open_platform(file: &str, parameters: &str) -> Result<(), PlatformError> {
    use windows::{
        Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
        core::{PCWSTR, w},
    };

    let file = wide(file);
    let parameters = wide(parameters);
    let result = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(file.as_ptr()),
            PCWSTR(parameters.as_ptr()),
            None,
            SW_SHOWNORMAL,
        )
    };
    // ShellExecuteW reports success with a value above 32.
    if result.0 as usize > 32 {
        Ok(())
    } else {
        Err(PlatformError::Operation(
            windows::core::Error::from_thread().message(),
        ))
    }
}

#[cfg(not(windows))]
fn shell_open_platform(_file: &str, _parameters: &str) -> Result<(), PlatformError> {
    Err(PlatformError::Operation(
        "default app settings are only available on Windows".to_owned(),
    ))
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
    #[test]
    fn platform_description_matches_python_platform() {
        assert_eq!(
            super::platform_description(Some(10), Some(0), Some("26200")),
            "Windows-11-10.0.26200-SP0"
        );
        assert_eq!(
            super::platform_description(Some(10), Some(0), Some("19045")),
            "Windows-10-10.0.19045-SP0"
        );
    }

    use std::path::Path;

    use crate::ApplicationIdentity;

    use super::{
        RegistryData, media_association_names, media_association_writes, startup_command,
        startup_value_name,
    };

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

    #[test]
    fn beta_media_registration_never_touches_stable_keys() {
        assert_ne!(
            media_association_names(ApplicationIdentity::Stable).0,
            media_association_names(ApplicationIdentity::RustBeta).0
        );
        let writes = media_association_writes(
            ApplicationIdentity::RustBeta,
            Path::new(r"C:\Apps\ApricotPlayer2Beta\apricot-player.exe"),
        )
        .expect("media registration");
        assert!(writes.iter().all(|write| {
            !write.subkey.contains(r"\ApricotPlayer\")
                && !write.subkey.ends_with(r"\ApricotPlayer")
                && !write.subkey.contains("ApricotPlayer.Media")
                && write.name != "ApricotPlayer"
                && write.name != "ApricotPlayer.Media"
        }));
    }

    #[test]
    fn media_registration_follows_python_layout() {
        let executable = Path::new(r"C:\Program Files\Apricot\Apricot.exe");
        let writes = media_association_writes(ApplicationIdentity::Stable, executable)
            .expect("media registration");
        let command = r#""C:\Program Files\Apricot\Apricot.exe" "%1""#;
        assert_eq!(writes[0].subkey, r"Software\RegisteredApplications");
        assert_eq!(writes[0].name, "ApricotPlayer");
        assert_eq!(
            writes[0].data,
            RegistryData::Text(r"Software\ApricotPlayer\Capabilities".to_owned())
        );
        assert!(writes.iter().any(|write| {
            write.subkey == r"Software\Classes\.mp3\OpenWithProgids"
                && write.name == "ApricotPlayer.Media"
                && write.data == RegistryData::Empty
        }));
        assert!(writes.iter().any(|write| {
            write.subkey
                == r"Software\Classes\SystemFileAssociations\.flac\shell\ApricotPlayer\command"
                && write.data == RegistryData::Text(command.to_owned())
        }));
        assert!(writes.iter().any(|write| {
            write.subkey == r"Software\Classes\SystemFileAssociations\video\shell\ApricotPlayer"
                && write.name == "MUIVerb"
                && write.data == RegistryData::Text("Play with ApricotPlayer".to_owned())
        }));
        let associations = writes
            .iter()
            .filter(|write| write.subkey.ends_with(r"\FileAssociations"))
            .count();
        assert_eq!(associations, 53);
    }
}
