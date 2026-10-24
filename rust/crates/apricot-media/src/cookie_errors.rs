//! Error classifiers Python uses to decide when yt-dlp needs sign-in cookies.

/// Python `is_cookie_auth_error`.
#[must_use]
pub fn is_cookie_auth_error(text: &str) -> bool {
    const CHECKS: [&str; 13] = [
        "sign in to confirm",
        "not a bot",
        "confirm you're not a bot",
        "confirm you are not a bot",
        "cookies-from-browser",
        "failed to load cookies",
        "could not copy chrome cookie database",
        "no youtube login cookies",
        "cookies were exported, but no youtube login cookies",
        "failed to decrypt with dpapi",
        "object has no attribute 'decode'",
        "login required",
        "this video may be inappropriate",
    ];
    let lowered = text.to_lowercase();
    CHECKS.iter().any(|check| lowered.contains(check))
}

/// Python `is_age_or_js_playback_error`.
#[must_use]
pub fn is_age_or_js_playback_error(text: &str) -> bool {
    const CHECKS: [&str; 11] = [
        "requested format is not available",
        "no video formats found",
        "video unavailable",
        "this video is unavailable",
        "nsig extraction failed",
        "signature extraction failed",
        "n challenge",
        "age restricted",
        "age-restricted",
        "this video may be inappropriate",
        "only available to registered users",
    ];
    let lowered = text.to_lowercase();
    CHECKS.iter().any(|check| lowered.contains(check))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookie_auth_errors_match_python_phrases() {
        assert!(is_cookie_auth_error(
            "ERROR: [youtube] x: Sign in to confirm you're not a bot"
        ));
        assert!(is_cookie_auth_error("Login required"));
        assert!(is_cookie_auth_error(
            "Could not copy Chrome cookie database. See ..."
        ));
        assert!(!is_cookie_auth_error("HTTP Error 404: Not Found"));
    }

    #[test]
    fn age_and_js_errors_match_python_phrases() {
        assert!(is_age_or_js_playback_error(
            "Requested format is not available"
        ));
        assert!(is_age_or_js_playback_error("This video is age-restricted"));
        assert!(!is_age_or_js_playback_error("Sign in to confirm"));
    }
}
