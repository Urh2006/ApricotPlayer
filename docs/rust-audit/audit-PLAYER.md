# PLAYER area audit — Python vs Rust ApricotPlayer

## 1. Summary

Audit in progress. Focus: player screen, controls, shortcuts, dialogs reachable from
player (details, chapters, bookmarks, transcript, lyrics, comments), edit mode/markers,
output devices, EQ (player side), resume/last-session, autoplay/gapless, and the mpv
launch/command path (Rust: apricot-playback/mpv_process.rs, libmpv.rs vs Python:
apricot/player/mpv.py, playback.py, volume.py, apricot/media/tempo.py).

Overall the default keyboard shortcut table (apricot/constants.py DEFAULT_KEYBOARD_SHORTCUTS)
and the seek/adjustment hold delay constants match the Rust action table and win32.rs
constants exactly, which suggests the shortcut layer was ported very faithfully. The most
significant deviation found so far is architectural: Rust's PlayerSession retains
speed/pitch/output-device/EQ state across track changes within an open session
(Next/Previous/Related), while Python resets speed to the configured default and pitch to
1.0 on every new track. Volume carryover matches (both carry volume across tracks).

## 2. Deviations in ported functionality

### PLAYER-01 (P2): Speed/pitch are NOT reset on track change in Rust, but ARE reset in Python

- Python: `apricot/ui/player.py` `start_mpv()` (lines 44-186), specifically:
  - line 84: `"--pitch=1.0"` — every new mpv process launches with pitch always reset to 1.0.
  - line 63/85: `target_speed = self.player_start_speed_value()` → `apricot/ui/player.py:2063-2071`,
    which reads `self.settings.player_speed` (persisted global setting) or a podcast-speed-preset
    override (`pending_player_speed_override`, set in `apricot/player/playback.py:342` and cleared
    at `apricot/player/playback.py:609`) — NOT the in-session current speed of the previous track.
  - Volume, by contrast, explicitly carries over via `self.session_volume`
    (`apricot/player/volume.py:113-116`, `player_start_volume_value()`), which is only set once
    and reused across tracks in the same app run.
  - Net effect: pressing player_next/player_previous/player_next_related (PageDown/PageUp/Ctrl+Shift+PageDown)
    always resets speed to the configured default (or podcast preset) and pitch to 1.0 for the new
    track, while the (boosted) volume persists.
- Rust: `crates/apricot-app/src/player_session.rs` `start_item_at()` (lines 163-189):
  ```
  if !self.is_open() {
      self.audio = Some(defaults.audio);
      self.enabled_toggles = defaults.enabled_toggles;
  }
  ```
  `is_open()` (line 76) is `self.phase != PlaybackPhase::Closed`. Going to the next/previous/related
  track while playing keeps `phase` non-Closed, so this branch is skipped entirely and the *whole*
  `AudioSession` (`volume`, `output_device`, `speed`, `pitch`, `equalizer`) from the previous track is
  reused unchanged for the new track. `crates/apricot-ui-windows/src/win32.rs:10466-10467` then feeds
  `audio.speed` / `audio.pitch` straight into `MpvLaunchOptions.initial_speed` / `initial_pitch`.
  Compare to `crates/apricot-app/src/application.rs:1631-1652` (`start_player_item_at`), whose
  `defaults.audio.pitch` is hardcoded to `1.0` and `defaults.audio.speed` reads
  `settings.player_speed` — this *is* correct for a genuinely new session, but is bypassed whenever
  a session is already open.
- Impact: An NVDA user who slows a track down with S/speeds up with D, then presses Next/Previous to
  move to another item in the same session, will keep hearing the adjusted speed/pitch in Rust,
  whereas Python quietly resets to the user's configured default speed and pitch=1.0 for every new
  item. This is a real, user-noticeable behavioural difference for anyone using speed/pitch controls
  in a playlist/queue/search-results context.
- Suggested fix: In Rust `start_item_at`/`start_player_item_at`, when starting a *different* media
  item (not just a resume of the same one), reset `audio.speed` to `settings.player_speed` and
  `audio.pitch` to `1.0` (keep `volume`, `output_device`, `equalizer` carried over, matching Python),
  or confirm with the user whether this carryover is an intentional UX improvement — if so it should
  be documented as an approved deviation, since it is currently undocumented.

### PLAYER-02 (P3, needs runtime verification): mpv started with `--idle=yes` in Rust vs `--idle=no` in Python; stream URL passed differently

- Python: `apricot/player/mpv.py:80` uses `"--idle=no"` and appends the stream URL as the last
  positional CLI argument (`apricot/player/mpv.py:140`, `args.append(stream_url)`). With idle=no, if
  mpv cannot open the URL, the process exits immediately and Python's monitor thread observes process
  death as the failure signal.
- Rust: `crates/apricot-playback/src/mpv_process.rs:325` uses `"--idle=yes"` and never appends a URL to
  the argv (see `launch_arguments()` at lines 313-393) — it presumably issues a `loadfile` IPC command
  after the idle process starts (not located in the excerpt read). Comment at line 129: "Starts an idle
  mpv process and its bounded event monitor."
- Impact: functionally can be equivalent if Rust reliably detects load failures via IPC `end-file`
  events instead of process exit, but this needs runtime verification: if the IPC pipe fails to
  connect before `loadfile` is sent, or if `loadfile` errors are not correctly translated into a
  user-facing "player_failed" style announcement, the user could be left in a silent idle mpv window
  with no announcement, whereas Python always surfaces `self.t("player_failed", error=exc)` on launch
  failure (`apricot/player/mpv.py:186`) or process exit soon after.
- Needs runtime verification: intentionally play an invalid/unreachable URL and a corrupted local file
  in both builds and confirm identical NVDA announcement text/timing.

### PLAYER-03 (P1): Player context menu is missing most player-specific items present in Python

- Python: `apricot/ui/menus.py` `open_player_context_menu()` (lines 323-378) builds, in order
  (conditionally on local-media / YouTube / rss item):
  download_audio, download_video (remote only), add_favorite, remove_favorite,
  subscribe_channel, unsubscribe_channel, open_channel (remote+YouTube-channel only),
  add_to_playback_queue, remove_from_playback_queue, remove_from_playlist,
  copy_path/copy_link, copy_stream_url (remote only), copy_timestamp_link (remote+YouTube
  only), **output_devices**, **fullscreen**, **equalizer**, **audio_normalization
  (replaygain cycle)**, **save_podcast_speed_preset** (rss items only),
  **play_related_video** (YouTube only), **add_bookmark**, **bookmarks**, **chapters**,
  **transcript**, **lyrics**, **comments** (remote only), **open_browser** (remote only),
  close_player, and finally an **add-to-playlist submenu** listing the user's actual
  playlists by name (`apricot/library/library.py:348-359`,
  `append_add_to_playlist_menu`), or a flat "add to playlist" item if none exist yet.
- Rust: `crates/apricot-ui-windows/src/win32.rs` `show_player_context_menu()` (lines
  2187-2293) builds only: show_video_details (not in Python's menu at all — see below),
  add/remove favorite, subscribe/unsubscribe, add_to_playback_queue,
  remove_from_playback_queue, playback_queue (not in Python's player menu either),
  add_to_playlist (flat, no submenu of actual playlists), remove_from_playlist,
  copy_path/copy_link, download_audio/download_video (remote only), copy_stream_url
  (remote only), copy_timestamp_link (remote+YouTube only), close_player.
- **Entirely missing from the Rust player context menu**: output_devices, fullscreen,
  equalizer, audio_normalization/replaygain, save_podcast_speed_preset, play_related_video,
  add_bookmark, bookmarks, chapters, transcript, lyrics, comments, open_browser. Searched
  `crates/apricot-ui-windows/src/win32.rs` for `ID_CONTEXT_` constants and
  `show_player_context_menu` body; none of these are appended to the popup menu (they are
  only reachable via their keyboard shortcuts, e.g. Ctrl+Shift+C for chapters, F4 for
  equalizer, O for output devices, Ctrl+Shift+B/K for bookmarks).
- Also: Rust always shows "show_video_details" and "playback_queue" as menu items which
  Python's player context menu does not include (Python exposes video details via F7 and
  the global "open_playback_queue" shortcut/menu instead, not from *this* menu).
- Also: Rust's "add_to_playlist" is a single flat command, not the submenu-of-existing-
  playlists Python provides via `append_add_to_playlist_menu`, so NVDA users cannot pick a
  specific existing playlist directly from the player context menu in Rust — they get
  routed into a different flow (needs runtime verification for exactly what Rust's
  `add_active_item_to_user_playlist` does, e.g. does it open a picker dialog).
- Impact: This is a major discoverability regression for NVDA/keyboard users who rely on
  Shift+F10 / Applications-key context menus to discover available player actions instead
  of memorizing ~30 shortcuts. Bookmarks, chapters, transcript, lyrics, comments, EQ, and
  output-device selection — all major accessible-player features — are invisible in the
  Rust player context menu.
- Suggested fix: Add the missing entries to `show_player_context_menu` in the same
  conditional order as Python's `open_player_context_menu`, including a playlist submenu
  built from the user's actual playlists (mirroring `append_add_to_playlist_menu`).

## 3. Missing functionality

- PLAYER-M-01 (P1): Player context-menu items for output devices, fullscreen, equalizer,
  replaygain/audio-normalization, save-podcast-speed-preset, play-related-video,
  add-bookmark, bookmarks, chapters, transcript, lyrics, comments, and open-in-browser are
  entirely absent from Rust's `show_player_context_menu`
  (`crates/apricot-ui-windows/src/win32.rs:2187-2293`). Python reference:
  `apricot/ui/menus.py:323-378`. (Same as PLAYER-03; listed here too since it constitutes
  missing menu-level functionality, not just a reordering.)
- PLAYER-M-02 (P3): The player context menu's "add to playlist" is a flat single command in
  Rust instead of Python's inline submenu of actual playlist names
  (`apricot/library/library.py:348-359` vs `crates/apricot-ui-windows/src/win32.rs:2214`
  `ID_CONTEXT_ADD_TO_PLAYLIST`). Confirmed Rust's handler `add_active_item_to_user_playlist`
  (`crates/apricot-ui-windows/src/win32.rs:11808-1828`) opens a separate `choose_user_playlist`
  picker dialog when more than one playlist exists (or creates one if none exist), so the
  functionality is present but via an extra dialog hop instead of Python's single-level
  submenu — a flow/efficiency difference rather than lost functionality. Downgraded to P3.

## 4. Verified as matching

- Default keyboard shortcut table: `apricot/constants.py:635-727` (`DEFAULT_KEYBOARD_SHORTCUTS`)
  matches `crates/apricot-core/src/action.rs:111-174` key-for-key for all `player_*` actions checked.
- Seek-hold timing: Python `apricot/ui/player.py:1915` (180ms initial) / `:1963` (110ms repeat, hardcoded)
  matches Rust `crates/apricot-ui-windows/src/win32.rs:220-221`
  (`SEEK_HOLD_DELAY_MS = 180`, `SEEK_HOLD_INTERVAL_MS = 110`).
- Speed/pitch adjustment-hold timing settings: Python `apricot/constants.py:570-575`
  (`SPEED_PITCH_HOLD_DELAY_DEFAULT_MS=180`, min 50/max 1000; `INTERVAL_DEFAULT_MS=110`, min 20/max 500)
  matches Rust `crates/apricot-storage/src/settings.rs:198-199,366-367` defaults and clamps.
- mpv launch flags largely match 1:1: `--no-config`, `--force-window`, `--input-ipc-server`,
  `--keep-open=yes`, `--volume-max`, `--volume`, `--speed`, `--loop-file`, `--gapless-audio`,
  `--replaygain`/`--replaygain-clip=yes`, `--term-playing-msg=`, `--msg-level=all=warn`, `--wid`,
  `--vid=no`, `--pause=yes`, `--audio-device`, cache flags (`--cache=yes/no`,
  `--demuxer-max-bytes`, `--demuxer-max-back-bytes`, `--demuxer-readahead-secs=30`,
  `--stream-lavf-o=reconnect=...`), `--af=` initial EQ filter.

- Player context menu (Shift+F10 / Applications key) both correctly wired via `WM_CONTEXTMENU`
  in Rust (`crates/apricot-ui-windows/src/win32.rs:1666,2181,2984`); matches Python's dual
  binding (Applications-key shortcut default `"Applications"` in
  `apricot/constants.py:671` handled the same way natively by wx/MSAA).
- Lyrics dialog: Rust (`crates/apricot-ui-windows/src/win32.rs:12961-13026`, using
  `crate::details_win32::show_async` + `highlight_lyrics` at
  `crates/apricot-ui-windows/src/details_win32.rs:373-432`) implements the same
  local/online source lookup and live active-line highlighting as Python's timer-based
  highlighting in `apricot/ui/misc.py:1978-2100+` (`show_lyrics`).
- Transcript dialog: Rust (`crates/apricot-ui-windows/src/transcript_win32.rs`) has the
  same control set as Python (`apricot/ui/misc.py:1805-1840`): search box, list, Play/jump,
  copy line, copy all, copy timestamp link, back — including correctly disabling
  copy/jump buttons when there are no visible entries
  (`transcript_win32.rs:279-292` vs Python's `jump_button.Enable(False)` etc. at
  `apricot/ui/misc.py:1837-1840`).
- Bookmarks: `show_player_bookmarks` → filtered to the current item only
  (`current_only=True`) matches between Python (`apricot/ui/misc.py:1507-1510`,
  `show_bookmarks_dialog(current_only=True)`) and Rust
  (`crates/apricot-ui-windows/src/win32.rs:11303-11420`, `bookmark_dialog_entries`
  branches on `current_only`).

## 5. Needs runtime/NVDA verification

- PLAYER-02 idle/loadfile failure-path announcement parity (see above).
- PLAYER-01: confirm in a running build that Next/Previous/Related during an open player
  session in Rust indeed keeps an adjusted speed/pitch from the prior track, vs Python
  resetting to defaults — static analysis strongly indicates this but was not run live.
- PLAYER-03/M-01: confirm live that none of output_devices/fullscreen/equalizer/replaygain/
  chapters/transcript/lyrics/bookmarks/comments/open_browser appear anywhere in the actual
  rendered Rust player context menu (source-level search is conclusive for the code path
  reached via Shift+F10/Applications key on `MainView::Player`, but worth a manual check in
  case another code path augments the menu).
- Did not reach in this pass (time-boxed): full comparison of announcement text for T
  (time), V (volume status), F (format status), B (BPM) responses against Python locale
  strings; the player controls Tab order (`player_tab_order` in
  `apricot/ui/player.py:856-889`) vs Rust's tab-order implementation; details dialog
  (F7) exact text/field ordering; comments dialog; edit mode and marker/clip
  preview/export flow; output-device selection dialog; equalizer dialog opened from the
  player (F4); resume-position/last-session restore; autoplay/gapless; and a full
  line-by-line diff of the mpv af (audio filter) chain construction for EQ/bass-boost/
  volume-boost/clipping-protection beyond the initial launch-arg comparison already done.
  These should be covered by a follow-up pass if the above cannot be fully closed by other
  agents' overlapping areas.
