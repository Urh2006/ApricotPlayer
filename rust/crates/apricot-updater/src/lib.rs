//! App and component update policy. Local Rust betas explicitly disable remote
//! publication and installation.

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
