//! The worker halves of Python `app_update_worker`,
//! `download_and_install_update` and `update_ytdlp_component_package`, over an
//! injected transport so tests and local betas can use a fake server.

use std::{
    fs,
    io::Read as _,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use serde_json::Value;

use crate::{
    package::{
        UPDATE_ASSET_MAX_BYTES, file_sha256, is_sha256_digest, validate_trusted_download_url,
        validate_update_package, verify_file_sha256, verify_release_asset_file,
    },
    release::{
        PackageNames, Release, ReleaseAsset, ReleaseFeed, cumulative_changelog_text,
        fetch_latest_release,
    },
    version::{is_component_version_newer, is_newer_version},
};

/// Python `UPDATE_PROGRESS_MIN_INTERVAL`.
pub const UPDATE_PROGRESS_MIN_INTERVAL: Duration = Duration::from_millis(350);
/// The official standalone `yt-dlp` releases.
pub const YTDLP_LATEST_RELEASE_API_URL: &str =
    "https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest";
/// Python `YTDLP_WHEEL_MAX_BYTES`, also the limit for `yt-dlp.exe`.
pub const YTDLP_MAX_BYTES: u64 = 64 * 1024 * 1024;
pub const YTDLP_ASSET_NAME: &str = "yt-dlp.exe";
const YTDLP_CHECKSUMS_ASSET_NAME: &str = "SHA2-256SUMS";

/// Hosts GitHub metadata and downloads may come from.
pub const GITHUB_METADATA_HOSTS: &[&str] = &["github.com"];
pub const GITHUB_DOWNLOAD_HOSTS: &[&str] = &["github.com", "githubusercontent.com"];

/// HTTP access the update workers need.
pub trait UpdateTransport {
    /// Reads JSON from a trusted HTTPS address. A 404 is `Ok(None)`.
    ///
    /// # Errors
    /// Transport, trust or size errors.
    fn get_json(&mut self, url: &str, limit: u64) -> Result<Option<Value>, String>;

    /// Downloads `url` into `destination`, at most `maximum` bytes. The final
    /// address after redirects must be HTTPS under `allowed_roots`.
    /// `progress(downloaded, total)` gets the declared length when known.
    ///
    /// # Errors
    /// Transport, trust or size errors.
    fn download(
        &mut self,
        url: &str,
        octet_stream: bool,
        allowed_roots: &[&str],
        destination: &Path,
        maximum: u64,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<(), String>;
}

/// The Python release feed over a transport.
pub struct GithubReleaseFeed<'a> {
    pub transport: &'a mut dyn UpdateTransport,
}

impl ReleaseFeed for GithubReleaseFeed<'_> {
    fn latest_release(&mut self) -> Result<Option<Value>, String> {
        self.transport.get_json(
            crate::release::GITHUB_LATEST_RELEASE_API_URL,
            crate::release::UPDATE_METADATA_MAX_BYTES,
        )
    }

    fn releases(&mut self) -> Result<Value, String> {
        Ok(self
            .transport
            .get_json(
                crate::release::GITHUB_RELEASES_API_URL,
                crate::release::UPDATE_METADATA_MAX_BYTES,
            )?
            .unwrap_or(Value::Null))
    }
}

/// What Python `app_update_worker` decides.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppUpdateCheck {
    /// `app_up_to_date`.
    UpToDate,
    /// `update_skip_status`.
    Skipped(String),
    /// `app_update_failed` with "no Windows asset found in release".
    NoAsset,
    Available {
        release: Box<Release>,
        asset: ReleaseAsset,
    },
}

/// Python `app_update_worker` without the UI calls.
#[allow(clippy::too_many_arguments)]
pub fn check_app_update(
    feed: &mut dyn ReleaseFeed,
    package: &PackageNames,
    channel: &str,
    current_version: &str,
    skipped_version: &str,
    manual: bool,
    installed_build: bool,
    no_changelog: &str,
) -> AppUpdateCheck {
    let Some(mut release) = fetch_latest_release(feed, channel) else {
        return AppUpdateCheck::UpToDate;
    };
    let remote = release.version();
    if !is_newer_version(&remote, current_version) {
        return AppUpdateCheck::UpToDate;
    }
    // The Rust version never installs a Python 1.x release, whatever the
    // version compare says.
    if crate::version::parse_version(&remote)
        .first()
        .copied()
        .unwrap_or(0)
        < 2
    {
        return AppUpdateCheck::UpToDate;
    }
    if !manual && remote == skipped_version {
        return AppUpdateCheck::Skipped(remote);
    }
    let Some(asset) = package.find_asset(&release, installed_build).cloned() else {
        return AppUpdateCheck::NoAsset;
    };
    if let Ok(cumulative) =
        cumulative_changelog_text(feed, channel, current_version, &remote, no_changelog)
        && !cumulative.is_empty()
    {
        release.cumulative_changelog = cumulative;
    }
    AppUpdateCheck::Available {
        release: Box::new(release),
        asset,
    }
}

/// A verified package ready for Python `finish_app_update_install`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DownloadedUpdate {
    pub path: PathBuf,
    pub sha256: String,
}

/// Python `download_and_install_update` without the UI calls. The package is
/// saved in a new temporary folder; on failure the folder is removed.
///
/// # Errors
/// The first failure, as Python reports it in `app_update_failed`.
pub fn download_app_update(
    transport: &mut dyn UpdateTransport,
    package: &PackageNames,
    asset: &ReleaseAsset,
    log: &mut dyn FnMut(&str),
    progress: &mut dyn FnMut(Option<u8>),
) -> Result<DownloadedUpdate, String> {
    let folder = tempfile::Builder::new()
        .prefix("apricotplayer-update-")
        .tempdir()
        .map_err(|error| error.to_string())?;
    let result = (|| {
        let path = folder.path().join(package.safe_asset_filename(asset)?);
        log(&format!("Downloading update to {}", path.display()));
        download_update_asset(transport, asset, &path, log, progress)?;
        let size = fs::metadata(&path)
            .map(|metadata| metadata.len())
            .unwrap_or_default();
        log(&format!("Downloaded update; size={size}"));
        verify_release_asset_file(asset, &path)?;
        validate_update_package(package, &path)?;
        let sha256 = file_sha256(&path)?;
        Ok(DownloadedUpdate { path, sha256 })
    })();
    if result.is_ok() {
        // The install script removes the package after it ran.
        let _ = folder.keep();
    }
    result
}

/// Python `download_update_asset`.
fn download_update_asset(
    transport: &mut dyn UpdateTransport,
    asset: &ReleaseAsset,
    path: &Path,
    log: &mut dyn FnMut(&str),
    progress: &mut dyn FnMut(Option<u8>),
) -> Result<(), String> {
    let mut attempts = Vec::new();
    if !asset.browser_download_url.is_empty() {
        validate_trusted_download_url(&asset.browser_download_url, &["github.com"])?;
        attempts.push(asset.browser_download_url.as_str());
    }
    if !asset.api_url.is_empty() {
        validate_trusted_download_url(&asset.api_url, &["api.github.com"])?;
        attempts.push(asset.api_url.as_str());
    }
    if attempts.is_empty() {
        return Err("missing download url".to_owned());
    }
    let expected = asset.size.unwrap_or_default();
    if expected > UPDATE_ASSET_MAX_BYTES {
        return Err("update asset is larger than the safe download limit".to_owned());
    }
    let maximum = if expected == 0 {
        UPDATE_ASSET_MAX_BYTES
    } else {
        expected
    };
    let mut last_error = String::from("download failed");
    for url in attempts {
        let host = url::Url::parse(url)
            .ok()
            .and_then(|url| url.host_str().map(str::to_owned))
            .unwrap_or_default();
        log(&format!(
            "Download attempt: host={host}; asset={}; expected_size={}",
            asset.name,
            asset
                .size
                .map_or_else(|| "unknown".to_owned(), |size| size.to_string())
        ));
        let started = Instant::now();
        let mut last_percent = None;
        let mut last_report: Option<Instant> = None;
        let mut report = |downloaded: u64, total: Option<u64>| {
            let now = Instant::now();
            let due = last_report.is_none_or(|last| now - last >= UPDATE_PROGRESS_MIN_INTERVAL);
            match total.filter(|total| *total > 0) {
                Some(total) => {
                    let percent = u8::try_from((downloaded.saturating_mul(100) / total).min(100))
                        .unwrap_or(100);
                    if Some(percent) != last_percent && (percent >= 100 || due) {
                        last_percent = Some(percent);
                        last_report = Some(now);
                        progress(Some(percent));
                    }
                }
                None if due => {
                    last_report = Some(now);
                    progress(None);
                }
                None => {}
            }
        };
        match transport.download(url, true, GITHUB_DOWNLOAD_HOSTS, path, maximum, &mut report) {
            Ok(()) => {
                let bytes = fs::metadata(path)
                    .map(|metadata| metadata.len())
                    .unwrap_or_default();
                let seconds = started.elapsed().as_secs_f64().max(0.001);
                #[allow(clippy::cast_precision_loss)]
                let mbps = bytes as f64 * 8.0 / 1_000_000.0 / seconds;
                log(&format!(
                    "Download completed: bytes={bytes}; seconds={seconds:.1}; mbps={mbps:.2}"
                ));
                return Ok(());
            }
            Err(error) => {
                log(&format!("Download attempt failed from {host}: {error}"));
                let _ = fs::remove_file(path);
                last_error = error;
            }
        }
    }
    Err(last_error)
}

/// What Python `update_ytdlp_worker` reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum YtdlpUpdate {
    /// No newer release.
    Current,
    /// A newer `yt-dlp.exe` replaced the component.
    Updated(String),
}

/// Python `update_ytdlp_component_package` for the standalone
/// `yt-dlp.exe`: the newest official release goes into `components_dir`,
/// which the app prefers over the bundled copy.
///
/// # Errors
/// The error text Python shows in `updates_failed`.
pub fn update_ytdlp_component(
    transport: &mut dyn UpdateTransport,
    current_version: &str,
    components_dir: &Path,
    on_updating: &mut dyn FnMut(),
) -> Result<YtdlpUpdate, String> {
    let release = transport
        .get_json(
            YTDLP_LATEST_RELEASE_API_URL,
            crate::release::UPDATE_METADATA_MAX_BYTES,
        )?
        .as_ref()
        .and_then(Release::from_json)
        .ok_or_else(|| "Could not find a yt-dlp release on GitHub".to_owned())?;
    let latest = release.version();
    if !is_component_version_newer(&latest, current_version) {
        return Ok(YtdlpUpdate::Current);
    }
    let asset = release
        .assets
        .iter()
        .find(|asset| asset.name == YTDLP_ASSET_NAME)
        .ok_or_else(|| "Could not find yt-dlp.exe in the latest yt-dlp release".to_owned())?;
    if asset.browser_download_url.is_empty() {
        return Err("yt-dlp download URL is empty".to_owned());
    }
    validate_trusted_download_url(&asset.browser_download_url, &["github.com"])?;
    on_updating();
    fs::create_dir_all(components_dir).map_err(|error| error.to_string())?;
    let folder = tempfile::Builder::new()
        .prefix("apricotplayer-ytdlp-")
        .tempdir()
        .map_err(|error| error.to_string())?;
    let downloaded = folder.path().join(YTDLP_ASSET_NAME);
    if asset.size.is_some_and(|size| size > YTDLP_MAX_BYTES) {
        return Err("yt-dlp is larger than the safe download limit".to_owned());
    }
    transport.download(
        &asset.browser_download_url,
        true,
        GITHUB_DOWNLOAD_HOSTS,
        &downloaded,
        YTDLP_MAX_BYTES,
        &mut |_, _| {},
    )?;
    let digest = if is_sha256_digest(&asset.digest, true) {
        asset.digest.clone()
    } else {
        published_checksum(transport, &release, folder.path())?
    };
    if let Some(size) = asset.size
        && fs::metadata(&downloaded)
            .map(|metadata| metadata.len())
            .ok()
            != Some(size)
    {
        return Err(
            "downloaded yt-dlp size did not match the GitHub release asset size".to_owned(),
        );
    }
    verify_file_sha256(&downloaded, &digest)?;
    let mut header = [0_u8; 2];
    fs::File::open(&downloaded)
        .and_then(|mut file| file.read_exact(&mut header))
        .map_err(|error| error.to_string())?;
    if &header != b"MZ" {
        return Err("downloaded yt-dlp is not a Windows executable".to_owned());
    }
    install_component(&downloaded, &components_dir.join(YTDLP_ASSET_NAME))?;
    Ok(YtdlpUpdate::Updated(latest))
}

/// The `SHA2-256SUMS` line for `yt-dlp.exe` when the API has no digest.
fn published_checksum(
    transport: &mut dyn UpdateTransport,
    release: &Release,
    folder: &Path,
) -> Result<String, String> {
    let missing = || "GitHub did not publish a valid SHA-256 digest for yt-dlp.exe".to_owned();
    let asset = release
        .assets
        .iter()
        .find(|asset| asset.name == YTDLP_CHECKSUMS_ASSET_NAME)
        .ok_or_else(missing)?;
    validate_trusted_download_url(&asset.browser_download_url, &["github.com"])?;
    let path = folder.join(YTDLP_CHECKSUMS_ASSET_NAME);
    transport.download(
        &asset.browser_download_url,
        true,
        GITHUB_DOWNLOAD_HOSTS,
        &path,
        1024 * 1024,
        &mut |_, _| {},
    )?;
    let text = fs::read_to_string(&path).map_err(|error| error.to_string())?;
    text.lines()
        .filter_map(|line| line.split_once(char::is_whitespace))
        .find(|(_, name)| name.trim().trim_start_matches('*') == YTDLP_ASSET_NAME)
        .map(|(digest, _)| digest.trim().to_owned())
        .filter(|digest| is_sha256_digest(digest, false))
        .ok_or_else(missing)
}

/// Python's rename-aside replacement of the component package: the old copy
/// comes back when the new one cannot be put in place.
fn install_component(source: &Path, target: &Path) -> Result<(), String> {
    let old = target.with_extension("exe.old");
    if old.exists() {
        let _ = fs::remove_file(&old);
    }
    let renamed_old = if target.exists() {
        fs::rename(target, &old).map_err(|error| error.to_string())?;
        true
    } else {
        false
    };
    if let Err(error) = fs::copy(source, target) {
        let _ = fs::remove_file(target);
        if renamed_old && old.exists() {
            let _ = fs::rename(&old, target);
        }
        return Err(error.to_string());
    }
    if old.exists() {
        // A running search may still use the old copy; it goes next time.
        let _ = fs::remove_file(&old);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, fs, path::Path};

    use serde_json::{Value, json};

    use super::{
        AppUpdateCheck, GithubReleaseFeed, UpdateTransport, YtdlpUpdate, check_app_update,
        download_app_update, update_ytdlp_component,
    };
    use crate::{package::file_sha256, release::RUST_BETA_PACKAGE};

    /// A fake GitHub: JSON by address and file bodies by address.
    #[derive(Default)]
    pub(crate) struct FakeGithub {
        pub json: BTreeMap<String, Value>,
        pub files: BTreeMap<String, Vec<u8>>,
        pub requests: Vec<String>,
    }

    impl UpdateTransport for FakeGithub {
        fn get_json(&mut self, url: &str, _limit: u64) -> Result<Option<Value>, String> {
            self.requests.push(url.to_owned());
            Ok(self.json.get(url).cloned())
        }

        fn download(
            &mut self,
            url: &str,
            _octet_stream: bool,
            _allowed_roots: &[&str],
            destination: &Path,
            maximum: u64,
            progress: &mut dyn FnMut(u64, Option<u64>),
        ) -> Result<(), String> {
            self.requests.push(url.to_owned());
            let body = self.files.get(url).ok_or("HTTP Error 404: Not Found")?;
            if body.len() as u64 > maximum {
                return Err("too large".to_owned());
            }
            progress(body.len() as u64, Some(body.len() as u64));
            fs::write(destination, body).map_err(|error| error.to_string())
        }
    }

    fn sha(bytes: &[u8]) -> String {
        let folder = tempfile::tempdir().expect("folder");
        let path = folder.path().join("x");
        fs::write(&path, bytes).expect("write");
        file_sha256(&path).expect("sha")
    }

    #[test]
    fn app_check_reports_up_to_date_skipped_and_available() {
        let mut github = FakeGithub::default();
        github.json.insert(
            crate::release::GITHUB_LATEST_RELEASE_API_URL.to_owned(),
            json!({"tag_name": "v2.0.1", "body": "Fixes", "assets": [{"name": "ApricotPlayer2Beta.zip", "size": 9}]}),
        );
        github.json.insert(
            crate::release::GITHUB_RELEASES_API_URL.to_owned(),
            json!([{"tag_name": "v2.0.1", "body": "Fixes"}, {"tag_name": "v2.0.0", "body": "First"}]),
        );
        let mut check = |current: &str, skipped: &str, manual: bool| {
            let mut feed = GithubReleaseFeed {
                transport: &mut github,
            };
            check_app_update(
                &mut feed,
                &RUST_BETA_PACKAGE,
                "stable",
                current,
                skipped,
                manual,
                false,
                "None.",
            )
        };
        assert_eq!(check("2.0.1", "", false), AppUpdateCheck::UpToDate);
        assert_eq!(
            check("1.9.0", "2.0.1", false),
            AppUpdateCheck::Skipped("2.0.1".to_owned())
        );
        let AppUpdateCheck::Available { release, asset } = check("1.9.0", "2.0.1", true) else {
            panic!("an update");
        };
        assert_eq!(asset.name, "ApricotPlayer2Beta.zip");
        assert_eq!(
            release.cumulative_changelog,
            "What's new in version 2.0.1\n\nFixes\n\nWhat's new in version 2.0.0\n\nFirst"
        );
    }

    #[test]
    fn app_check_without_a_matching_asset_fails_like_python() {
        let mut github = FakeGithub::default();
        github.json.insert(
            crate::release::GITHUB_LATEST_RELEASE_API_URL.to_owned(),
            json!({"tag_name": "v2.0.1", "assets": [{"name": "ApricotPlayerSetup.exe"}]}),
        );
        let mut feed = GithubReleaseFeed {
            transport: &mut github,
        };
        assert_eq!(
            check_app_update(
                &mut feed,
                &RUST_BETA_PACKAGE,
                "stable",
                "2.0.0",
                "",
                true,
                true,
                ""
            ),
            AppUpdateCheck::NoAsset
        );
    }

    #[test]
    fn a_python_release_is_never_offered() {
        let mut github = FakeGithub::default();
        github.json.insert(
            crate::release::GITHUB_LATEST_RELEASE_API_URL.to_owned(),
            json!({"tag_name": "v1.0.22", "assets": [{"name": "ApricotPlayerSetup.exe"}]}),
        );
        let mut feed = GithubReleaseFeed {
            transport: &mut github,
        };
        assert_eq!(
            check_app_update(
                &mut feed,
                &crate::release::STABLE_PACKAGE,
                "stable",
                "1.0.21",
                "",
                true,
                true,
                ""
            ),
            AppUpdateCheck::UpToDate
        );
    }

    #[test]
    fn app_download_verifies_the_package_and_falls_back_to_the_api_url() {
        let mut body = vec![b'M', b'Z'];
        body.resize(1024 * 1024, 0);
        let asset = crate::release::ReleaseAsset {
            name: "ApricotPlayer2BetaSetup.exe".to_owned(),
            size: Some(body.len() as u64),
            digest: format!("sha256:{}", sha(&body)),
            browser_download_url:
                "https://github.com/o/r/releases/download/v2/ApricotPlayer2BetaSetup.exe".to_owned(),
            api_url: "https://api.github.com/repos/o/r/releases/assets/1".to_owned(),
        };
        let mut github = FakeGithub::default();
        github.files.insert(asset.api_url.clone(), body.clone());
        let mut lines = Vec::new();
        let mut percents = Vec::new();
        let downloaded = download_app_update(
            &mut github,
            &RUST_BETA_PACKAGE,
            &asset,
            &mut |line| lines.push(line.to_owned()),
            &mut |percent| percents.push(percent),
        )
        .expect("download");
        assert_eq!(fs::read(&downloaded.path).expect("package"), body);
        assert_eq!(downloaded.sha256, asset.digest["sha256:".len()..]);
        assert_eq!(percents, [Some(100)]);
        assert!(
            lines
                .iter()
                .any(|line| line.starts_with("Download attempt failed from github.com"))
        );
        fs::remove_dir_all(downloaded.path.parent().expect("folder")).expect("cleanup");

        let mut tampered = FakeGithub::default();
        let mut other = body.clone();
        other[5] = 1;
        tampered
            .files
            .insert(asset.browser_download_url.clone(), other);
        assert_eq!(
            download_app_update(
                &mut tampered,
                &RUST_BETA_PACKAGE,
                &asset,
                &mut |_| {},
                &mut |_| {}
            ),
            Err("downloaded file checksum did not match the published SHA-256 digest".to_owned())
        );
        let mut untrusted = asset.clone();
        untrusted.browser_download_url = "https://example.com/x".to_owned();
        assert_eq!(
            download_app_update(
                &mut github,
                &RUST_BETA_PACKAGE,
                &untrusted,
                &mut |_| {},
                &mut |_| {}
            ),
            Err("untrusted download URL: https://example.com/x".to_owned())
        );
    }

    #[test]
    fn ytdlp_update_replaces_the_component_only_when_newer_and_verified() {
        let folder = tempfile::tempdir().expect("folder");
        let components = folder.path().join("components");
        let exe = b"MZ new yt-dlp".to_vec();
        let url = "https://github.com/yt-dlp/yt-dlp/releases/download/2026.09.20/yt-dlp.exe";
        let sums = "https://github.com/yt-dlp/yt-dlp/releases/download/2026.09.20/SHA2-256SUMS";
        let mut github = FakeGithub::default();
        github.json.insert(
            super::YTDLP_LATEST_RELEASE_API_URL.to_owned(),
            json!({"tag_name": "2026.09.20", "assets": [
                {"name": "yt-dlp.exe", "size": exe.len(), "browser_download_url": url},
                {"name": "SHA2-256SUMS", "browser_download_url": sums},
            ]}),
        );
        github.files.insert(url.to_owned(), exe.clone());
        github.files.insert(
            sums.to_owned(),
            format!("{}  yt-dlp\n{}  yt-dlp.exe\n", "0".repeat(64), sha(&exe)).into_bytes(),
        );
        let mut updating = 0;
        assert_eq!(
            update_ytdlp_component(&mut github, "2026.09.20", &components, &mut || updating +=
                1),
            Ok(YtdlpUpdate::Current)
        );
        assert_eq!(updating, 0);
        fs::create_dir_all(&components).expect("components");
        fs::write(components.join("yt-dlp.exe"), b"MZ old").expect("old");
        assert_eq!(
            update_ytdlp_component(&mut github, "2026.08.19", &components, &mut || updating +=
                1),
            Ok(YtdlpUpdate::Updated("2026.09.20".to_owned()))
        );
        assert_eq!(updating, 1);
        assert_eq!(fs::read(components.join("yt-dlp.exe")).expect("new"), exe);
        assert!(!components.join("yt-dlp.exe.old").exists());

        github.files.insert(url.to_owned(), b"MZ tampered".to_vec());
        let result = update_ytdlp_component(&mut github, "2026.08.19", &components, &mut || {});
        assert!(result.is_err());
        assert_eq!(fs::read(components.join("yt-dlp.exe")).expect("kept"), exe);
    }
}
