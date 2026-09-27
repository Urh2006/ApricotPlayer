# SHELL Audit — Application Shell & Global Navigation

Status: COMPLETE (time-boxed; see "Needs runtime verification" for follow-ups)

## 1. Summary

The default keyboard-shortcut table (91 actions) and the main-menu item catalog/order are ported faithfully and match Python exactly. The single biggest defect is structural: the Rust global shortcut/action dispatcher (`activate_action` in win32.rs) is missing match arms for 16 of the 91 registered actions, including the entire AudioVault screen, the diagnostic-report copier, and several player features (equalizer, fullscreen, output devices, comments, edit mode, replaygain, shuffle, related-video, result-column navigation) — all of which currently pop a "not implemented in this internal build yet" MessageBox instead of working, for both keyboard shortcuts and the native player buttons that route through the same dispatcher. The app self-update pipeline (pending-update main-menu item + install flow) has settings scaffolding but no actual implementation. The screen-reader announcement path lacks the JAWS-specific fallback Python has. The first-run language dialog doesn't respect a background/tray-start launch the way Python does. Main-menu ordering, customization, shortcut labels, and the NVDA controller-DLL integration all check out as matching.

Counts: P1 deviations/missing: 2 (SHELL-01, SHELL-M-01) + SHELL-M-03 diagnostic report is also P1 = 3 P1 total. P2: 3 (SHELL-02, SHELL-M-02, SHELL-M-04). P3: 1 (SHELL-03).

## 2. Deviations in ported functionality

### SHELL-01 (P1) — `activate_action` dispatch table is missing 16 of 91 registered actions; they silently show a "not implemented" MessageBox instead of doing anything

Python: every action in `DEFAULT_KEYBOARD_SHORTCUTS` (apricot/constants.py:635-727, 91 entries) has a working handler reachable via `handle_global_navigation_shortcut` (apricot/ui/shortcuts.py:481) or `handle_player_shortcut_event` (apricot/ui/shortcuts.py:520), and via menu/button items in menus.py.

Rust: `crates/apricot-core/src/action.rs` `ACTIONS` const also declares 91 actions (test `baseline_contains_91_unique_actions` asserts this), but the central keyboard-shortcut executor `activate_action()` in `crates/apricot-ui-windows/src/win32.rs:10760-10839` has match arms for only 78 of them (`_ => show_unimplemented_action(window, action_id)` for the rest, defined at win32.rs:13548, which pops a `MessageBoxW` saying "... is registered, but its Rust route is not implemented in this internal build yet."). Confirmed via `grep` that none of the following action ids appear anywhere else in win32.rs either (i.e. there is no alternate code path that handles them):

- `open_audiovault` (Ctrl+Alt+A) — the AudioVault main-menu item itself
- `open_channel` (Ctrl+Shift+O)
- `copy_diagnostic_report` (Ctrl+Alt+Shift+D)
- `player_bpm` (B)
- `player_comments` (Ctrl+Shift+M)
- `player_edit_mode` (E)
- `player_equalizer` (F4)
- `player_fullscreen` (F11)
- `player_next_related` (Ctrl+Shift+PageDown)
- `player_output_devices` (O)
- `player_replace_edit_original` (Ctrl+R)
- `player_replaygain` (Ctrl+Shift+G)
- `player_save_edit_copy` (Ctrl+S)
- `player_shuffle` (Shift+S)
- `result_column_previous` (Ctrl+Alt+Left)
- `result_column_next` (Ctrl+Alt+Right)

Importantly, the player-screen native buttons (`equalizer`, `output_devices`, `comments`, `edit_mode`, `audio_normalization`, and the `fullscreen` checkbox — `crates/apricot-app/src/player_model.rs:139-222`, wired up in `crates/apricot-ui-windows/src/player_controls_win32.rs:40-65`) are built with `action_id: Some("player_equalizer")` etc. and route their clicks through the exact same `activate_action()` — see `player_controls_win32.rs:169,251`. So this is not just a keyboard-shortcut gap: **clicking these buttons with the mouse or activating them via NVDA also produces the "not implemented" MessageBox**, for both keyboard and screen-reader/mouse users.

Suggested fix: add match arms in `activate_action()` (win32.rs) for all 16 ids, wiring them to real implementations (some — equalizer, output devices, comments, edit mode, fullscreen — clearly have partially-built supporting UI/state elsewhere that just isn't reachable yet; others — AudioVault screen, diagnostic report, column navigation, shuffle, related-video, replace-original-save — appear to need net-new implementation; see Missing functionality section).

### SHELL-02 (P2) — First-run language dialog ignores "started hidden in tray" and shows even on a silent/background launch
Python: `wx_main.py:331` — the one-time language prompt (`prompt_initial_language`, apricot/ui/dialogs.py:39) is gated by `self.first_run_without_settings and not self.settings.language_prompted and not self.started_hidden_in_tray` — i.e. it is suppressed when the app was started hidden (e.g. "start with Windows" launching to tray).

Rust: `apps/apricot-player/src/main.rs:103` gates the equivalent (`apricot_ui_windows::choose_initial_language`) with only `if !application.settings().language_prompted`. The `start_hidden` flag computed at `main.rs:23` (from `--start-in-tray`) is never consulted for this decision (it's only used later for `run_application`). Result: a genuinely first-run install that auto-starts hidden in the tray will still pop a modal, focus-stealing native "Language" window instead of starting silently — the opposite of what "start hidden" is supposed to do.

Suggested fix: thread `start_hidden` into the `language_prompted` gate at main.rs:103, matching Python's three-part condition (also consider the `first_run_without_settings` distinction, noted as P3 below).

### SHELL-03 (P3) — First-run language dialog gate does not distinguish "true first run" from "upgraded settings file lacking the flag"
Python: `wx_main.py:67` computes `first_run_without_settings = not SETTINGS_FILE.exists() and not LEGACY_SETTINGS_FILE.exists()` and requires it (together with `language_prompted` false) before prompting.
Rust: `main.rs:103` relies solely on `settings().language_prompted`. Low impact in practice (Rust settings always default `language_prompted` appropriately for its own history), flagged for completeness — needs runtime verification against an actual upgrade/migration scenario.

## 3. Missing functionality

### SHELL-M-01 (P1) — AudioVault screen family entirely unimplemented in the Win32 UI; main-menu item and Ctrl+Alt+A shortcut lead nowhere
Python: `show_audiovault_menu` is a first-class main-menu destination (apricot/ui/menus.py:61, `MAIN_MENU_CUSTOMIZABLE_ITEMS` "audiovault" entry, apricot/constants.py:618), with its own shortcut `open_audiovault` = Ctrl+Alt+A (constants.py:638) and full screens (menu/search/results/episodes/login) in apricot/ui code.

Rust: `apricot-core/src/screen.rs:148-164` declares the screen catalog entries (`audiovault_menu`, `audiovault_search`, `audiovault_results`, `audiovault_episodes`, `audiovault_login`) and `apricot-app/src/main_menu.rs:224` maps the main-menu id to the `open_audiovault` action, so the menu item is listed and clickable/selectable — but the concrete Win32 `MainView` enum (`crates/apricot-ui-windows/src/win32.rs:258-278`) has no AudioVault-related variant at all, and `activate_action()` (win32.rs:10760) has no `"open_audiovault"` arm (see SHELL-01). Selecting the AudioVault item in the main menu, or pressing Ctrl+Alt+A anywhere, produces the "not implemented" MessageBox (win32.rs:13548) instead of opening any screen.

### SHELL-M-02 (P2) — App self-update flow (pending-update banner menu item + install) is entirely absent
Python: apricot/ui/system.py:160-191 (`pending_app_update_version`, `open_pending_app_update`) plus apricot/updater/updater.py implement a full self-update pipeline: periodic GitHub-release check, a dynamic "Update to vX.Y.Z" item inserted at the very top of the main menu when a release is staged (apricot/ui/menus.py:78-79), a confirm/skip prompt, and installing + relaunching.

Rust: settings plumbing exists (`app_update_interval_hours`, `app_update_notifications`, `skipped_update_version` — `crates/apricot-storage/src/settings.rs:107-109`, exposed in Settings UI via `crates/apricot-app/src/settings_model.rs:342-355`) but there is no code anywhere in `apricot-app`, `apricot-ui-windows`, or `apps/apricot-player` that checks for releases, stages a pending version, inserts the top-of-menu update item, or installs anything (`apricot-updater` crate only implements `check_youtube_components`, unrelated to app self-update — `crates/apricot-updater/src/lib.rs:22-28`). The Settings screen's "Check app updates now" button and update-interval/notification toggles therefore currently do nothing observable. Needs runtime verification of the Settings button's actual behavior, but no supporting implementation was found by search.

### SHELL-M-03 (P1) — Diagnostic report copy (Ctrl+Alt+Shift+D / main-menu "Copy diagnostic report") produces no report
Python: apricot/system/diagnostics.py — `DiagnosticsMixin` builds a multi-section report (app info, player state, audio/output device, current item, download queue, settings, log tails, yt-dlp/player/ffmpeg versions, equalizer gains, URL/PII redaction) and copies it to the clipboard, reachable from the main menu (apricot/ui/menus.py:74) and Action Finder (menus.py:132), with default shortcut Ctrl+Alt+Shift+D.
Rust: the main-menu item exists and is wired to the `copy_diagnostic_report` action id (`crates/apricot-app/src/main_menu.rs:235`), but no code builds or copies any diagnostic text anywhere in the Rust workspace (searched all of `apricot-app`/`apricot-ui-windows` for `diagnostic`/`diagnostics`: only that one mapping line exists) and `activate_action()` has no arm for it (SHELL-01), so it falls to the "not implemented" MessageBox.

### SHELL-M-04 (P2) — JAWS-specific announcement path (COM `SayString`) is missing from the Rust announcer; only NVDA-controller and generic WinEvents exist
Python: `apricot/ui/misc.py:522-566` (`speak_text`) tries, in order: (1) NVDA controller DLL `nvdaController_speakText`/`brailleMessage`, (2) `_jaws_speak_ctypes` (JAWS COM API) if NVDA didn't announce, (3) generic MSAA WinEvents (`EVENT_OBJECT_NAMECHANGE` always, `EVENT_SYSTEM_ALERT` unless already announced by NVDA/JAWS, `EVENT_OBJECT_VALUECHANGE` on the status control) for Narrator/other IAccessible screen readers.

Rust: `crates/apricot-ui-windows/src/announcement_win32.rs` (`WindowsAnnouncer::announce`, line 86) only tries (1) NVDA controller DLL, then falls back directly to (3) `SetWindowTextW` + `NotifyWinEvent(EVENT_OBJECT_NAMECHANGE, ...)`. There is no JAWS COM `SayString` call anywhere in the Rust workspace (searched for "jaws"/"JAWS", no hits), and the WinEvents fallback never raises `EVENT_SYSTEM_ALERT` or a `VALUECHANGE` event, only `NAMECHANGE`.

Impact: a JAWS-only user (NVDA not installed/running) gets no direct screen-reader speech call from the app at all — only whatever JAWS infers from the generic MSAA name-change event, which may be missed or delayed depending on control focus. Needs runtime verification with actual JAWS, but the code-level gap is clear. Priority P2 because most of this app's target audience is NVDA-first, but JAWS users would notice a real regression.

## 4. Verified as matching

- **Default shortcut table**: all 91 actions in Python's `DEFAULT_KEYBOARD_SHORTCUTS` (apricot/constants.py:635-727) have an identically-named, identically-chorded counterpart in Rust's `ACTIONS` const (`crates/apricot-core/src/action.rs:64-175`, asserted by its own test to contain exactly 91). Spot-checked every chord string; all matched.
- **Main menu item order and customizability**: Python's `MAIN_MENU_CUSTOMIZABLE_ITEMS` (constants.py:612-632, 19 entries) order is reproduced exactly by Rust's `CUSTOMIZABLE_MAIN_MENU` (`crates/apricot-core/src/menu.rs:9-…`), including the "search" item's special `"X / Y"` label composition (menus.py:7-8 vs `menu.rs` `menu_label`/`main_menu.rs:207-217`), per-item count suffixes for `current_downloads`/`playback_queue`, and conditional visibility for `resume_last_session`/`trending`/`history`/`rss_feeds`. Settings/Exit are permanently appended last in both. Confirmed via Rust's own test `default_menu_matches_python_availability_and_permanent_items`.
- **Applications key / Shift+F10 context-menu opening**: Python explicitly special-cases Shift+F10 in `context_menu_shortcut_matches` (apricot/ui/shortcuts.py:399-405) because wx does not natively translate it; Rust relies on the native Win32 `WM_CONTEXTMENU` message (handled at `crates/apricot-ui-windows/src/win32.rs:1666`), which the OS itself raises for both the Applications/Menu key and Shift+F10 on standard controls — a different mechanism reaching the same behavior. Marked as needing runtime confirmation since it wasn't executed.
- **NVDA controller announcement path** (primary path): both use `nvdaController_speakText`/`nvdaController_brailleMessage`/`nvdaController_cancelSpeech` from the vendored `nvdaControllerClient64.dll`, with equivalent candidate search paths (bundled `nvda/` folder next to the executable, then Program Files\NVDA). See SHELL-M-04 for the JAWS/WinEvents fallback-tier gap.
- **First-run language dialog core flow**: language list from `LANGUAGES`, single-selection modal, writes `settings.language`, sets `language_prompted = true`, shown before the main window/menu — matches on the happy path; see SHELL-02/SHELL-03 for gating differences.

## 5. Needs runtime/NVDA verification

- Whether the 16 `activate_action` gaps (SHELL-01) genuinely have zero user-facing recourse (e.g. confirm no separate keyboard path bypasses `activate_action` for fullscreen/equalizer/output-devices via direct WM_COMMAND from the native player buttons — code inspection strongly suggests they all funnel through the same dispatcher, but a live click/keypress trace on the running app would remove all doubt).
- Whether Shift+F10 actually raises `WM_CONTEXTMENU` for every list/player control in the Rust build the way it does for standard Win32 list boxes (custom-drawn or owner-draw controls sometimes need to request it explicitly).
- Whether JAWS is actually available in this environment to confirm SHELL-M-04's real-world impact (no JAWS COM `SayString` call exists in Rust; effect for JAWS-only users needs a live JAWS session).
- Whether the Settings screen's "Check app updates now" button in Rust silently no-ops or errors, given SHELL-M-02 found no supporting implementation.
- Exact focus-restoration behavior returning to the main menu after Escape/Back from various screens — inspected `navigate_back` structurally but did not trace every route's focus target against Python's per-screen `leave_*_to_main_menu` equivalents in this pass (time-boxed).
