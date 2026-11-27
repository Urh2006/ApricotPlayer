//! My `YouTube` for the Mac (Windows `youtube_account_win32.rs`, 2.2.0): the
//! main menu entry below Spotify with the lists of the signed-in `YouTube`
//! account, its sign-in (the user's own browser first, then `ApricotPlayer`'s
//! own sign-in window), the import of its subscriptions and the sign-out.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver},
};

use apricot_app::YoutubeCollectionKind;
use apricot_app::youtube_account::{MyYoutubeEntry, MyYoutubeModel};
use apricot_core::{MediaItem, Route, RouteFrame};
use apricot_media::YoutubeAccountFeed;

use crate::{ListScreen, ScreenControl, Shell, ShellUi, View};

const SIGN_IN_TIMEOUT: std::time::Duration = std::time::Duration::from_mins(15);

/// Settings, Cookies, "Mark played videos as watched on `YouTube`" (2.2.0):
/// a `YouTube` video that starts playing joins the account's history, on a
/// worker, so playback never waits. Both frontends call this when playback
/// starts.
/// Whether a started item is marked as watched: checked before the cookie
/// configuration is prepared, so other playback never touches the cookies.
#[must_use]
pub fn wants_mark_watched(app: &apricot_app::Application, item: &MediaItem) -> bool {
    app.settings().youtube_mark_watched
        && item.source == apricot_core::MediaSource::Youtube
        && item.youtube_video_id().is_some()
        && item.url.is_some()
}

pub fn mark_youtube_watched(
    app: &apricot_app::Application,
    item: &MediaItem,
    components: &std::path::Path,
    config: &apricot_media::YoutubeSessionConfig,
) {
    if !wants_mark_watched(app, item) || !app.youtube_signed_in() {
        return;
    }
    let Some(url) = item.url.as_ref() else {
        return;
    };
    apricot_platform::ytdlp_youtube::mark_watched(
        &apricot_platform::app_update::preferred_ytdlp_executable(components),
        config,
        url.as_str(),
    );
}

/// What a My `YouTube` worker reports.
pub(crate) enum MyYoutubeEvent {
    SignIn {
        browser: String,
        result: Result<Vec<serde_json::Value>, apricot_platform::browser_cookies::SignInEnd>,
    },
    AccountChecked(Result<Option<String>, String>),
}

#[derive(Default)]
pub(crate) struct MyYoutubeState {
    rows: Vec<MyYoutubeEntry>,
    selected: Option<MyYoutubeEntry>,
    signing_in: Option<Arc<AtomicBool>>,
    /// The user's browser the cookies came from, until `YouTube` confirms them.
    from_browser: Option<String>,
    /// The worker that drives the sign-in window and closes it.
    worker: Option<std::thread::JoinHandle<()>>,
    receiver: Option<Receiver<MyYoutubeEvent>>,
    /// The user's browser is being read for the sign-in.
    pub(crate) exporting: bool,
}

pub(crate) const fn is_view(view: View) -> bool {
    matches!(view, View::MyYoutube)
}

impl Shell {
    /// Main menu My `YouTube`, `open_my_youtube`.
    pub(crate) fn show_my_youtube(&mut self, ui: &mut dyn ShellUi) {
        self.remember_menu_item("my_youtube");
        ui.restore();
        self.stop_key_repeat(ui);
        self.cancel_youtube_work();
        self.cancel_local_folder_scan();
        if self.application.current_route() != Route::MyYoutube {
            self.application.navigate_main_menu();
            self.application
                .navigate_to(RouteFrame::new(Route::MyYoutube));
        }
        self.view = View::MyYoutube;
        let catalog = self.catalog();
        let model = MyYoutubeModel::build(
            &catalog,
            self.application.youtube_signed_in(),
            self.application.youtube_account_name().as_deref(),
        );
        self.my_youtube.rows = model.rows.iter().map(|row| row.entry).collect();
        let selected = self
            .my_youtube
            .selected
            .and_then(|entry| self.my_youtube.rows.iter().position(|row| *row == entry))
            .unwrap_or(0);
        let button = |id: &'static str, key: &str| ScreenControl::Button {
            id,
            label: self.text(key),
        };
        ui.show_list_screen(&ListScreen {
            list_name: model.accessible_name,
            rows: model.rows.into_iter().map(|row| row.label).collect(),
            selected: Some(selected),
            controls: vec![
                ScreenControl::List,
                button("open", "open"),
                button("back", "back"),
            ],
            focus_list: true,
        });
    }

    pub(crate) fn my_youtube_activate(&mut self, ui: &mut dyn ShellUi) {
        let Some(entry) = ui
            .selected_row()
            .and_then(|index| self.my_youtube.rows.get(index).copied())
        else {
            return;
        };
        self.my_youtube.selected = Some(entry);
        match entry {
            MyYoutubeEntry::SignIn => self.my_youtube_sign_in(ui),
            MyYoutubeEntry::Feed(feed) => self.my_youtube_open_feed(ui, feed),
            MyYoutubeEntry::ImportSubscriptions => self.my_youtube_import(ui),
            MyYoutubeEntry::SignOut => self.my_youtube_sign_out(ui),
        }
    }

    pub(crate) fn my_youtube_back(&mut self, ui: &mut dyn ShellUi) {
        self.show_main_menu(ui);
    }

    fn my_youtube_open_feed(&mut self, ui: &mut dyn ShellUi, feed: YoutubeAccountFeed) {
        let catalog = self.catalog();
        if !self.application.youtube_signed_in() {
            ui.set_status(catalog.text("my_youtube_not_signed_in"), true);
            return;
        }
        let title = catalog.text(feed.text_key()).to_owned();
        let work = match self.application.begin_youtube_collection(
            title.clone(),
            feed.web_url(),
            YoutubeCollectionKind::Account(feed),
        ) {
            Ok(work) => work,
            Err(error) => {
                ui.set_status(&error.to_string(), true);
                return;
            }
        };
        ui.set_status(
            &catalog.text("loading_playlist").replace("{title}", &title),
            false,
        );
        self.start_youtube_collection_work(ui, work);
    }

    /// A sign-in runs: the browser export, the own window or `YouTube`'s
    /// answer. A second one does not start meanwhile.
    const fn my_youtube_busy(&self) -> bool {
        self.my_youtube.exporting
            || self.my_youtube.signing_in.is_some()
            || self.my_youtube.receiver.is_some()
    }

    fn my_youtube_sign_in(&mut self, ui: &mut dyn ShellUi) {
        let catalog = self.catalog();
        if self.my_youtube_busy() {
            ui.set_status(catalog.text("my_youtube_signing_in"), true);
            return;
        }
        // The browser chosen in Settings, otherwise the default browser.
        let mut browser =
            apricot_app::cookies::normalized_cookies_browser(self.application.settings());
        if browser.is_empty() {
            browser = apricot_platform::browser_cookies::default_browser().unwrap_or_default();
        }
        if !browser.is_empty() && crate::cookies::ytdlp_available() {
            let title = apricot_app::cookies::browser_title(&browser);
            let running = apricot_platform::browser_cookies::cookie_browser_is_running(&browser);
            let question = catalog
                .text(if running {
                    "my_youtube_browser_close_question"
                } else {
                    "my_youtube_browser_question"
                })
                .replace("{browser}", &title);
            if ui.ask_yes_no(&question, catalog.text("yes"), catalog.text("no")) {
                ui.set_status(catalog.text("my_youtube_signing_in"), true);
                self.start_my_youtube_browser_export(ui, browser);
                return;
            }
        }
        let chromium = apricot_app::cookies::CHROMIUM_COOKIE_BROWSERS.contains(&browser.as_str());
        self.my_youtube_open_sign_in_window(ui, if chromium { &browser } else { "" });
    }

    fn my_youtube_open_sign_in_window(&mut self, ui: &mut dyn ShellUi, preferred: &str) {
        let catalog = self.catalog();
        let Some(browser) = apricot_platform::browser_cookies::youtube_sign_in_browser(preferred)
        else {
            ui.show_error(catalog.text("my_youtube_no_browser"));
            return;
        };
        let profile = apricot_app::youtube_account::sign_in_user_data_dir(
            &self.application.app_data_folder(),
        );
        let cancel = Arc::new(AtomicBool::new(false));
        self.my_youtube.signing_in = Some(cancel.clone());
        let (sender, receiver) = mpsc::channel();
        self.my_youtube.receiver = Some(receiver);
        ui.set_status(catalog.text("my_youtube_own_window"), true);
        self.my_youtube.worker = Some(std::thread::spawn(move || {
            let result = apricot_platform::browser_cookies::youtube_sign_in_with_own_profile(
                &browser,
                &profile,
                SIGN_IN_TIMEOUT,
                &cancel,
                apricot_app::youtube_account::devtools_cookies_signed_in,
            );
            let _ = sender.send(MyYoutubeEvent::SignIn { browser, result });
        }));
        ui.set_background_polling(true);
    }

    /// The user's browser was read, or could not be.
    pub(crate) fn my_youtube_browser_read(
        &mut self,
        ui: &mut dyn ShellUi,
        browser: &str,
        succeeded: bool,
    ) {
        if succeeded && self.application.youtube_cookies_have_sign_in() {
            self.my_youtube.from_browser = Some(browser.to_owned());
            self.my_youtube_signed_in(ui);
        } else {
            let chromium = apricot_app::cookies::CHROMIUM_COOKIE_BROWSERS.contains(&browser);
            self.my_youtube_open_sign_in_window(ui, if chromium { browser } else { "" });
        }
    }

    /// The cookies hold a sign-in: `YouTube` is asked whether it accepts it.
    fn my_youtube_signed_in(&mut self, ui: &mut dyn ShellUi) {
        let cookies = self.application.youtube_cookies_text().unwrap_or_default();
        let (sender, receiver) = mpsc::channel();
        self.my_youtube.receiver = Some(receiver);
        std::thread::spawn(move || {
            let answer = apricot_platform::youtube_account::account_name(&cookies);
            let _ = sender.send(MyYoutubeEvent::AccountChecked(answer));
        });
        ui.set_background_polling(true);
    }

    /// Worker answers; `true` while one may still come.
    pub(crate) fn poll_my_youtube(&mut self, ui: &mut dyn ShellUi) -> bool {
        let Some(receiver) = self.my_youtube.receiver.as_ref() else {
            return false;
        };
        let event = match receiver.try_recv() {
            Ok(event) => event,
            Err(mpsc::TryRecvError::Empty) => return true,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.my_youtube.receiver = None;
                return false;
            }
        };
        self.my_youtube.receiver = None;
        let catalog = self.catalog();
        match event {
            MyYoutubeEvent::SignIn { browser, result } => {
                use apricot_platform::browser_cookies::SignInEnd;
                self.my_youtube.signing_in = None;
                ui.restore();
                match result {
                    Ok(cookies) => {
                        match self.application.finish_youtube_sign_in(&browser, &cookies) {
                            Ok(()) => {
                                crate::cookies::reset_repair_suppression();
                                crate::cookies::update_repair_settings(self.application.settings());
                                self.my_youtube_signed_in(ui);
                            }
                            Err(error) => ui.show_error(
                                &catalog
                                    .text("my_youtube_sign_in_failed")
                                    .replace("{error}", &error),
                            ),
                        }
                    }
                    Err(SignInEnd::Closed) => {
                        ui.set_status(catalog.text("my_youtube_sign_in_cancelled"), true);
                    }
                    Err(SignInEnd::TimedOut) => {
                        ui.set_status(catalog.text("my_youtube_sign_in_timeout"), true);
                    }
                    Err(SignInEnd::Failed(error)) => ui.show_error(
                        &catalog
                            .text("my_youtube_sign_in_failed")
                            .replace("{error}", &error),
                    ),
                }
                if is_view(self.view) {
                    self.show_my_youtube(ui);
                }
            }
            MyYoutubeEvent::AccountChecked(answer) => {
                let from_browser = self.my_youtube.from_browser.take();
                let name = match answer {
                    Ok(Some(name)) => Some(name),
                    Err(_) => None,
                    Ok(None) => {
                        let _ = self.application.sign_out_of_youtube();
                        crate::cookies::update_repair_settings(self.application.settings());
                        if let Some(browser) = from_browser {
                            ui.set_status(
                                &catalog.text("my_youtube_browser_not_signed_in").replace(
                                    "{browser}",
                                    &apricot_app::cookies::browser_title(&browser),
                                ),
                                true,
                            );
                            let chromium = apricot_app::cookies::CHROMIUM_COOKIE_BROWSERS
                                .contains(&browser.as_str());
                            self.my_youtube_open_sign_in_window(
                                ui,
                                if chromium { &browser } else { "" },
                            );
                        } else {
                            ui.show_error(catalog.text("my_youtube_not_accepted"));
                        }
                        return self.my_youtube.receiver.is_some();
                    }
                };
                self.application.confirm_youtube_sign_in(name.as_deref());
                let text = name.as_deref().map_or_else(
                    || catalog.text("my_youtube_signed_in_noname").to_owned(),
                    |name| catalog.text("my_youtube_signed_in").replace("{name}", name),
                );
                ui.set_status(&text, true);
                // The sign-in the user started has just finished.
                self.my_youtube.selected = Some(MyYoutubeEntry::Feed(YoutubeAccountFeed::Home));
                if is_view(self.view) {
                    self.show_my_youtube(ui);
                }
            }
        }
        self.my_youtube.receiver.is_some()
    }

    fn my_youtube_sign_out(&mut self, ui: &mut dyn ShellUi) {
        let catalog = self.catalog();
        if !ui.ask_yes_no(
            catalog.text("my_youtube_sign_out_confirm"),
            catalog.text("yes"),
            catalog.text("no"),
        ) {
            return;
        }
        match self.application.sign_out_of_youtube() {
            Ok(()) => {
                crate::cookies::update_repair_settings(self.application.settings());
                self.my_youtube.selected = Some(MyYoutubeEntry::SignIn);
                ui.set_status(catalog.text("my_youtube_signed_out"), true);
                self.show_my_youtube(ui);
            }
            Err(error) => ui.show_error(&error),
        }
    }

    fn my_youtube_import(&mut self, ui: &mut dyn ShellUi) {
        ui.set_status(&self.text("my_youtube_importing"), true);
        self.load_all_youtube_collection(
            ui,
            YoutubeAccountFeed::Channels.web_url().to_owned(),
            YoutubeCollectionKind::Account(YoutubeAccountFeed::Channels),
        );
    }

    /// The account's channels arrived: the new ones join the subscriptions.
    pub(crate) fn my_youtube_channels_read(
        &mut self,
        ui: &mut dyn ShellUi,
        channels: Vec<MediaItem>,
    ) {
        let catalog = self.catalog();
        let total = channels
            .iter()
            .filter(|item| item.kind == apricot_core::MediaKind::Channel)
            .count();
        if total == 0 {
            ui.set_status(catalog.text("my_youtube_import_empty"), true);
            return;
        }
        let new = apricot_app::youtube_account::channels_to_import(channels, |item| {
            self.application.is_subscribed(item)
        });
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0.0, |duration| duration.as_secs_f64());
        let mut added = 0;
        for item in &new {
            match self.application.subscribe_to_item(item, now) {
                Ok(_) => added += 1,
                Err(error) => {
                    ui.show_error(&error.to_string());
                    break;
                }
            }
        }
        ui.set_status(
            &catalog
                .text("my_youtube_imported")
                .replace("{count}", &added.to_string())
                .replace("{existing}", &(total - new.len()).to_string()),
            true,
        );
    }

    /// Closes an open sign-in window when `ApricotPlayer` exits and waits
    /// until its browser is gone (a few seconds at most).
    pub(crate) fn my_youtube_shutdown(&mut self) {
        if let Some(cancel) = &self.my_youtube.signing_in {
            cancel.store(true, Ordering::SeqCst);
        }
        if let Some(worker) = self.my_youtube.worker.take() {
            let _ = worker.join();
        }
    }
}
