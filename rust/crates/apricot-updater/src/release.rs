//! GitHub release metadata: Python `fetch_latest_release`,
//! `fetch_public_releases`, `cumulative_changelog_text`,
//! `release_changelog_text` and `find_release_asset`.

use serde_json::Value;

use crate::version::{is_newer_version, parse_version};

/// The Python repository whose releases the app follows.
pub const GITHUB_RELEASES_API_URL: &str =
    "https://api.github.com/repos/Urh2006/ApricotPlayer/releases";
pub const GITHUB_LATEST_RELEASE_API_URL: &str =
    "https://api.github.com/repos/Urh2006/ApricotPlayer/releases/latest";

/// Python `UPDATE_METADATA_MAX_BYTES`.
pub const UPDATE_METADATA_MAX_BYTES: u64 = 4_000_000;

/// One GitHub release asset.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReleaseAsset {
    pub name: String,
    /// Python keeps a size only when it is a positive integer.
    pub size: Option<u64>,
    pub digest: String,
    pub browser_download_url: String,
    pub api_url: String,
}

/// One GitHub release as Python reads it from the JSON dictionary.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Release {
    pub id: Option<i64>,
    pub tag_name: String,
    pub name: String,
    pub body: String,
    pub draft: bool,
    pub prerelease: bool,
    pub assets: Vec<ReleaseAsset>,
    /// Python `release["_cumulative_changelog"]`.
    pub cumulative_changelog: String,
}

impl Release {
    /// Reads a release dictionary. Anything that is not an object is `None`,
    /// like Python's `isinstance(release, dict)` checks.
    #[must_use]
    pub fn from_json(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        let text = |key: &str| {
            object
                .get(key)
                .map(|value| match value {
                    Value::String(text) => text.clone(),
                    Value::Null => String::new(),
                    other => other.to_string(),
                })
                .unwrap_or_default()
        };
        let truthy = |key: &str| object.get(key).is_some_and(python_truthy);
        let assets = object
            .get("assets")
            .and_then(Value::as_array)
            .map(|assets| assets.iter().filter_map(ReleaseAsset::from_json).collect())
            .unwrap_or_default();
        Some(Self {
            id: object.get("id").and_then(Value::as_i64),
            tag_name: text("tag_name"),
            name: text("name"),
            body: text("body"),
            draft: truthy("draft"),
            prerelease: truthy("prerelease"),
            assets,
            cumulative_changelog: String::new(),
        })
    }

    /// Python `release_version`.
    #[must_use]
    pub fn version(&self) -> String {
        let value = if self.tag_name.is_empty() {
            &self.name
        } else {
            &self.tag_name
        };
        value.trim().trim_start_matches('v').to_owned()
    }
}

impl ReleaseAsset {
    fn from_json(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        let text = |key: &str| {
            object
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        Some(Self {
            name: text("name"),
            size: object
                .get("size")
                .and_then(Value::as_u64)
                .filter(|size| *size > 0),
            digest: text("digest"),
            browser_download_url: text("browser_download_url"),
            api_url: text("url"),
        })
    }
}

fn python_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(number) => number.as_f64().is_some_and(|number| number != 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(items) => !items.is_empty(),
    }
}

/// Where release metadata comes from.
pub trait ReleaseFeed {
    /// `GET /releases/latest`. A 404 is `Ok(None)`.
    ///
    /// # Errors
    /// Transport, trust or size errors.
    fn latest_release(&mut self) -> Result<Option<Value>, String>;

    /// `GET /releases`.
    ///
    /// # Errors
    /// Transport, trust or size errors.
    fn releases(&mut self) -> Result<Value, String>;
}

/// Python `normalized_update_channel_value`: anything else is `stable`, the
/// default of a stable build.
#[must_use]
pub fn normalized_update_channel(value: &str) -> &'static str {
    if value.trim().eq_ignore_ascii_case("beta") {
        "beta"
    } else {
        "stable"
    }
}

/// Python `fetch_github_latest_release`.
///
/// # Errors
/// Any error but a 404.
pub fn fetch_github_latest_release(feed: &mut dyn ReleaseFeed) -> Result<Option<Release>, String> {
    Ok(feed
        .latest_release()?
        .as_ref()
        .and_then(Release::from_json)
        .filter(|release| !release.draft))
}

/// Python `fetch_public_releases`: newest version first.
///
/// # Errors
/// Errors of the release list request.
pub fn fetch_public_releases(
    feed: &mut dyn ReleaseFeed,
    channel: &str,
) -> Result<Vec<Release>, String> {
    let stable = normalized_update_channel(channel) == "stable";
    let payload = feed.releases()?;
    let mut releases: Vec<Release> = payload
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(Release::from_json)
                .filter(|release| !(release.draft || stable && release.prerelease))
                .collect()
        })
        .unwrap_or_default();
    if let Ok(Some(latest)) = fetch_github_latest_release(feed)
        && !(stable && latest.prerelease)
        && !releases
            .iter()
            .any(|release| release.id == latest.id || release.tag_name == latest.tag_name)
    {
        releases.push(latest);
    }
    releases.sort_by_key(|release| std::cmp::Reverse(parse_version(&release.version())));
    Ok(releases)
}

/// Python `fetch_latest_release`: every error means no release.
pub fn fetch_latest_release(feed: &mut dyn ReleaseFeed, channel: &str) -> Option<Release> {
    if normalized_update_channel(channel) == "beta" {
        return fetch_public_releases(feed, channel)
            .ok()
            .and_then(|releases| releases.into_iter().next());
    }
    if let Ok(Some(release)) = fetch_github_latest_release(feed)
        && !release.prerelease
    {
        return Some(release);
    }
    fetch_public_releases(feed, channel)
        .ok()
        .and_then(|releases| releases.into_iter().next())
}

/// Python `cumulative_changelog_text`.
///
/// # Errors
/// Errors of the release list request.
pub fn cumulative_changelog_text(
    feed: &mut dyn ReleaseFeed,
    channel: &str,
    current_version: &str,
    latest_version: &str,
    no_changelog: &str,
) -> Result<String, String> {
    let mut sections = Vec::new();
    for release in fetch_public_releases(feed, channel)? {
        let version = release.version();
        if version.is_empty() {
            continue;
        }
        if is_newer_version(&version, current_version)
            && !is_newer_version(&version, latest_version)
        {
            let body = release.body.replace("\r\n", "\n");
            let body = body.trim();
            let body = if body.is_empty() { no_changelog } else { body };
            if starts_with_whats_new_heading(body) {
                sections.push(body.to_owned());
            } else {
                sections.push(format!("What's new in version {version}\n\n{body}"));
            }
        }
    }
    let text = sections.join("\n\n");
    Ok(truncated(text.trim(), 12_000))
}

/// Python `re.match(r"^#*\s*what'?s new in version", body, re.IGNORECASE)`.
fn starts_with_whats_new_heading(body: &str) -> bool {
    let rest = body.trim_start_matches('#').trim_start();
    let lower = rest.to_lowercase();
    lower.starts_with("what's new in version") || lower.starts_with("whats new in version")
}

fn truncated(text: &str, limit: usize) -> String {
    match text.char_indices().nth(limit) {
        Some((index, _)) => format!("{}\n\n...", text[..index].trim_end()),
        None => text.to_owned(),
    }
}

/// Python `release_changelog_text`.
#[must_use]
pub fn release_changelog_text(release: &Release, no_changelog: &str) -> String {
    let cumulative = release.cumulative_changelog.trim();
    if !cumulative.is_empty() {
        return cumulative.to_owned();
    }
    let body = release.body.replace("\r\n", "\n");
    let body = body.trim();
    if body.is_empty() {
        return no_changelog.to_owned();
    }
    truncated(body, 6_000)
}

/// Release asset names of one build flavour.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PackageNames {
    /// Python `INSTALLER_ASSET_NAME`.
    pub installer: &'static str,
    /// Python `PORTABLE_ZIP_ASSET_NAME` and `LEGACY_PORTABLE_ZIP_ASSET_NAME`.
    pub portable: &'static [&'static str],
    /// The folder at the root of the portable zip.
    pub portable_root: &'static str,
    /// The application executable inside the install folder.
    pub executable: &'static str,
    /// The display name of the installed program in the uninstall registry.
    pub display_name: &'static str,
}

/// The local Rust beta. Its assets are never published before 2.0 (D-011).
pub const RUST_BETA_PACKAGE: PackageNames = PackageNames {
    installer: "ApricotPlayer2BetaSetup.exe",
    portable: &["ApricotPlayer2Beta.zip"],
    portable_root: "ApricotPlayer2Beta",
    executable: "ApricotPlayer2Beta.exe",
    display_name: "ApricotPlayer 2 Beta",
};

impl PackageNames {
    /// Python `find_release_asset`.
    #[must_use]
    pub fn find_asset<'a>(
        &self,
        release: &'a Release,
        installed_build: bool,
    ) -> Option<&'a ReleaseAsset> {
        let mut preferred: Vec<&str> = Vec::new();
        if installed_build {
            preferred.push(self.installer);
            preferred.extend(self.portable);
        } else {
            preferred.extend(self.portable);
            preferred.push(self.installer);
        }
        preferred
            .into_iter()
            .find_map(|name| release.assets.iter().find(|asset| asset.name == name))
    }

    /// Python `safe_asset_filename`.
    ///
    /// # Errors
    /// An unexpected asset name.
    pub fn safe_asset_filename(&self, asset: &ReleaseAsset) -> Result<String, String> {
        let name = asset
            .name
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or_default()
            .to_owned();
        if name == self.installer || self.portable.contains(&name.as_str()) {
            Ok(name)
        } else {
            let shown = if name.is_empty() { "missing" } else { &name };
            Err(format!("unexpected update asset name: {shown}"))
        }
    }

    /// Python `is_installer_asset`.
    #[must_use]
    pub fn is_installer_asset(&self, name: &str) -> bool {
        let name = file_name(name).to_lowercase();
        name == self.installer.to_lowercase()
            || name.contains("setup")
            || name.contains("installer")
    }

    /// Python `is_portable_zip_asset`.
    #[must_use]
    pub fn is_portable_zip_asset(&self, name: &str) -> bool {
        let name = file_name(name).to_lowercase();
        self.portable
            .iter()
            .any(|portable| portable.to_lowercase() == name)
            || (name
                .rsplit_once('.')
                .is_some_and(|(_, extension)| extension == "zip")
                && name.contains(&self.portable_root.to_lowercase()))
    }
}

fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{
        RUST_BETA_PACKAGE, Release, ReleaseFeed, cumulative_changelog_text, fetch_latest_release,
        fetch_public_releases, release_changelog_text,
    };

    struct Feed {
        latest: Result<Option<Value>, String>,
        releases: Result<Value, String>,
    }

    impl ReleaseFeed for Feed {
        fn latest_release(&mut self) -> Result<Option<Value>, String> {
            self.latest.clone()
        }

        fn releases(&mut self) -> Result<Value, String> {
            self.releases.clone()
        }
    }

    fn release(tag: &str, prerelease: bool) -> Value {
        let id: u64 = tag.bytes().map(u64::from).sum();
        json!({"id": id, "tag_name": tag, "prerelease": prerelease, "body": format!("Notes {tag}")})
    }

    #[test]
    fn stable_channel_uses_latest_and_beta_uses_the_newest_listed() {
        let mut feed = Feed {
            latest: Ok(Some(release("v1.0.22", false))),
            releases: Ok(json!([
                release("v1.0.21", false),
                release("v1.1.0-beta.1", true),
                {"tag_name": "v9.0.0", "draft": true},
                "not a release",
            ])),
        };
        let stable = fetch_latest_release(&mut feed, "stable").expect("stable");
        assert_eq!(stable.version(), "1.0.22");
        let beta = fetch_latest_release(&mut feed, "beta").expect("beta");
        assert_eq!(beta.version(), "1.1.0-beta.1");
        let listed: Vec<_> = fetch_public_releases(&mut feed, "stable")
            .expect("list")
            .iter()
            .map(Release::version)
            .collect();
        assert_eq!(listed, ["1.0.22", "1.0.21"]);
    }

    #[test]
    fn stable_channel_falls_back_to_the_list_and_errors_mean_no_release() {
        let mut feed = Feed {
            latest: Ok(None),
            releases: Ok(json!([
                release("v1.0.20", false),
                release("v1.0.21", false)
            ])),
        };
        assert_eq!(
            fetch_latest_release(&mut feed, "").map(|release| release.version()),
            Some("1.0.21".to_owned())
        );
        let mut broken = Feed {
            latest: Err("offline".to_owned()),
            releases: Err("offline".to_owned()),
        };
        assert_eq!(fetch_latest_release(&mut broken, "stable"), None);
        assert_eq!(fetch_latest_release(&mut broken, "beta"), None);
    }

    #[test]
    fn changelogs_match_python() {
        let mut feed = Feed {
            latest: Ok(None),
            releases: Ok(json!([
                {"tag_name": "v1.0.23", "body": "## What's new in version 1.0.23\r\n\r\nC"},
                {"tag_name": "v1.0.22", "body": ""},
                {"tag_name": "v1.0.21", "body": "Old"},
                {"tag_name": "v1.0.24", "body": "Future"},
            ])),
        };
        let text = cumulative_changelog_text(&mut feed, "stable", "1.0.21", "1.0.23", "None.")
            .expect("changelog");
        assert_eq!(
            text,
            "## What's new in version 1.0.23\n\nC\n\nWhat's new in version 1.0.22\n\nNone."
        );
        let mut release = Release {
            body: "x".repeat(6_001),
            ..Release::default()
        };
        let body = release_changelog_text(&release, "None.");
        assert_eq!(body.chars().count(), 6_005);
        assert!(body.ends_with("x\n\n..."));
        release.body = String::new();
        assert_eq!(release_changelog_text(&release, "None."), "None.");
        release.cumulative_changelog = "All".to_owned();
        assert_eq!(release_changelog_text(&release, "None."), "All");
    }

    #[test]
    fn assets_follow_the_build_kind() {
        let release = Release::from_json(&json!({
            "tag_name": "v2.0.0",
            "assets": [
                {"name": "ApricotPlayer2Beta.zip", "size": 5, "digest": "sha256:ab"},
                {"name": "ApricotPlayer2BetaSetup.exe", "size": 0},
            ]
        }))
        .expect("release");
        let portable = RUST_BETA_PACKAGE.find_asset(&release, false).expect("zip");
        assert_eq!(portable.name, "ApricotPlayer2Beta.zip");
        assert_eq!(portable.size, Some(5));
        let installer = RUST_BETA_PACKAGE.find_asset(&release, true).expect("setup");
        assert_eq!(installer.size, None);
        assert!(RUST_BETA_PACKAGE.safe_asset_filename(portable).is_ok());
        let mut other = portable.clone();
        other.name = "../evil.exe".to_owned();
        assert_eq!(
            RUST_BETA_PACKAGE.safe_asset_filename(&other),
            Err("unexpected update asset name: evil.exe".to_owned())
        );
        assert!(RUST_BETA_PACKAGE.is_installer_asset("C:\\t\\ApricotPlayer2BetaSetup.exe"));
        assert!(RUST_BETA_PACKAGE.is_portable_zip_asset("x/ApricotPlayer2Beta.zip"));
        assert!(!RUST_BETA_PACKAGE.is_portable_zip_asset("other.zip"));
    }
}
