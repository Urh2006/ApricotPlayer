use std::{env, io, path::PathBuf};

use thiserror::Error;

use crate::{ApplicationIdentity, PlatformPaths};

#[derive(Debug, Error)]
pub enum PathDiscoveryError {
    #[error("required Windows environment variable {0} is missing")]
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
    discover_windows_paths_for_identity(crate::BUILD_IDENTITY)
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
    let executable = env::current_exe()?;
    let runtime = executable
        .parent()
        .ok_or_else(|| PathDiscoveryError::MissingRuntimeParent(executable.clone()))?;
    Ok(PlatformPaths::from_windows_roots_for_identity(
        &roaming, &user_home, runtime, identity,
    ))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::{ApplicationIdentity, PlatformPaths};

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
