//! The cookie workflow (Windows `cookies_win32.rs` and
//! `handle_cookie_event`): the yt-dlp and browser backend, the export
//! worker, the automatic repair used by yt-dlp workers, the playback retry
//! after a sign-in error and the Settings commands for cookies.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use apricot_app::cookies::{
    BrowserCookieBackend, browser_title, export_browser_cookies, save_cookie_jar_to_cache,
};
use apricot_platform::browser_cookies::{
    CookieRepairHook, close_cookie_browser_processes, cookie_browser_is_running,
    devtools_browser_cookies, discover_cookie_profiles, extract_browser_cookies_with_ytdlp,
    set_cookie_repair_hook, wait_for_cookie_browser_exit,
};
use serde_json::Value;

use crate::{Shell, ShellUi};

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

/// The yt-dlp in `components`.
fn ytdlp_executable() -> Option<PathBuf> {
    crate::youtube::components_directory()
        .map(|components| apricot_platform::app_update::preferred_ytdlp_executable(&components))
}

/// Python `get_yt_dlp() is not None`.
pub(crate) fn ytdlp_available() -> bool {
    ytdlp_executable().is_some_and(|path| path.is_file())
}

struct PlatformCookieBackend {
    ytdlp: Option<PathBuf>,
}

impl PlatformCookieBackend {
    fn new() -> Self {
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

/// Python `export_browser_cookies_blocking(allow_close=True)` and the cache.
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
fn spawn_export(
    sender: Sender<CookieEvent>,
    origin: CookieExportOrigin,
    language: String,
    browser: String,
    profile: String,
    cache: PathBuf,
) {
    std::thread::spawn(move || {
        let event = match export_to_cache(&language, &browser, &profile, &cache) {
            Ok(profile_label) => CookieEvent::Exported {
                origin,
                browser,
                profile_label,
            },
            Err(error) => CookieEvent::Failed { origin, error },
        };
        let _ = sender.send(event);
    });
}

#[derive(Clone, Default)]
struct RepairSettings {
    language: String,
    browser: String,
    profile: String,
}

/// Python `repair_cookies_for_error` for yt-dlp workers.
struct CookieRepairer {
    sender: Mutex<Sender<CookieEvent>>,
    cache: PathBuf,
    settings: Mutex<RepairSettings>,
    suppressed_until: Mutex<Option<Instant>>,
    /// Python `cookie_repair_lock`.
    running: Mutex<()>,
}

static REPAIRER: OnceLock<Arc<CookieRepairer>> = OnceLock::new();

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

impl CookieRepairer {
    fn post(&self, event: CookieEvent) {
        if let Ok(sender) = self.sender.lock() {
            let _ = sender.send(event);
        }
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
        self.post(CookieEvent::Announce(
            catalog
                .text("cookie_auto_refresh_start")
                .replace("{browser}", &browser_title(&settings.browser)),
        ));
        match export_to_cache(
            &settings.language,
            &settings.browser,
            &settings.profile,
            &self.cache,
        ) {
            Ok(profile_label) => {
                self.post(CookieEvent::Exported {
                    origin: CookieExportOrigin::Repair,
                    browser: settings.browser.clone(),
                    profile_label: profile_label.clone(),
                });
                self.post(CookieEvent::Announce(
                    catalog
                        .text("cookie_auto_refresh_done")
                        .replace("{profile}", &profile_label),
                ));
                if let Ok(mut suppressed) = self.suppressed_until.lock() {
                    *suppressed = None;
                }
                Some(self.cache.clone())
            }
            Err(error) => {
                if let Ok(mut suppressed) = self.suppressed_until.lock() {
                    *suppressed = Some(Instant::now() + REPAIR_SUPPRESSION);
                }
                self.post(CookieEvent::Announce(
                    catalog.text("cookie_auto_refresh_failed").replace(
                        "{error}",
                        &apricot_app::comments::friendly_error(&catalog, &error),
                    ),
                ));
                None
            }
        }
    }
}

/// Python `open_youtube_login_profile_from_settings` without the message.
fn open_youtube_login_profile(browser: &str, profile: &str) -> Result<(), String> {
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
        apricot_platform::shell::open_url("https://www.youtube.com/")
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}

/// The cookie workers' channel and the playback waiting for a refresh
/// (Windows `pending_cookie_playback`).
pub(crate) struct CookieState {
    sender: Sender<CookieEvent>,
    receiver: Receiver<CookieEvent>,
    pub(crate) pending_playback: Option<crate::youtube::PendingResolve>,
}

impl Default for CookieState {
    fn default() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            sender,
            receiver,
            pending_playback: None,
        }
    }
}

/// What a Settings cookie command did, for the Settings window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CookieCommandResult {
    /// Nothing to show.
    None,
    /// Speak this.
    Announce(String),
    /// A message with an optional title, then speak `announce`.
    Message {
        title: Option<String>,
        text: String,
        announce: Option<String>,
    },
    /// The cookies file field shows `path` and the browser choice
    /// `browser_index` (and the profile Auto when it is 0).
    Source {
        path: String,
        browser_index: usize,
        announce: Vec<String>,
        warning: Option<String>,
    },
}

impl Shell {
    /// Windows `install_repairer` at the start.
    pub(crate) fn install_cookie_repairer(&self) {
        let repairer = Arc::new(CookieRepairer {
            sender: Mutex::new(self.cookies.sender.clone()),
            cache: self.application.cached_cookies_file(),
            settings: Mutex::new(RepairSettings::default()),
            suppressed_until: Mutex::new(None),
            running: Mutex::new(()),
        });
        if REPAIRER.set(Arc::clone(&repairer)).is_ok() {
            set_cookie_repair_hook(repairer);
        }
        update_repair_settings(self.application.settings());
    }

    /// Windows `WM_COOKIE_EVENT` (`handle_cookie_event`); `true` while an
    /// export may still answer.
    pub(crate) fn poll_cookie_events(&mut self, ui: &mut dyn ShellUi) -> bool {
        while let Ok(event) = self.cookies.receiver.try_recv() {
            self.handle_cookie_event(ui, event);
        }
        self.cookies.pending_playback.is_some() || self.settings_cookie_export_running
    }

    fn handle_cookie_event(&mut self, ui: &mut dyn ShellUi, event: CookieEvent) {
        let catalog = self.catalog();
        match event {
            CookieEvent::Announce(text) => ui.set_status(&text, true),
            CookieEvent::Exported {
                origin,
                browser,
                profile_label,
            } => {
                if let Err(error) = self.application.remember_browser_cookie_export(&browser) {
                    ui.show_error(&error.to_string());
                }
                reset_repair_suppression();
                update_repair_settings(self.application.settings());
                let path = self
                    .application
                    .cached_cookies_file()
                    .to_string_lossy()
                    .into_owned();
                match origin {
                    CookieExportOrigin::Settings => {
                        self.settings_cookie_export_running = false;
                        let index = apricot_app::cookies::COOKIES_BROWSER_OPTIONS
                            .iter()
                            .position(|option| *option == browser)
                            .unwrap_or_default();
                        ui.settings_cookie_source(&path, index);
                        let text = catalog
                            .text("browser_cookies_exported")
                            .replace("{path}", &path)
                            .replace("{profile}", &profile_label);
                        ui.set_status(&text, true);
                    }
                    CookieExportOrigin::Repair => {}
                    CookieExportOrigin::Playback(token) => {
                        let Some(pending) = self
                            .cookies
                            .pending_playback
                            .take_if(|pending| pending.token == token)
                        else {
                            return;
                        };
                        let text = catalog
                            .text("cookie_auto_refresh_done")
                            .replace("{profile}", &profile_label);
                        ui.set_status(&text, true);
                        self.retry_cookie_playback(ui, &pending);
                    }
                }
            }
            CookieEvent::Failed { origin, error } => {
                let friendly = apricot_app::comments::friendly_error(&catalog, &error);
                match origin {
                    CookieExportOrigin::Settings => {
                        self.settings_cookie_export_running = false;
                        let text = catalog
                            .text("browser_cookies_export_failed")
                            .replace("{error}", &friendly);
                        ui.show_error(&text);
                    }
                    CookieExportOrigin::Repair => {}
                    CookieExportOrigin::Playback(token) => {
                        if self
                            .cookies
                            .pending_playback
                            .take_if(|pending| pending.token == token)
                            .is_some()
                        {
                            let text = catalog
                                .text("cookie_auto_refresh_failed")
                                .replace("{error}", &friendly);
                            ui.show_error(&text);
                        }
                    }
                }
            }
        }
    }

    /// Windows `prompt_cookie_refresh_for_playback`.
    pub(crate) fn prompt_cookie_refresh_for_playback(
        &mut self,
        ui: &mut dyn ShellUi,
        pending: crate::youtube::PendingResolve,
        player_failed: &str,
    ) {
        let message = format!(
            "{player_failed}\n\n{}",
            self.text("cookie_refresh_prompt_message")
        );
        if !ui.ask_yes_no(&message, &self.text("yes"), &self.text("no")) {
            return;
        }
        let browser = apricot_app::cookies::normalized_cookies_browser(self.application.settings());
        if browser.is_empty() {
            ui.show_error(&self.text("select_cookies_browser"));
            return;
        }
        let text = self
            .text("cookie_auto_refresh_start")
            .replace("{browser}", &browser_title(&browser));
        ui.set_status(&text, true);
        let token = pending.token;
        self.cookies.pending_playback = Some(pending);
        let settings = self.application.settings();
        spawn_export(
            self.cookies.sender.clone(),
            CookieExportOrigin::Playback(token),
            settings.language.clone(),
            browser,
            settings.cookies_browser_profile.clone(),
            self.application.cached_cookies_file(),
        );
        ui.set_background_polling(true);
    }

    /// Windows `choose_cookies_file` after the file was chosen.
    pub fn settings_import_cookies_file(&mut self, path: &Path) -> CookieCommandResult {
        let catalog = self.catalog();
        let result = match self.application.import_cookies_file(path) {
            Ok(result) => result,
            Err(error) => {
                return CookieCommandResult::Message {
                    title: None,
                    text: catalog.text("cookies_file_load_failed").replace(
                        "{error}",
                        &apricot_app::comments::friendly_error(
                            &catalog,
                            &error.localized(&catalog),
                        ),
                    ),
                    announce: None,
                };
            }
        };
        reset_repair_suppression();
        let path_text = path.to_string_lossy().into_owned();
        let mut announce = vec![
            catalog
                .text(result.message_key())
                .replace("{path}", &path_text),
        ];
        let warning = if result.has_login {
            announce.push(catalog.text("cookies_file_login_found").to_owned());
            None
        } else {
            Some(catalog.text("cookies_file_no_login_warning").to_owned())
        };
        CookieCommandResult::Source {
            path: path_text,
            browser_index: 0,
            announce,
            warning,
        }
    }

    /// The browser of the Settings for an export or the login profile, or
    /// Python's `select_cookies_browser` message.
    fn settings_cookie_browser(&self) -> Result<String, CookieCommandResult> {
        let browser = apricot_app::cookies::normalized_cookies_browser(self.application.settings());
        if browser.is_empty() {
            return Err(CookieCommandResult::Message {
                title: None,
                text: self.text("select_cookies_browser"),
                announce: None,
            });
        }
        Ok(browser)
    }

    /// Windows `open_youtube_login_profile` (after the visible controls were
    /// applied).
    pub fn settings_open_youtube_login(&self) -> CookieCommandResult {
        let browser = match self.settings_cookie_browser() {
            Ok(browser) => browser,
            Err(message) => return message,
        };
        let profile = &self.application.settings().cookies_browser_profile;
        match open_youtube_login_profile(&browser, profile) {
            Ok(()) => CookieCommandResult::Announce(self.text("youtube_profile_opened")),
            Err(error) => CookieCommandResult::Message {
                title: None,
                text: self
                    .text("youtube_profile_open_failed")
                    .replace("{error}", &error),
                announce: None,
            },
        }
    }

    /// Windows `export_browser_cookies` before the export: the browser,
    /// or why there is none; `close_prompt` asks to close a running
    /// browser (title and message).
    ///
    /// # Errors
    ///
    /// Returns what to show when the export cannot start.
    pub fn settings_cookie_export_browser(
        &self,
    ) -> Result<(String, Option<(String, String)>), CookieCommandResult> {
        if !ytdlp_available() {
            return Err(CookieCommandResult::Message {
                title: None,
                text: self.text("missing_ytdlp"),
                announce: None,
            });
        }
        let browser = self.settings_cookie_browser()?;
        let prompt = cookie_browser_is_running(&browser).then(|| {
            (
                self.text("close_browser_for_cookie_export_title"),
                self.text("close_browser_for_cookie_export_message")
                    .replace("{browser}", &browser_title(&browser)),
            )
        });
        Ok((browser, prompt))
    }

    /// Windows `export_browser_cookies` after the close prompt: closes the
    /// browser when asked and starts the export; the result arrives as a
    /// cookie event.
    pub fn settings_start_cookie_export(
        &mut self,
        ui: &mut dyn ShellUi,
        browser: String,
        close_browser: bool,
    ) -> Vec<String> {
        let mut announcements = Vec::new();
        if close_browser {
            if close_cookie_browser_processes(&browser) {
                announcements.push(self.text("browser_closed_for_cookie_export"));
            }
            wait_for_cookie_browser_exit(&browser, Duration::from_secs(6));
        }
        announcements.push(self.text("exporting_browser_cookies"));
        let settings = self.application.settings();
        self.settings_cookie_export_running = true;
        spawn_export(
            self.cookies.sender.clone(),
            CookieExportOrigin::Settings,
            settings.language.clone(),
            browser,
            settings.cookies_browser_profile.clone(),
            self.application.cached_cookies_file(),
        );
        ui.set_background_polling(true);
        announcements
    }
}
