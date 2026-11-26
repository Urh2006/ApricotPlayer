//! My `YouTube` (2.2.0): the main menu entry below Spotify with the lists of
//! the signed-in `YouTube` account. Each list opens as a `YouTube` collection
//! (Enter plays, Load more at the end, the usual context menu); Back returns
//! to the same row here. The sign-in first reads the user's own browser, as
//! the cookie export in Settings does, and otherwise opens `ApricotPlayer`'s
//! own sign-in window, which closes by itself once `YouTube` knows the user.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use apricot_app::YoutubeCollectionKind;
use apricot_app::youtube_account::{MyYoutubeEntry, MyYoutubeModel};
use apricot_core::{Route, RouteFrame};
use windows::Win32::{
    Foundation::{HWND, WPARAM},
    UI::WindowsAndMessaging::{
        LB_GETCURSEL, LB_RESETCONTENT, LB_SETCURSEL, SendMessageW, SetForegroundWindow,
    },
};

use super::{
    MainView, add_list_string, ask_yes_no, cancel_local_folder_scan, cancel_youtube_work,
    catalog_text, focus_list, layout_controls_state, remember_menu_item, restore_from_tray,
    set_open_button_label, set_status, show_error_message, show_main_menu,
    start_youtube_collection_work, state, state_mut, stop_controlled_repeat,
};

/// How long the own sign-in window waits for the user.
const SIGN_IN_TIMEOUT: std::time::Duration = std::time::Duration::from_mins(15);

#[derive(Default)]
pub(super) struct MyYoutubeState {
    rows: Vec<MyYoutubeEntry>,
    selected: Option<MyYoutubeEntry>,
    /// The own sign-in window is open; setting it closes the window.
    signing_in: Option<Arc<AtomicBool>>,
    /// The worker that drives the sign-in window and closes it.
    worker: Option<std::thread::JoinHandle<()>>,
}

pub(super) const fn is_view(view: MainView) -> bool {
    matches!(view, MainView::MyYoutube)
}

fn catalog(state: &super::WindowState) -> apricot_core::TranslationCatalog {
    apricot_app::embedded_catalog(&state.application.settings().language)
}

/// Main menu My `YouTube`, `open_my_youtube`.
pub(super) unsafe fn show(window: HWND) {
    remember_menu_item(window, "my_youtube");
    restore_from_tray(window);
    stop_controlled_repeat(window);
    let Some(state) = state_mut(window) else {
        return;
    };
    cancel_youtube_work(window, state);
    cancel_local_folder_scan(window, state);
    if state.application.current_route() != Route::MyYoutube {
        state.application.navigate_main_menu();
        state
            .application
            .navigate_to(RouteFrame::new(Route::MyYoutube));
    }
    state.view = MainView::MyYoutube;
    let catalog = catalog(state);
    let model = MyYoutubeModel::build(
        &catalog,
        state.application.youtube_signed_in(),
        state.application.youtube_account_name().as_deref(),
    );
    set_open_button_label(state, "open");
    SendMessageW(state.list, LB_RESETCONTENT, None, None);
    crate::accessibility_win32::set_control_name(state.list, &model.accessible_name);
    for row in &model.rows {
        add_list_string(state.list, &row.label);
    }
    state.my_youtube.rows = model.rows.iter().map(|row| row.entry).collect();
    let selected = state
        .my_youtube
        .selected
        .and_then(|entry| state.my_youtube.rows.iter().position(|row| *row == entry))
        .unwrap_or(0);
    SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(selected)), None);
    layout_controls_state(window, state);
    focus_list(state.list);
}

/// Enter or Open on a row.
pub(super) unsafe fn activate(window: HWND) {
    let Some(entry) = state(window).and_then(|state| {
        let index = usize::try_from(SendMessageW(state.list, LB_GETCURSEL, None, None).0).ok()?;
        state.my_youtube.rows.get(index).copied()
    }) else {
        return;
    };
    if let Some(state) = state_mut(window) {
        state.my_youtube.selected = Some(entry);
    }
    match entry {
        MyYoutubeEntry::SignIn => sign_in(window),
        MyYoutubeEntry::Feed(feed) => open_feed(window, feed),
        MyYoutubeEntry::ImportSubscriptions => import_subscriptions(window),
        MyYoutubeEntry::SignOut => sign_out(window),
    }
}

/// Escape and Back: the main menu.
pub(super) unsafe fn back(window: HWND) {
    show_main_menu(window);
}

unsafe fn open_feed(window: HWND, feed: apricot_app::YoutubeAccountFeed) {
    let work = {
        let Some(state) = state_mut(window) else {
            return;
        };
        let catalog = catalog(state);
        if !state.application.youtube_signed_in() {
            set_status(state, catalog.text("my_youtube_not_signed_in"), true);
            return;
        }
        let title = catalog.text(feed.text_key()).to_owned();
        match state.application.begin_youtube_collection(
            title.clone(),
            feed.web_url(),
            YoutubeCollectionKind::Account(feed),
        ) {
            Ok(work) => {
                let message = catalog.text("loading_playlist").replace("{title}", &title);
                set_status(state, &message, false);
                work
            }
            Err(error) => {
                set_status(state, &error.to_string(), true);
                return;
            }
        }
    };
    start_youtube_collection_work(window, work);
}

// ---------------------------------------------------------------------------
// Sign-in

unsafe fn sign_in(window: HWND) {
    let Some(state) = state(window) else {
        return;
    };
    if state.my_youtube.signing_in.is_some() {
        return;
    }
    let catalog = catalog(state);
    let browser = apricot_app::cookies::normalized_cookies_browser(state.application.settings());
    let chromium = apricot_app::cookies::CHROMIUM_COOKIE_BROWSERS.contains(&browser.as_str());
    // Urh (2.2.0): first the user's own browser, then the own window.
    if !browser.is_empty() && crate::cookies_win32::ytdlp_available() {
        let title = apricot_app::cookies::browser_title(&browser);
        let running = apricot_platform::browser_cookies::cookie_browser_is_running(&browser);
        let question = catalog
            .text(if running {
                "my_youtube_browser_close_question"
            } else {
                "my_youtube_browser_question"
            })
            .replace("{browser}", &title);
        if ask_yes_no(window, &question) {
            let Some(state) = state_mut(window) else {
                return;
            };
            set_status(state, catalog.text("my_youtube_signing_in"), true);
            let settings = state.application.settings();
            crate::cookies_win32::spawn_export(
                window,
                crate::cookies_win32::CookieExportOrigin::MyYoutube,
                settings.language.clone(),
                browser,
                settings.cookies_browser_profile.clone(),
                state.application.cached_cookies_file(),
            );
            return;
        }
    }
    open_sign_in_window(window, if chromium { &browser } else { "" });
}

/// `ApricotPlayer`'s own sign-in window with its own browser profile.
unsafe fn open_sign_in_window(window: HWND, preferred: &str) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let catalog = catalog(state);
    let Some(browser) = apricot_platform::browser_cookies::youtube_sign_in_browser(preferred)
    else {
        show_error_message(window, catalog.text("my_youtube_no_browser"));
        return;
    };
    let profile =
        apricot_app::youtube_account::sign_in_user_data_dir(&state.application.app_data_folder());
    let cancel = Arc::new(AtomicBool::new(false));
    state.my_youtube.signing_in = Some(cancel.clone());
    set_status(state, catalog.text("my_youtube_own_window"), true);
    let target = window.0 as isize;
    state.my_youtube.worker = Some(std::thread::spawn(move || {
        let result = apricot_platform::browser_cookies::youtube_sign_in_with_own_profile(
            &browser,
            &profile,
            SIGN_IN_TIMEOUT,
            &cancel,
            apricot_app::youtube_account::devtools_cookies_signed_in,
        );
        crate::cookies_win32::post_event(
            target,
            crate::cookies_win32::CookieEvent::YoutubeSignIn {
                browser,
                result: Box::new(result),
            },
        );
    }));
}

/// The user's browser was read (or could not be): signed in, or on to the
/// own sign-in window.
pub(super) unsafe fn browser_read(window: HWND, browser: &str, succeeded: bool) {
    let signed_in = state(window).is_some_and(|state| state.application.youtube_signed_in());
    if succeeded && signed_in {
        signed_in_now(window);
    } else {
        let chromium = apricot_app::cookies::CHROMIUM_COOKIE_BROWSERS.contains(&browser);
        open_sign_in_window(window, if chromium { browser } else { "" });
    }
}

/// The own sign-in window ended.
pub(super) unsafe fn sign_in_window_ended(
    window: HWND,
    browser: &str,
    result: Result<Vec<serde_json::Value>, apricot_platform::browser_cookies::SignInEnd>,
) {
    use apricot_platform::browser_cookies::SignInEnd;
    let Some(state) = state_mut(window) else {
        return;
    };
    state.my_youtube.signing_in = None;
    let catalog = catalog(state);
    restore_from_tray(window);
    let _ = SetForegroundWindow(window);
    match result {
        Ok(cookies) => match state.application.finish_youtube_sign_in(browser, &cookies) {
            Ok(()) => {
                crate::cookies_win32::reset_repair_suppression();
                crate::cookies_win32::update_repair_settings(state.application.settings());
                signed_in_now(window);
            }
            Err(error) => show_error_message(
                window,
                &catalog
                    .text("my_youtube_sign_in_failed")
                    .replace("{error}", &error),
            ),
        },
        Err(SignInEnd::Closed) => {
            set_status(state, catalog.text("my_youtube_sign_in_cancelled"), true);
        }
        Err(SignInEnd::TimedOut) => {
            set_status(state, catalog.text("my_youtube_sign_in_timeout"), true);
        }
        Err(SignInEnd::Failed(error)) => show_error_message(
            window,
            &catalog
                .text("my_youtube_sign_in_failed")
                .replace("{error}", &error),
        ),
    }
    if state_mut(window).is_some_and(|state| is_view(state.view)) {
        show(window);
    }
}

/// Signed in: the name is read in the background and announced.
unsafe fn signed_in_now(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    state.my_youtube.selected = Some(MyYoutubeEntry::Feed(apricot_app::YoutubeAccountFeed::Home));
    let cookies = state.application.youtube_cookies_text().unwrap_or_default();
    let target = window.0 as isize;
    std::thread::spawn(move || {
        let name = apricot_platform::youtube_account::account_name(&cookies)
            .ok()
            .flatten();
        crate::cookies_win32::post_event(
            target,
            crate::cookies_win32::CookieEvent::YoutubeAccountName(name),
        );
    });
    if is_view(state.view) {
        show(window);
    }
}

/// The account name arrived.
pub(super) unsafe fn account_name_read(window: HWND, name: Option<&str>) {
    let Some(state) = state_mut(window) else {
        return;
    };
    // The rows already show the lists; the name appears in the sign-out row
    // the next time My YouTube opens, so the selection and focus stay where
    // the user is.
    state.application.set_youtube_account_name(name);
    let catalog = catalog(state);
    let text = name.map_or_else(
        || catalog.text("my_youtube_signed_in_noname").to_owned(),
        |name| catalog.text("my_youtube_signed_in").replace("{name}", name),
    );
    set_status(state, &text, true);
}

unsafe fn sign_out(window: HWND) {
    let Some(state) = state(window) else {
        return;
    };
    let catalog = catalog(state);
    if !ask_yes_no(window, catalog.text("my_youtube_sign_out_confirm")) {
        return;
    }
    let Some(state) = state_mut(window) else {
        return;
    };
    match state.application.sign_out_of_youtube() {
        Ok(()) => {
            crate::cookies_win32::update_repair_settings(state.application.settings());
            state.my_youtube.selected = Some(MyYoutubeEntry::SignIn);
            set_status(state, catalog.text("my_youtube_signed_out"), true);
            show(window);
        }
        Err(error) => show_error_message(window, &error),
    }
}

// ---------------------------------------------------------------------------
// Import of the account's subscriptions

unsafe fn import_subscriptions(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let text = catalog_text(&state.application, "my_youtube_importing");
    set_status(state, &text, true);
    super::load_all_youtube_collection(
        window,
        apricot_app::YoutubeAccountFeed::Channels.web_url(),
        YoutubeCollectionKind::Account(apricot_app::YoutubeAccountFeed::Channels),
        super::AllCollectionPurpose::ImportSubscriptions,
    );
}

/// The account's channels arrived: the new ones join `ApricotPlayer`'s
/// subscriptions; nothing is removed.
pub(super) unsafe fn channels_read(window: HWND, channels: Vec<apricot_core::MediaItem>) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let catalog = catalog(state);
    let total = channels
        .iter()
        .filter(|item| item.kind == apricot_core::MediaKind::Channel)
        .count();
    if total == 0 {
        set_status(state, catalog.text("my_youtube_import_empty"), true);
        return;
    }
    let new = apricot_app::youtube_account::channels_to_import(channels, |item| {
        state.application.is_subscribed(item)
    });
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |duration| duration.as_secs_f64());
    let mut added = 0;
    for item in &new {
        match state.application.subscribe_to_item(item, now) {
            Ok(_) => added += 1,
            Err(error) => {
                show_error_message(window, &error.to_string());
                break;
            }
        }
    }
    let Some(state) = state_mut(window) else {
        return;
    };
    let text = catalog
        .text("my_youtube_imported")
        .replace("{count}", &added.to_string())
        .replace("{existing}", &(total - new.len()).to_string());
    set_status(state, &text, true);
    focus_list(state.list);
}

/// Closes an open sign-in window when `ApricotPlayer` exits and waits until
/// its browser is gone (a few seconds at most).
pub(super) fn shutdown(state: &mut super::WindowState) {
    if let Some(cancel) = &state.my_youtube.signing_in {
        cancel.store(true, Ordering::SeqCst);
    }
    if let Some(worker) = state.my_youtube.worker.take() {
        let _ = worker.join();
    }
}
