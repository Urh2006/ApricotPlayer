//! Windows side of Python's cookie workflow: the yt-dlp and browser backend,
//! the export worker and the automatic repair used by yt-dlp workers.

#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

use apricot_app::cookies::{
    BrowserCookieBackend, browser_title, export_browser_cookies, save_cookie_jar_to_cache,
};
use apricot_platform::browser_cookies::{
    CookieRepairHook, close_cookie_browser_processes, cookie_browser_is_running,
    devtools_browser_cookies, discover_cookie_profiles, extract_browser_cookies_with_ytdlp,
    set_cookie_repair_hook, wait_for_cookie_browser_exit,
};
use serde_json::Value;
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    UI::WindowsAndMessaging::{PostMessageW, WM_APP},
};

/// Carries a boxed [`CookieEvent`] from a worker to the main window.
pub(crate) const WM_COOKIE_EVENT: u32 = WM_APP + 6;
/// Python suppresses automatic refreshes for five minutes after a failure.
const REPAIR_SUPPRESSION: Duration = Duration::from_secs(300);

/// Who asked for a browser export.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CookieExportOrigin {
    /// Settings, Export browser cookies.
    Settings,
    /// Python `repair_cookies_for_error` in a yt-dlp worker.
    Repair,
    /// Python `prompt_cookie_refresh_for_playback`, for one resolve token.
    Playback(u64),
}

pub(crate) enum CookieEvent {
    /// Python `ui_queue.put(("announce", text))`.
    Announce(String),
    /// The cache holds the browser's cookies; the settings still need them.
    Exported {
        origin: CookieExportOrigin,
        browser: String,
        profile_label: String,
    },
    Failed {
        origin: CookieExportOrigin,
        error: String,
    },
}

/// Posts `event` to the main window; it is dropped when the window is gone.
pub(crate) fn post_event(window: isize, event: CookieEvent) {
    let pointer = Box::into_raw(Box::new(event));
    // SAFETY: The main window takes ownership of the pointer in its
    // WM_COOKIE_EVENT handler; a failed post releases it here.
    unsafe {
        if PostMessageW(
            Some(HWND(window as *mut _)),
            WM_COOKIE_EVENT,
            WPARAM(0),
            LPARAM(pointer as isize),
        )
        .is_err()
        {
            drop(Box::from_raw(pointer));
        }
    }
}

/// Takes the event out of a `WM_COOKIE_EVENT` message.
///
/// # Safety
/// `lparam` must come from [`post_event`] and be taken exactly once.
pub(crate) unsafe fn take_event(lparam: LPARAM) -> Option<CookieEvent> {
    let pointer = lparam.0 as *mut CookieEvent;
    (!pointer.is_null()).then(|| *Box::from_raw(pointer))
}

/// The bundled `components\yt-dlp.exe`.
pub(crate) fn ytdlp_executable() -> Option<PathBuf> {
    std::env::current_exe().ok().and_then(|path| {
        path.parent().map(|folder| {
            apricot_platform::app_update::preferred_ytdlp_executable(&folder.join("components"))
        })
    })
}

/// Python `get_yt_dlp() is not None`.
pub(crate) fn ytdlp_available() -> bool {
    ytdlp_executable().is_some_and(|path| path.is_file())
}

pub(crate) struct PlatformCookieBackend {
    ytdlp: Option<PathBuf>,
}

impl PlatformCookieBackend {
    pub(crate) fn new() -> Self {
        Self {
            ytdlp: ytdlp_executable().filter(|path| path.is_file()),
        }
    }
}

impl BrowserCookieBackend for PlatformCookieBackend {
    fn ytdlp_available(&self) -> bool {
        self.ytdlp.is_some()
    }

    fn discover_profiles(&self, browser: &str) -> Vec<(String, String)> {
        discover_cookie_profiles(browser)
    }

    fn is_running(&self, browser: &str) -> bool {
        cookie_browser_is_running(browser)
    }

    fn close(&self, browser: &str) -> bool {
        close_cookie_browser_processes(browser)
    }

    fn wait_for_exit(&self, browser: &str, timeout: Duration) -> bool {
        wait_for_cookie_browser_exit(browser, timeout)
    }

    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }

    fn extract(&self, browser: &str, profile: Option<&str>) -> Result<String, String> {
        let ytdlp = self
            .ytdlp
            .as_deref()
            .ok_or_else(|| "yt-dlp is missing".to_owned())?;
        extract_browser_cookies_with_ytdlp(ytdlp, browser, profile)
    }

    fn devtools_cookies(
        &self,
        browser: &str,
        profile: Option<&str>,
        headless: bool,
    ) -> Result<(String, Vec<Value>), String> {
        devtools_browser_cookies(browser, profile, headless)
    }
}

/// Runs Python `export_browser_cookies_blocking(allow_close=True)` and saves
/// the cache; the settings are updated by the main window.
fn export_to_cache(
    language: &str,
    browser: &str,
    profile: &str,
    cache: &Path,
) -> Result<String, String> {
    let catalog = apricot_app::embedded_catalog(language);
    let export = export_browser_cookies(
        &PlatformCookieBackend::new(),
        &catalog,
        browser,
        profile,
        true,
    )?;
    save_cookie_jar_to_cache(&export.jar, cache).map_err(|error| error.to_string())?;
    Ok(export.profile_label)
}

/// Starts a browser export on a worker for Settings or a playback retry.
pub(crate) fn spawn_export(
    main_window: HWND,
    origin: CookieExportOrigin,
    language: String,
    browser: String,
    profile: String,
    cache: PathBuf,
) {
    let window = main_window.0 as isize;
    std::thread::spawn(move || {
        let event = match export_to_cache(&language, &browser, &profile, &cache) {
            Ok(profile_label) => CookieEvent::Exported {
                origin,
                browser,
                profile_label,
            },
            Err(error) => CookieEvent::Failed { origin, error },
        };
        post_event(window, event);
    });
}

#[derive(Clone, Default)]
struct RepairSettings {
    language: String,
    browser: String,
    profile: String,
}

/// Python `repair_cookies_for_error` for yt-dlp workers.
pub(crate) struct CookieRepairer {
    window: isize,
    cache: PathBuf,
    settings: Mutex<RepairSettings>,
    suppressed_until: Mutex<Option<Instant>>,
    /// Python `cookie_repair_lock`.
    running: Mutex<()>,
}

static REPAIRER: OnceLock<Arc<CookieRepairer>> = OnceLock::new();

/// Installs the repairer for the main window once.
pub(crate) fn install_repairer(main_window: HWND, cache: PathBuf) {
    let repairer = Arc::new(CookieRepairer {
        window: main_window.0 as isize,
        cache,
        settings: Mutex::new(RepairSettings::default()),
        suppressed_until: Mutex::new(None),
        running: Mutex::new(()),
    });
    if REPAIRER.set(Arc::clone(&repairer)).is_ok() {
        set_cookie_repair_hook(repairer);
    }
}

/// Keeps the repairer's browser, profile and language current.
pub(crate) fn update_repair_settings(settings: &apricot_storage::SettingsDocument) {
    if let Some(repairer) = REPAIRER.get()
        && let Ok(mut current) = repairer.settings.lock()
    {
        *current = RepairSettings {
            language: settings.language.clone(),
            browser: apricot_app::cookies::normalized_cookies_browser(settings),
            profile: settings.cookies_browser_profile.clone(),
        };
    }
}

/// Python `cookie_repair_suppressed_until = 0.0`.
pub(crate) fn reset_repair_suppression() {
    if let Some(repairer) = REPAIRER.get()
        && let Ok(mut suppressed) = repairer.suppressed_until.lock()
    {
        *suppressed = None;
    }
}

impl CookieRepairHook for CookieRepairer {
    fn repair(&self, _error: &str) -> Option<PathBuf> {
        let settings = self.settings.lock().ok()?.clone();
        if settings.browser.is_empty() {
            return None;
        }
        if self
            .suppressed_until
            .lock()
            .ok()?
            .is_some_and(|until| Instant::now() < until)
        {
            return None;
        }
        let Ok(_guard) = self.running.try_lock() else {
            // Another worker is refreshing; use its result.
            let _guard = self.running.lock().ok()?;
            let ready = std::fs::metadata(&self.cache).is_ok_and(|metadata| metadata.len() > 0);
            return ready.then(|| self.cache.clone());
        };
        let catalog = apricot_app::embedded_catalog(&settings.language);
        post_event(
            self.window,
            CookieEvent::Announce(
                catalog
                    .text("cookie_auto_refresh_start")
                    .replace("{browser}", &browser_title(&settings.browser)),
            ),
        );
        match export_to_cache(
            &settings.language,
            &settings.browser,
            &settings.profile,
            &self.cache,
        ) {
            Ok(profile_label) => {
                post_event(
                    self.window,
                    CookieEvent::Exported {
                        origin: CookieExportOrigin::Repair,
                        browser: settings.browser.clone(),
                        profile_label: profile_label.clone(),
                    },
                );
                post_event(
                    self.window,
                    CookieEvent::Announce(
                        catalog
                            .text("cookie_auto_refresh_done")
                            .replace("{profile}", &profile_label),
                    ),
                );
                if let Ok(mut suppressed) = self.suppressed_until.lock() {
                    *suppressed = None;
                }
                Some(self.cache.clone())
            }
            Err(error) => {
                if let Ok(mut suppressed) = self.suppressed_until.lock() {
                    *suppressed = Some(Instant::now() + REPAIR_SUPPRESSION);
                }
                post_event(
                    self.window,
                    CookieEvent::Announce(catalog.text("cookie_auto_refresh_failed").replace(
                        "{error}",
                        &apricot_app::comments::friendly_error(&catalog, &error),
                    )),
                );
                None
            }
        }
    }
}

/// Python `open_youtube_login_profile_from_settings` without the message.
///
/// # Errors
/// Returns the launch error text.
pub(crate) fn open_youtube_login_profile(browser: &str, profile: &str) -> Result<(), String> {
    if apricot_app::cookies::CHROMIUM_COOKIE_BROWSERS.contains(&browser) {
        let executable = apricot_platform::browser_cookies::cookie_browser_executable(browser)
            .ok_or_else(|| format!("{browser} executable not found"))?;
        let mut profile_directory = String::new();
        if !profile.is_empty() && profile != apricot_app::cookies::COOKIE_PROFILE_AUTO {
            let path = Path::new(profile);
            profile_directory = if path.is_absolute() {
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default()
            } else {
                profile.to_owned()
            };
        }
        let mut command = std::process::Command::new(executable);
        if !profile_directory.is_empty() && browser != "opera" {
            command.arg(format!("--profile-directory={profile_directory}"));
        }
        command
            .arg("https://www.youtube.com/")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(|error| error.to_string())
    } else {
        apricot_platform::windows_registration::open_web_url("https://www.youtube.com/")
            .map_err(|error| error.to_string())
    }
}
