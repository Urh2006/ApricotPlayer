//! Network and file-system side of the updater: Python `open_url` with
//! trusted redirects, the user's `components` folder and the local test feed.

use std::{
    ffi::OsString,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

use apricot_updater::{
    UpdateTransport,
    package::{validate_https_response_url, validate_trusted_https_url},
};
use reqwest::{StatusCode, blocking::Client, redirect::Policy};
use serde_json::Value;

/// Python `UPDATE_DOWNLOAD_CHUNK_SIZE`.
const CHUNK_SIZE: usize = 512 * 1024;

/// Local betas only: a folder that stands in for GitHub (see
/// [`LocalFeedTransport`]).
pub const UPDATE_TEST_FEED_VARIABLE: &str = "APRICOT_UPDATE_TEST_FEED";

/// Python `open_url` for update metadata and packages.
pub struct HttpUpdateTransport {
    user_agent: String,
}

impl HttpUpdateTransport {
    #[must_use]
    pub fn new(app_version: &str) -> Self {
        Self {
            user_agent: format!("ApricotPlayer/{app_version}"),
        }
    }

    fn client(&self, timeout: Duration) -> Result<Client, String> {
        Client::builder()
            .timeout(timeout)
            .connect_timeout(Duration::from_secs(20))
            .user_agent(self.user_agent.clone())
            .redirect(Policy::custom(|attempt| {
                if attempt.previous().len() >= 10 {
                    attempt.error("too many redirects")
                } else if attempt.url().scheme() != "https" {
                    attempt.error("download redirected to a non-HTTPS URL")
                } else {
                    attempt.follow()
                }
            }))
            .build()
            .map_err(|error| error.to_string())
    }
}

fn github_request(client: &Client, url: &str, accept: &str) -> reqwest::blocking::RequestBuilder {
    client
        .get(url)
        .header("Accept", accept)
        .header("X-GitHub-Api-Version", "2022-11-28")
}

fn http_error(status: StatusCode) -> String {
    format!(
        "HTTP Error {}: {}",
        status.as_u16(),
        status.canonical_reason().unwrap_or_default()
    )
}

impl UpdateTransport for HttpUpdateTransport {
    fn get_json(&mut self, url: &str, limit: u64) -> Result<Option<Value>, String> {
        let client = self.client(Duration::from_secs(30))?;
        let response = github_request(&client, url, "application/vnd.github+json")
            .send()
            .map_err(|error| error.to_string())?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !response.status().is_success() {
            return Err(http_error(response.status()));
        }
        validate_trusted_https_url(
            response.url().as_str(),
            &["github.com"],
            "GitHub release metadata",
        )?;
        if response
            .content_length()
            .is_some_and(|length| length > limit)
        {
            return Err("GitHub release metadata is larger than the allowed limit".to_owned());
        }
        let mut bytes = Vec::new();
        response
            .take(limit + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        if bytes.len() as u64 > limit {
            return Err("GitHub release metadata is larger than the allowed limit".to_owned());
        }
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|error| error.to_string())
    }

    fn download(
        &mut self,
        url: &str,
        octet_stream: bool,
        allowed_roots: &[&str],
        destination: &Path,
        maximum: u64,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<(), String> {
        let client = self.client(Duration::from_secs(300))?;
        let accept = if octet_stream {
            "application/octet-stream"
        } else {
            "application/vnd.github+json"
        };
        let mut response = github_request(&client, url, accept)
            .send()
            .map_err(|error| error.to_string())?;
        validate_https_response_url(response.url().as_str(), allowed_roots)?;
        if !response.status().is_success() {
            return Err(http_error(response.status()));
        }
        let total = response.content_length().filter(|length| *length > 0);
        if total.is_some_and(|total| total > maximum) {
            return Err("update response is larger than the published asset size".to_owned());
        }
        let mut file = fs::File::create(destination).map_err(|error| error.to_string())?;
        let mut buffer = vec![0_u8; CHUNK_SIZE];
        let mut downloaded = 0_u64;
        loop {
            let read = response
                .read(&mut buffer)
                .map_err(|error| error.to_string())?;
            if read == 0 {
                break;
            }
            downloaded += read as u64;
            if downloaded > maximum {
                return Err("update response exceeded the published asset size".to_owned());
            }
            file.write_all(&buffer[..read])
                .map_err(|error| error.to_string())?;
            progress(downloaded, total);
        }
        file.flush().map_err(|error| error.to_string())
    }
}

/// A folder that plays GitHub for local betas and live tests: `latest.json`,
/// `releases.json` and `ytdlp-latest.json` answer the three metadata
/// requests, and a download is the file named like the last part of its
/// address. Missing JSON files are a 404. Addresses must still be the trusted
/// GitHub ones, so every check before the transport runs as with GitHub.
pub struct LocalFeedTransport {
    folder: PathBuf,
}

impl LocalFeedTransport {
    #[must_use]
    pub fn new(folder: PathBuf) -> Self {
        Self { folder }
    }

    /// The test feed of a local beta, from `APRICOT_UPDATE_TEST_FEED`.
    #[must_use]
    pub fn from_environment() -> Option<Self> {
        if apricot_updater::BUILD_CHANNEL.allows_remote_install() {
            return None;
        }
        std::env::var_os(UPDATE_TEST_FEED_VARIABLE)
            .map(PathBuf::from)
            .filter(|folder| folder.is_dir())
            .map(Self::new)
    }
}

impl UpdateTransport for LocalFeedTransport {
    fn get_json(&mut self, url: &str, limit: u64) -> Result<Option<Value>, String> {
        let name = if url == apricot_updater::flow::YTDLP_LATEST_RELEASE_API_URL {
            "ytdlp-latest.json"
        } else if url == apricot_updater::release::GITHUB_LATEST_RELEASE_API_URL {
            "latest.json"
        } else if url == apricot_updater::release::GITHUB_RELEASES_API_URL {
            "releases.json"
        } else {
            return Err(format!("unexpected test feed address: {url}"));
        };
        let path = self.folder.join(name);
        if !path.is_file() {
            return Ok(None);
        }
        let bytes = fs::read(&path).map_err(|error| error.to_string())?;
        if bytes.len() as u64 > limit {
            return Err("GitHub release metadata is larger than the allowed limit".to_owned());
        }
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|error| error.to_string())
    }

    fn download(
        &mut self,
        url: &str,
        _octet_stream: bool,
        allowed_roots: &[&str],
        destination: &Path,
        maximum: u64,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<(), String> {
        validate_https_response_url(url, allowed_roots)?;
        let name = url
            .rsplit('/')
            .next()
            .filter(|name| !name.is_empty() && !name.contains(['\\', ':']) && *name != "..")
            .ok_or_else(|| format!("unexpected test feed address: {url}"))?;
        let source = self.folder.join(name);
        let length = fs::metadata(&source)
            .map_err(|_| "HTTP Error 404: Not Found".to_owned())?
            .len();
        if length > maximum {
            return Err("update response is larger than the published asset size".to_owned());
        }
        fs::copy(&source, destination).map_err(|error| error.to_string())?;
        progress(length, Some(length));
        Ok(())
    }
}

/// The transport for update checks: the local test feed when a local beta has
/// one, otherwise GitHub.
#[must_use]
pub fn update_transport(app_version: &str) -> Box<dyn UpdateTransport + Send> {
    match LocalFeedTransport::from_environment() {
        Some(feed) => Box::new(feed),
        None => Box::new(HttpUpdateTransport::new(app_version)),
    }
}

/// Whether installing a found app update is allowed: never for a local beta,
/// except from its test feed.
#[must_use]
pub fn app_update_install_allowed() -> bool {
    apricot_updater::BUILD_CHANNEL.allows_remote_install()
        || LocalFeedTransport::from_environment().is_some()
}

/// Python `COMPONENTS_DIR`: `components` in the app data folder, where
/// updated components go.
#[must_use]
pub fn user_components_directory() -> Option<PathBuf> {
    crate::discover_windows_beta_paths()
        .ok()
        .map(|paths| paths.app_data.join("components"))
}

/// Python `get_yt_dlp`: an updated `yt-dlp.exe` in the app data
/// `components` folder wins over the bundled one.
#[must_use]
pub fn preferred_ytdlp_executable(bundled_components: &Path) -> PathBuf {
    user_components_directory()
        .map(|folder| folder.join("yt-dlp.exe"))
        .filter(|path| path.is_file())
        .unwrap_or_else(|| bundled_components.join("yt-dlp.exe"))
}

/// Python `is_installed_build`: an installer left `unins000.exe`, or the app
/// lives under Program Files.
#[must_use]
pub fn is_installed_build(executable: &Path) -> bool {
    let Some(folder) = executable.parent() else {
        return false;
    };
    if folder.join("unins000.exe").is_file() {
        return true;
    }
    ["ProgramFiles", "ProgramFiles(x86)"]
        .into_iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
        .filter(|root| !root.as_os_str().is_empty())
        .any(|root| {
            let root = fs::canonicalize(&root).unwrap_or(root);
            let executable = fs::canonicalize(executable).unwrap_or_else(|_| executable.to_owned());
            executable.starts_with(root)
        })
}

/// `yt-dlp --version` of one executable, or `"0"` like Python when the
/// version cannot be read.
#[must_use]
pub fn ytdlp_version(executable: &Path) -> String {
    crate::YtDlpYoutubeEngine::new(executable)
        .and_then(|engine| engine.version())
        .unwrap_or_else(|_| "0".to_owned())
}

/// The arguments Python passes to a relaunched app.
#[must_use]
pub fn relaunch_requested(arguments: &[OsString]) -> bool {
    arguments
        .iter()
        .any(|argument| argument == apricot_updater::script::UPDATE_RELAUNCH_ARG)
}

/// Python `mark_update_relaunch_window`.
pub fn mark_update_relaunch_window(app_data: &Path, now: f64) {
    let _ = fs::create_dir_all(app_data);
    let payload = serde_json::json!({ "expires_at": now + 45.0 });
    let _ = fs::write(app_data.join("updated-relaunch.json"), payload.to_string());
}

/// Python `suppress_already_open_for_update`.
#[must_use]
pub fn suppress_already_open_for_update(app_data: &Path, now: f64) -> bool {
    let path = app_data.join("updated-relaunch.json");
    let Ok(text) = fs::read_to_string(&path) else {
        return false;
    };
    let expires = serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|value| value.get("expires_at").and_then(Value::as_f64))
        .unwrap_or_default();
    if expires >= now {
        return true;
    }
    let _ = fs::remove_file(path);
    false
}

#[cfg(test)]
mod tests {
    use std::fs;

    use apricot_updater::UpdateTransport;

    use super::{
        LocalFeedTransport, mark_update_relaunch_window, suppress_already_open_for_update,
    };

    #[test]
    fn local_feed_answers_the_github_addresses() {
        let folder = tempfile::tempdir().expect("folder");
        fs::write(
            folder.path().join("latest.json"),
            br#"{"tag_name":"v2.0.1"}"#,
        )
        .expect("latest");
        fs::write(folder.path().join("ApricotPlayer2Beta.zip"), b"zip").expect("zip");
        let mut feed = LocalFeedTransport::new(folder.path().to_owned());
        assert_eq!(
            feed.get_json(apricot_updater::release::GITHUB_LATEST_RELEASE_API_URL, 100)
                .expect("latest")
                .expect("json")["tag_name"],
            "v2.0.1"
        );
        assert_eq!(
            feed.get_json(apricot_updater::release::GITHUB_RELEASES_API_URL, 100),
            Ok(None)
        );
        let target = folder.path().join("out.zip");
        let mut seen = None;
        feed.download(
            "https://github.com/Urh2006/ApricotPlayer/releases/download/v2.0.1/ApricotPlayer2Beta.zip",
            true,
            &["github.com"],
            &target,
            10,
            &mut |done, total| seen = Some((done, total)),
        )
        .expect("download");
        assert_eq!(fs::read(&target).expect("copy"), b"zip");
        assert_eq!(seen, Some((3, Some(3))));
        assert!(
            feed.download(
                "https://example.com/ApricotPlayer2Beta.zip",
                true,
                &["github.com"],
                &target,
                10,
                &mut |_, _| {}
            )
            .is_err()
        );
    }

    /// Live check against GitHub: downloads the newest official `yt-dlp.exe`
    /// into a temporary folder through the real trust checks.
    #[test]
    #[ignore = "downloads yt-dlp.exe from GitHub"]
    fn live_ytdlp_update_from_github() {
        let folder = tempfile::tempdir().expect("folder");
        let mut transport = super::HttpUpdateTransport::new("2.0.0-dev.1");
        let mut updating = false;
        let result = apricot_updater::update_ytdlp_component(
            &mut transport,
            "2000.01.01",
            folder.path(),
            &mut || updating = true,
        );
        assert!(
            matches!(result, Ok(apricot_updater::YtdlpUpdate::Updated(_))),
            "{result:?}"
        );
        assert!(updating);
        assert!(
            fs::metadata(folder.path().join("yt-dlp.exe"))
                .expect("exe")
                .len()
                > 1_000_000
        );
    }

    #[test]
    fn relaunch_window_suppresses_the_already_open_message_for_45_seconds() {
        let folder = tempfile::tempdir().expect("folder");
        assert!(!suppress_already_open_for_update(folder.path(), 100.0));
        mark_update_relaunch_window(folder.path(), 100.0);
        assert!(suppress_already_open_for_update(folder.path(), 145.0));
        assert!(!suppress_already_open_for_update(folder.path(), 146.0));
        assert!(!folder.path().join("updated-relaunch.json").exists());
    }
}
