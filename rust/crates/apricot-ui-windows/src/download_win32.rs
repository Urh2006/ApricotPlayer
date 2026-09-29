//! Background download worker used by the native Windows shell.

use std::{
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool, mpsc::SyncSender},
    thread,
};

use apricot_platform::{
    DownloadError, DownloadEvent, DownloadRequest, DownloadSummary, YtDlpDownloader,
};

#[derive(Debug)]
pub enum DownloadWorkerUpdate {
    Event {
        task_id: u64,
        event: DownloadEvent,
    },
    Finished {
        task_id: u64,
        result: Result<DownloadSummary, String>,
        output_directory: PathBuf,
    },
}

pub fn spawn_download(
    task_id: u64,
    executable: PathBuf,
    request: DownloadRequest,
    cancelled: Arc<AtomicBool>,
    sender: SyncSender<DownloadWorkerUpdate>,
) {
    thread::spawn(move || {
        let output_directory = request.output_directory.clone();
        let result = YtDlpDownloader::new(&executable)
            .and_then(|downloader| {
                download_with_cookie_recovery(&downloader, &request, &cancelled, |event| {
                    let _ = sender.send(DownloadWorkerUpdate::Event { task_id, event });
                })
            })
            .map_err(|error| error.to_string());
        let _ = sender.send(DownloadWorkerUpdate::Finished {
            task_id,
            result,
            output_directory,
        });
    });
}

pub fn spawn_batch_download(
    task_id: u64,
    executable: PathBuf,
    requests: Vec<DownloadRequest>,
    completion_directory: PathBuf,
    cancelled: Arc<AtomicBool>,
    sender: SyncSender<DownloadWorkerUpdate>,
) {
    thread::spawn(move || {
        let result = YtDlpDownloader::new(&executable).and_then(|downloader| {
            let mut summary = DownloadSummary::default();
            for request in &requests {
                if cancelled.load(std::sync::atomic::Ordering::Acquire) {
                    return Err(DownloadError::Cancelled);
                }
                match download_with_cookie_recovery(&downloader, request, &cancelled, |event| {
                    let _ = sender.send(DownloadWorkerUpdate::Event { task_id, event });
                }) {
                    Ok(item_summary) => {
                        summary.files.extend(item_summary.files);
                        summary.item_failures.extend(item_summary.item_failures);
                    }
                    Err(DownloadError::Cancelled) => return Err(DownloadError::Cancelled),
                    Err(error) => {
                        let message = format!("{}: {error}", request.title);
                        summary.item_failures.push(message.clone());
                        let _ = sender.send(DownloadWorkerUpdate::Event {
                            task_id,
                            event: DownloadEvent::ItemFailed { message },
                        });
                    }
                }
            }
            Ok(summary)
        });
        let _ = sender.send(DownloadWorkerUpdate::Finished {
            task_id,
            result: result.map_err(|error| error.to_string()),
            output_directory: completion_directory,
        });
    });
}

fn download_with_cookie_recovery<F>(
    downloader: &YtDlpDownloader,
    request: &DownloadRequest,
    cancelled: &Arc<AtomicBool>,
    mut emit: F,
) -> Result<DownloadSummary, DownloadError>
where
    F: FnMut(DownloadEvent),
{
    let cookies_file = request.options.cookies_file.clone();
    let mut without_cookies = request.clone();
    without_cookies.options.cookies_file = None;
    let mut deferred_events = Vec::new();
    let mut first_attempt_started_download = false;
    match downloader.download(&without_cookies, cancelled, |event| match event {
        DownloadEvent::Progress { .. } => {
            first_attempt_started_download = true;
            emit(event);
        }
        DownloadEvent::FileFinished { .. } => {
            first_attempt_started_download = true;
            deferred_events.push(event);
        }
        DownloadEvent::ItemFailed { .. } => deferred_events.push(event),
    }) {
        Ok(summary) => {
            for event in deferred_events {
                emit(event);
            }
            Ok(summary)
        }
        Err(error)
            if should_retry_with_cookies(&error.to_string(), first_attempt_started_download) =>
        {
            let mut retry_error = error;
            if let Some(cookies_file) = cookies_file {
                let mut with_cookies = request.clone();
                with_cookies.options.cookies_file = Some(cookies_file);
                let mut started = false;
                match downloader.download(&with_cookies, cancelled, |event| {
                    if matches!(
                        event,
                        DownloadEvent::Progress { .. } | DownloadEvent::FileFinished { .. }
                    ) {
                        started = true;
                    }
                    emit(event);
                }) {
                    Ok(summary) => return Ok(summary),
                    Err(error) if started => return Err(error),
                    Err(error) => retry_error = error,
                }
            }
            // Python `ydl_download_urls`: a sign-in error that cookies did
            // not fix refreshes them from the browser once.
            match apricot_platform::browser_cookies::repair_cookies_for_error(
                &retry_error.to_string(),
            ) {
                Some(repaired) => {
                    let mut with_cookies = request.clone();
                    with_cookies.options.cookies_file = Some(repaired);
                    downloader.download(&with_cookies, cancelled, emit)
                }
                None => Err(retry_error),
            }
        }
        Err(error) => {
            for event in deferred_events {
                emit(event);
            }
            Err(error)
        }
    }
}

fn cookie_retry_is_relevant(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    [
        "sign in to confirm",
        "not a bot",
        "confirm you're not a bot",
        "confirm you are not a bot",
        "login required",
        "this video may be inappropriate",
        "age-restricted",
    ]
    .iter()
    .any(|needle| message.contains(needle))
}

fn should_retry_with_cookies(message: &str, download_started: bool) -> bool {
    !download_started && cookie_retry_is_relevant(message)
}

#[cfg(test)]
mod tests {
    use super::{cookie_retry_is_relevant, should_retry_with_cookies};

    #[test]
    fn cookies_are_only_retried_for_authentication_failures() {
        assert!(cookie_retry_is_relevant("Sign in to confirm your age"));
        assert!(cookie_retry_is_relevant("This video is age-restricted"));
        assert!(!cookie_retry_is_relevant(
            "Requested format is not available"
        ));
        assert!(!cookie_retry_is_relevant("Network timeout"));
    }

    #[test]
    fn a_started_download_must_not_be_restarted_with_cookies() {
        let authentication_error = "Sign in to confirm your age";
        assert!(should_retry_with_cookies(authentication_error, false));
        assert!(!should_retry_with_cookies(authentication_error, true));
    }
}
