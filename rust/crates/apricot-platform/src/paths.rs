use std::{env, io, path::PathBuf};

use thiserror::Error;

use crate::PlatformPaths;

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
    Ok(PlatformPaths::from_windows_roots(
        &roaming, &user_home, runtime,
    ))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::PlatformPaths;

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
}
