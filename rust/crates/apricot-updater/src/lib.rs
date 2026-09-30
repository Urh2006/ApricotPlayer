//! App and component update policy. Local Rust betas explicitly disable remote
//! publication and installation.

pub mod flow;
pub mod package;
pub mod release;
pub mod script;
pub mod version;

pub use flow::{
    AppUpdateCheck, DownloadedUpdate, GithubReleaseFeed, UpdateTransport, YtdlpUpdate,
    check_app_update, download_app_update, update_ytdlp_component,
};
pub use release::{
    APP_PACKAGE, PackageNames, RUST_BETA_PACKAGE, Release, ReleaseAsset, STABLE_PACKAGE,
    release_changelog_text,
};
pub use version::{is_component_version_newer, is_newer_version, parse_version};

/// This build's channel: a local Rust beta is `LocalOnly` (D-011); the
/// distributed 2.0 beta is built with the `release-beta` feature.
pub const BUILD_CHANNEL: UpdateChannel = if cfg!(feature = "release-beta") {
    UpdateChannel::Beta
} else {
    UpdateChannel::LocalOnly
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UpdateChannel {
    Stable,
    Beta,
    LocalOnly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum YoutubeComponent {
    YtDlp,
    RustyYtdl,
}

impl YoutubeComponent {
    pub const ALL: [Self; 2] = [Self::YtDlp, Self::RustyYtdl];
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct YoutubeComponentCheck<T, E> {
    pub component: YoutubeComponent,
    pub result: Result<T, E>,
}

/// Checks every owned `YouTube` component even when an earlier check fails.
pub fn check_youtube_components<T, E>(
    mut check: impl FnMut(YoutubeComponent) -> Result<T, E>,
) -> Vec<YoutubeComponentCheck<T, E>> {
    YoutubeComponent::ALL
        .into_iter()
        .map(|component| YoutubeComponentCheck {
            component,
            result: check(component),
        })
        .collect()
}

impl UpdateChannel {
    pub const fn allows_remote_install(self) -> bool {
        !matches!(self, Self::LocalOnly)
    }
}

#[cfg(test)]
mod tests {
    use super::{UpdateChannel, YoutubeComponent, check_youtube_components};

    #[test]
    fn local_rust_betas_cannot_install_remote_updates() {
        assert!(!UpdateChannel::LocalOnly.allows_remote_install());
    }

    #[test]
    fn one_youtube_component_failure_does_not_skip_the_other() {
        let mut checked = Vec::new();
        let results = check_youtube_components(|component| {
            checked.push(component);
            if component == YoutubeComponent::YtDlp {
                Err("yt-dlp unavailable")
            } else {
                Ok("current")
            }
        });
        assert_eq!(checked, YoutubeComponent::ALL);
        assert!(results[0].result.is_err());
        assert_eq!(results[1].result, Ok("current"));
    }
}
