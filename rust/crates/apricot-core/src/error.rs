//! Stable error classification for localized UI recovery.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorDomain {
    Settings,
    Storage,
    Network,
    Extraction,
    Playback,
    Download,
    Conversion,
    Accessibility,
    Update,
    Podcast,
    Audiovault,
    Internal,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryAction {
    None,
    Retry,
    OpenSettings,
    ChooseFile,
    SignIn,
    RestartApplication,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppError {
    pub code: &'static str,
    pub message_key: &'static str,
    pub domain: ErrorDomain,
    pub recovery: RecoveryAction,
    pub technical_detail: String,
}

impl AppError {
    pub fn new(
        code: &'static str,
        message_key: &'static str,
        domain: ErrorDomain,
        recovery: RecoveryAction,
        technical_detail: impl Into<String>,
    ) -> Self {
        Self {
            code,
            message_key,
            domain,
            recovery,
            technical_detail: technical_detail.into(),
        }
    }

    pub fn is_retryable(&self) -> bool {
        self.recovery == RecoveryAction::Retry
    }
}

#[cfg(test)]
mod tests {
    use super::{AppError, ErrorDomain, RecoveryAction};

    #[test]
    fn user_recovery_is_separate_from_technical_detail() {
        let error = AppError::new(
            "youtube.rate_limited",
            "search_failed",
            ErrorDomain::Network,
            RecoveryAction::Retry,
            "HTTP 429 from extractor",
        );
        assert!(error.is_retryable());
        assert_eq!(error.message_key, "search_failed");
        assert_eq!(error.technical_detail, "HTTP 429 from extractor");
    }
}
