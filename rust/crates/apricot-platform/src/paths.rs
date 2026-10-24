use std::{env, io, path::PathBuf};

use thiserror::Error;

use crate::{ApplicationIdentity, PlatformPaths};

#[derive(Debug, Error)]
pub enum PathDiscoveryError {
    #[error("required environment variable {0} is missing")]
    MissingEnvironment(&'static str),
    #[error("current executable path is unavailable: {0}")]
    CurrentExecutable(#[from] io::Error),
    #[error("current executable has no parent directory: {0}")]
    MissingRuntimeParent(PathBuf),
}

/// Discovers the current Windows app-data, user, and executable directories.
///
/// # Errors
///
/// Returns [`PathDiscoveryError`] when required environment values or the
/// executable directory are unavailable.
pub fn discover_windows_paths() -> Result<PlatformPaths, PathDiscoveryError> {
    discover_windows_paths_for_identity(ApplicationIdentity::Stable)
}

/// Discovers side-by-side Rust beta paths without changing the stable profile.
///
/// # Errors
///
/// Returns [`PathDiscoveryError`] when required environment values or the
/// executable directory are unavailable.
pub fn discover_windows_beta_paths() -> Result<PlatformPaths, PathDiscoveryError> {
    discover_windows_paths_for_identity(ApplicationIdentity::RustBeta)
}

/// This build's paths: Python's for the 2.0 that replaces it.
///
/// # Errors
///
/// Returns [`PathDiscoveryError`] when required environment values or the
/// executable directory are unavailable.
pub fn discover_app_paths() -> Result<PlatformPaths, PathDiscoveryError> {
    #[cfg(target_os = "macos")]
    return discover_macos_paths_for_identity(crate::BUILD_IDENTITY);
    #[cfg(not(target_os = "macos"))]
    discover_windows_paths_for_identity(crate::BUILD_IDENTITY)
}

/// The macOS folders under the user's `Library`, named like the Windows
/// app-data folders.
#[cfg(target_os = "macos")]
fn discover_macos_paths_for_identity(
    identity: ApplicationIdentity,
) -> Result<PlatformPaths, PathDiscoveryError> {
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or(PathDiscoveryError::MissingEnvironment("HOME"))?;
    Ok(PlatformPaths::from_macos_home_for_identity(
        &home,
        runtime_directory()?.as_path(),
        identity,
    ))
}

fn runtime_directory() -> Result<PathBuf, PathDiscoveryError> {
    let executable = env::current_exe()?;
    executable
        .parent()
        .map(bundled_runtime_root)
        .ok_or(PathDiscoveryError::MissingRuntimeParent(executable))
}

pub(crate) fn bundled_runtime_root(directory: &std::path::Path) -> PathBuf {
    let complete =
        |root: &std::path::Path| root.join("components").is_dir() && root.join("mpv").is_dir();
    let internal = directory.join("_internal");
    if complete(&internal) {
        return internal;
    }
    // The YouTube helper runs from the components subfolder.
    if directory
        .file_name()
        .is_some_and(|name| name == "components")
        && let Some(parent) = directory.parent().filter(|parent| complete(parent))
    {
        return parent.to_path_buf();
    }
    directory.to_path_buf()
}

fn discover_windows_paths_for_identity(
    identity: ApplicationIdentity,
) -> Result<PlatformPaths, PathDiscoveryError> {
    let roaming = env::var_os("APPDATA")
        .map(PathBuf::from)
        .ok_or(PathDiscoveryError::MissingEnvironment("APPDATA"))?;
    let user_home = env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .ok_or(PathDiscoveryError::MissingEnvironment("USERPROFILE"))?;
    let runtime = runtime_directory()?;
    Ok(PlatformPaths::from_windows_roots_for_identity(
        &roaming, &user_home, &runtime, identity,
    ))
}

#[cfg(test)]
// Some tests use Windows paths and run only on Windows.
#[cfg_attr(not(windows), allow(unused_imports))]
mod tests {
    use std::path::Path;

    use crate::{ApplicationIdentity, PlatformPaths};

    #[test]
    fn bundled_tools_resolve_for_both_portable_layouts_and_the_helper() {
        let folder = tempfile::tempdir().expect("folder");
        let root = folder.path();
        assert_eq!(super::bundled_runtime_root(root), root);
        let internal = root.join("_internal");
        std::fs::create_dir_all(internal.join("components")).unwrap();
        std::fs::create_dir_all(internal.join("mpv")).unwrap();
        assert_eq!(super::bundled_runtime_root(root), internal);
        assert_eq!(
            super::bundled_runtime_root(&internal.join("components")),
            internal
        );
        std::fs::create_dir_all(root.join("components")).unwrap();
        std::fs::create_dir_all(root.join("mpv")).unwrap();
        assert_eq!(super::bundled_runtime_root(root), internal);
        assert_eq!(super::bundled_runtime_root(&root.join("components")), root);
    }

    #[test]
    fn macos_paths_use_the_library_folders_with_the_windows_names() {
        let home = Path::new("/Users/tester");
        let runtime = Path::new("/Applications/ApricotPlayer.app/Contents/MacOS");
        let stable =
            PlatformPaths::from_macos_home_for_identity(home, runtime, ApplicationIdentity::Stable);
        let support = home.join("Library").join("Application Support");
        assert_eq!(stable.app_data, support.join("ApricotPlayer"));
        assert_eq!(
            stable.legacy_app_data,
            support.join("UrhasaurusYouTubePlayer")
        );
        assert_eq!(stable.cache, home.join("Library/Caches/ApricotPlayer"));
        assert_eq!(stable.logs, home.join("Library/Logs/ApricotPlayer"));
        assert_eq!(stable.downloads, home.join("Downloads/ApricotPlayer"));
        assert_eq!(stable.runtime, runtime);
        let beta = PlatformPaths::from_macos_home_for_identity(
            home,
            runtime,
            ApplicationIdentity::RustBeta,
        );
        assert_eq!(beta.app_data, support.join("ApricotPlayer2Beta"));
        assert_eq!(beta.legacy_app_data, support.join("ApricotPlayer"));
        assert_eq!(beta.cache, home.join("Library/Caches/ApricotPlayer2Beta"));
        assert_eq!(beta.downloads, stable.downloads);
    }

    // Windows paths and tools; macOS equivalents belong to the macOS units.
    #[cfg(windows)]
    #[test]
    fn windows_paths_match_the_python_layout() {
        let paths = PlatformPaths::from_windows_roots(
            Path::new(r"C:\Users\tester\AppData\Roaming"),
            Path::new(r"C:\Users\tester"),
            Path::new(r"C:\Program Files\ApricotPlayer"),
        );
        assert_eq!(
            paths.app_data,
            Path::new(r"C:\Users\tester\AppData\Roaming\ApricotPlayer")
        );
        assert_eq!(
            paths.legacy_app_data,
            Path::new(r"C:\Users\tester\AppData\Roaming\UrhasaurusYouTubePlayer")
        );
        assert_eq!(paths.cache, paths.app_data.join("cache"));
        assert_eq!(paths.logs, paths.app_data);
        assert_eq!(
            paths.downloads,
            Path::new(r"C:\Users\tester\Downloads\ApricotPlayer")
        );
    }

    // Windows paths and tools; macOS equivalents belong to the macOS units.
    #[cfg(windows)]
    #[test]
    fn rust_beta_is_side_by_side_and_reads_stable_only_as_migration_source() {
        let paths = PlatformPaths::from_windows_roots_for_identity(
            Path::new(r"C:\Users\tester\AppData\Roaming"),
            Path::new(r"C:\Users\tester"),
            Path::new(r"C:\Users\tester\AppData\Local\Programs\ApricotPlayer2Beta"),
            ApplicationIdentity::RustBeta,
        );
        assert_eq!(
            paths.app_data,
            Path::new(r"C:\Users\tester\AppData\Roaming\ApricotPlayer2Beta")
        );
        assert_eq!(
            paths.legacy_app_data,
            Path::new(r"C:\Users\tester\AppData\Roaming\ApricotPlayer")
        );
        assert_eq!(paths.cache, paths.app_data.join("cache"));
        assert_eq!(paths.logs, paths.app_data);
    }
}
