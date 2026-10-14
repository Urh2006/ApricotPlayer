//! Real Win32 controls with synthetic Spotify data and a private account store.
use super::*;
use apricot_app::context_menu::{ContextCommand as C, ContextMenuContext, ContextMenuEntry};
use apricot_app::spotify::{SpotifyHubEntry, SpotifyHubModel};
use std::{ffi::c_void, path::PathBuf};
use windows::Win32::{
    Foundation::LRESULT,
    System::LibraryLoader::GetModuleHandleW,
    UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, LB_GETCOUNT, RegisterClassW,
        SetWindowLongPtrW, WINDOW_LONG_PTR_INDEX, WNDCLASSW, WS_OVERLAPPEDWINDOW,
    },
};

unsafe extern "system" fn procedure(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

struct Fixture {
    window: HWND,
    state: Option<Box<WindowState>>,
    folder: PathBuf,
}

impl Fixture {
    fn new() -> windows::core::Result<Self> {
        // SAFETY: All controls are owned by this thread's hidden test window.
        unsafe {
            let instance = HINSTANCE(GetModuleHandleW(None)?.0);
            let class = w!("ApricotSpotifyNativeQA");
            RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(procedure),
                hInstance: instance,
                lpszClassName: class,
                cbWndExtra: i32::try_from(std::mem::size_of::<usize>()).unwrap(),
                ..Default::default()
            });
            let window = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                class,
                w!("Isolated Spotify QA"),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                700,
                650,
                None,
                None,
                Some(instance),
                None,
            )?;
            let folder = std::env::temp_dir().join(format!(
                "apricot-spotify-qa-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&folder).unwrap();
            let accounts = fixture_accounts();
            std::fs::write(
                folder.join("spotify_accounts.json"),
                serde_json::to_vec(&accounts).unwrap(),
            )
            .unwrap();
            let settings = apricot_app::SettingsController::load(
                apricot_storage::SettingsPaths::for_app_data(&folder, &folder.join("legacy")),
                apricot_storage::SettingsDocument::default(),
            );
            let app = apricot_app::Application::new(
                settings,
                apricot_app::MainMenuAvailability::default(),
            );
            let mut state = Box::new(super::super::create_controls(window, instance, app)?);
            state.spotify.fixture_runtime(&folder);
            SetWindowLongPtrW(
                window,
                WINDOW_LONG_PTR_INDEX(0),
                (&raw mut *state).cast::<c_void>() as isize,
            );
            Ok(Self {
                window,
                state: Some(state),
                folder,
            })
        }
    }

    fn choose(&self, index: usize) {
        // SAFETY: Reads/writes the list owned by this fixture's thread.
        unsafe {
            SendMessageW(
                self.state.as_ref().unwrap().list,
                LB_SETCURSEL,
                Some(WPARAM(index)),
                None,
            );
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // SAFETY: The state remains alive until its controls are destroyed.
        unsafe {
            let _ = DestroyWindow(self.window);
        }
        self.state.take();
        if self.folder.parent() == Some(std::env::temp_dir().as_path()) {
            let _ = std::fs::remove_dir_all(&self.folder);
        }
    }
}

fn fixture_accounts() -> apricot_spotify::SpotifyAccounts {
    apricot_spotify::SpotifyAccounts {
        active: Some("1111111111111111".into()),
        accounts: vec![apricot_spotify::SpotifyAccount {
            key: "1111111111111111".into(),
            display_name: "QA".into(),
            credentials: "fixture-never-used-for-login".into(),
            ..Default::default()
        }],
        ..Default::default()
    }
}

fn commands(state: &WindowState) -> Vec<C> {
    let texts = super::super::spotify::catalog(state);
    let context = ContextMenuContext {
        catalog: &texts,
        settings: state.application.settings(),
        user_playlist_titles: &[],
        queued_download_count: 0,
    };
    apricot_app::context_menu::spotify_browse_context_menu(&context, row_menu(state).unwrap())
        .into_iter()
        .filter_map(|entry| match entry {
            ContextMenuEntry::Command { command, .. } => Some(command),
            _ => None,
        })
        .collect()
}

fn item(kind: ItemKind, uri: &str) -> CatalogItem {
    let mut row = section_row(&apricot_spotify::catalog::Section {
        uri: uri.into(),
        title: "QA".into(),
        items: Vec::new(),
    });
    row.kind = kind;
    row.artist_uri = "spotify:artist:0000000000000000000001".into();
    row.playable = kind.is_playable_item();
    row
}

#[test]
#[ignore = "requires native Windows desktop controls"]
#[allow(clippy::too_many_lines)]
fn native_spotify_profile_and_pins_keep_routes_and_scope() -> windows::core::Result<()> {
    let fixture = Fixture::new()?;
    let window = fixture.window;
    // SAFETY: Calls production handlers against the fixture's controls only.
    unsafe {
        let service = super::super::spotify::service(window).unwrap();
        let pins = [
            MenuPin::new(
                "QA playlist",
                MenuTarget::Playlist("spotify:playlist:0000000000000000000001".into()),
            ),
            MenuPin::new(
                "QA category",
                MenuTarget::Category("spotify:page:qa".into()),
            ),
        ];
        for pin in &pins {
            assert!(service.toggle_menu_pin(pin.clone()).unwrap());
        }
        super::super::spotify::show_hub(window);
        let state = super::state(window).unwrap();
        let profile = state
            .spotify
            .hub_entries
            .iter()
            .position(|entry| *entry == SpotifyHubEntry::MyProfile)
            .unwrap();
        assert_eq!(
            state.spotify.hub_entries[profile + 1],
            SpotifyHubEntry::Settings
        );
        assert_eq!(
            state.spotify_browse.pins.len(),
            2,
            "pins survive without a network session"
        );
        let pinned = state
            .spotify
            .hub_entries
            .iter()
            .position(|entry| *entry == SpotifyHubEntry::Pinned(0))
            .unwrap();
        fixture.choose(pinned);
        let menu = commands(super::state(window).unwrap());
        assert!(
            menu.contains(&C::SpotifyPlay)
                && menu.contains(&C::SpotifyOpen)
                && menu.contains(&C::SpotifyToggleMenuPin)
        );
        let state = super::state_mut(window).unwrap();
        let uri = context_item(state).unwrap().uri;
        assert!(
            !menu.contains(&C::SpotifyToggleProfilePublication),
            "unknown publication stays disabled"
        );
        state
            .spotify_browse
            .profile_states
            .insert(uri.clone(), false);
        assert!(commands(state).contains(&C::SpotifyToggleProfilePublication));
        assert!(
            commands(state).contains(&C::SpotifyToggleMenuPin),
            "profile and local menu pins are separate commands"
        );
        state.spotify_browse.profile_states.insert(uri, true);
        assert_eq!(row_menu(state).unwrap().playlist_published, Some(true));
        super::super::spotify::activate(window);
        assert!(
            matches!(&top(super::state(window).unwrap()).unwrap().source, Source::Playlist(uri) if uri.ends_with('1'))
        );
        super::super::spotify::show_hub(window);
        let pinned = super::state(window)
            .unwrap()
            .spotify
            .hub_entries
            .iter()
            .position(|entry| *entry == SpotifyHubEntry::Pinned(1))
            .unwrap();
        fixture.choose(pinned);
        let menu = commands(super::state(window).unwrap());
        assert!(
            menu.contains(&C::SpotifyOpen)
                && !menu.contains(&C::SpotifyPlay)
                && !menu.contains(&C::SpotifyToggleSaved)
        );
        assert!(!menu.contains(&C::SpotifyToggleProfilePublication));
        super::super::spotify::activate(window);
        assert!(
            matches!(&top(super::state(window).unwrap()).unwrap().source, Source::Browse(Some(uri)) if uri == "spotify:page:qa")
        );
        let profile_uri = "spotify:user:qa";
        open_root(
            window,
            Source::MyProfile(profile_uri.into()),
            "QA profile".into(),
        );
        let profile_state = super::state(window).unwrap();
        for (index, row) in top(profile_state).unwrap().items.iter().enumerate() {
            assert_eq!(
                row.count, None,
                "unloaded profile navigation has no known count"
            );
            let mut buffer = [0u16; 256];
            let length = SendMessageW(
                profile_state.list,
                windows::Win32::UI::WindowsAndMessaging::LB_GETTEXT,
                Some(WPARAM(index)),
                Some(LPARAM(buffer.as_mut_ptr() as isize)),
            )
            .0;
            let length = usize::try_from(length).expect("native list label read succeeded");
            assert!(length < buffer.len());
            assert_eq!(String::from_utf16_lossy(&buffer[..length]), row.name);
        }
        assert_eq!(
            SendMessageW(super::state(window).unwrap().list, LB_GETCOUNT, None, None).0,
            4
        );
        assert!(
            row_menu(super::state(window).unwrap())
                .unwrap()
                .main_menu_pin
                .is_none()
        );
        fixture.choose(2);
        activate(window);
        let state = super::state(window).unwrap();
        assert_eq!(state.spotify_browse.frames.len(), 2);
        assert!(
            matches!(&top(state).unwrap().source, Source::Search { kind: SearchKind::Profiles, query } if query.is_empty())
        );
        assert_eq!(
            SendMessageW(state.spotify_search_controls.kind, CB_GETCURSEL, None, None).0,
            7
        );
        windows::Win32::UI::WindowsAndMessaging::SetWindowTextW(
            state.spotify_search_controls.query,
            w!("qa"),
        )?;
        submit_search(window);
        assert_eq!(super::state(window).unwrap().spotify_browse.frames.len(), 2);
        back(window);
        assert!(
            matches!(&top(super::state(window).unwrap()).unwrap().source, Source::MyProfile(uri) if uri == profile_uri)
        );
        fixture.choose(0);
        activate(window);
        assert!(matches!(
            &top(super::state(window).unwrap()).unwrap().source,
            Source::ProfilePeople {
                followers: false,
                ..
            }
        ));
        back(window);
        fixture.choose(1);
        activate(window);
        assert!(matches!(
            &top(super::state(window).unwrap()).unwrap().source,
            Source::ProfilePeople {
                followers: true,
                ..
            }
        ));
        back(window);
        fixture.choose(3);
        activate(window);
        assert!(
            matches!(&top(super::state(window).unwrap()).unwrap().source, Source::ProfilePlaylists(uri) if uri == profile_uri)
        );
        open_root(
            window,
            Source::Section(vec![item(
                ItemKind::Track,
                "spotify:track:0000000000000000000001",
            )]),
            "QA tracks".into(),
        );
        assert!(
            row_menu(super::state(window).unwrap())
                .unwrap()
                .main_menu_pin
                .is_none()
        );
        let track = selected_media(super::state(window).unwrap()).unwrap();
        assert_eq!(
            playing_artist(&track).as_deref(),
            Some("spotify:artist:0000000000000000000001")
        );
        let state = super::state_mut(window).unwrap();
        state
            .spotify_browse
            .artist_states
            .insert(playing_artist(&track).unwrap(), true);
        assert!(commands(state).contains(&C::SpotifyToggleArtistFollow));
        let artist_uri = playing_artist(&track).unwrap();
        state.spotify_browse.marks.toggle(&track.id.0);
        for followed in [false, true] {
            let state = super::state_mut(window).unwrap();
            let stamp = state.spotify.epochs_begin();
            state.spotify_browse.pending = Some((stamp, Pending::Saved));
            edited(
                window,
                stamp,
                Ok(apricot_spotify::EditOutcome::SavedBatch {
                    uris: vec![artist_uri.clone()],
                    saved: followed,
                }),
            );
            let state = super::state(window).unwrap();
            assert_eq!(
                state.spotify_browse.artist_states.get(&artist_uri),
                Some(&followed)
            );
            assert!(
                !state.spotify_browse.marks.is_empty(),
                "following must preserve marked songs"
            );
            let key = if followed {
                "spotify_followed_done"
            } else {
                "spotify_unfollowed_done"
            };
            assert_eq!(
                super::super::window_text(state.status),
                super::super::spotify::catalog(state).text(key)
            );
        }
        assert!(prepare_context(window, LPARAM(-1), false));
        let state = super::state_mut(window).unwrap();
        assert!(row_menu(state).unwrap().artist_followed.is_none());
        let stamp = state.spotify_browse.context_read.as_ref().unwrap().stamp;
        state.spotify_browse.frames.last_mut().unwrap().items[0].uri =
            "spotify:track:0000000000000000000002".into();
        assert!(context_loaded(
            window,
            stamp,
            &Ok(CatalogResult::SavedStates(vec![true]))
        ));
        assert!(
            super::state(window)
                .unwrap()
                .spotify_browse
                .context_ready
                .is_none(),
            "late answer must not reopen a menu"
        );
        let mut artist = item(ItemKind::Artist, "spotify:artist:0000000000000000000001");
        artist.saved = Some(true);
        open_root(
            window,
            Source::Section(vec![artist]),
            "QA artist search".into(),
        );
        let menu = commands(super::state(window).unwrap());
        assert_eq!(
            menu.iter()
                .filter(|command| **command == C::SpotifyToggleSaved)
                .count(),
            1
        );
        assert!(
            !menu.contains(&C::SpotifyToggleArtistFollow),
            "artist rows have exactly one follow command"
        );
        assert!(menu.contains(&C::SpotifyToggleMenuPin));
        assert!(prepare_context(window, LPARAM(-1), false));
        assert!(
            !commands(super::state(window).unwrap()).contains(&C::SpotifyToggleSaved),
            "failed/unknown artist state cannot be guessed"
        );
        super::state_mut(window)
            .unwrap()
            .spotify_browse
            .cancel_context();
        let origin = MenuOrigin::BrowsePage("spotify:page:made-for-you".into());
        let pin = MenuPin::new(
            "Uniquely yours",
            MenuTarget::Section {
                origin: origin.clone(),
                uri: "spotify:section:unique".into(),
            },
        );
        assert_eq!(
            pin_source(&pin).request(0),
            Some(CatalogRequest::MenuSection {
                origin,
                uri: "spotify:section:unique".into()
            })
        );
        super::super::spotify::show_hub(window);
        let selected = super::state(window)
            .unwrap()
            .spotify
            .hub_entries
            .iter()
            .position(|entry| *entry == SpotifyHubEntry::Pinned(0))
            .unwrap();
        fixture.choose(selected);
        toggle_menu_pin(window);
        assert_eq!(service.menu_pins().unwrap().len(), 1);
        assert!(fixture_accounts().accounts[0].is_logged_in());
        let model = SpotifyHubModel::build(
            &super::super::spotify::catalog(super::state(window).unwrap()),
            &fixture_accounts(),
            false,
            &std::collections::BTreeMap::default(),
        );
        assert!(
            model
                .items
                .iter()
                .any(|item| item.entry == SpotifyHubEntry::Browse)
        );
    }
    Ok(())
}
