//! Deterministic adapters used by contract and differential tests.

use std::{path::Path, sync::Mutex};

use apricot_platform::{PlatformError, PlatformPaths, PlatformServices};

#[derive(Debug)]
pub struct FakePlatform {
    pub paths: PlatformPaths,
    pub announcements: Mutex<Vec<String>>,
}

impl PlatformServices for FakePlatform {
    fn paths(&self) -> &PlatformPaths {
        &self.paths
    }

    fn open_path(&self, _path: &Path) -> Result<(), PlatformError> {
        Ok(())
    }

    fn open_url(&self, _url: &str) -> Result<(), PlatformError> {
        Ok(())
    }

    fn copy_text(&self, _text: &str) -> Result<(), PlatformError> {
        Ok(())
    }

    fn announce(&self, text: &str) -> Result<(), PlatformError> {
        self.announcements
            .lock()
            .expect("announcement mutex")
            .push(text.to_owned());
        Ok(())
    }
}
