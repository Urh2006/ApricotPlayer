//! Requests delivered by startup arguments, file associations, or a second launch.

use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ActivationRequest {
    Show,
    OpenFile(PathBuf),
    OpenSettings,
}
