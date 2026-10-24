# PLAYER2 area audit — Python vs Rust ApricotPlayer (follow-up pass)

Covers items left by the previous PLAYER auditor: PLAYER-01 re-verification, exact
announcement texts/locale keys, player window tab order/initial focus, F7/comments/output
device/F4 EQ dialogs from player, edit mode/markers/clip preview/export, resume/autoplay/
gapless/repeat/shuffle, and the full af (audio filter) chain construction.

## 1. Summary

This pass re-verified the first PLAYER auditor's PLAYER-01 claim precisely (confirmed correct)
and then covered everything it explicitly deferred. The headline finding is that a surprisingly
large slice of player-reachable functionality that is fully implemented in Python and registered
as keyboard actions in Rust (with matching shortcuts and visible buttons) has **no actual
handler** in the Rust dispatcher and falls through to a generic `show_unimplemented_action()`
that pops a blocking, non-localized "not implemented" MessageBoxW: the F4 equalizer dialog, the O
output-device dialog, the Ctrl+Shift+M comments dialog, the B BPM shortcut, the Shift+S shuffle
toggle, and the entire E/Ctrl+S/Ctrl+R local-file "edit mode" bake-in-speed/EQ pipeline (8 actions
total). Separately, a whole family of player status announcements (T time, V volume, speed/pitch
change, speed/pitch reset, repeat/volume-boost/bass-boost toggle) are hardcoded English strings
that bypass the locale catalog entirely, unlike play/pause, playback-finished, no-next/previous,
and format-status announcements which are correctly localized -- so localization was clearly
intended but inconsistently applied. The "Audio quality when changing speed" setting
(rubberband/scaletempo2/scaletempo/mpv) is stored and shown in Settings but never actually applied
to the mpv process, so all speed-changed playback silently uses mpv's stock algorithm regardless
of user choice. On the positive side: the core PLAYER-01 speed/pitch-carryover bug is confirmed
precisely as described by the first auditor; player tab order and initial focus are a faithful,
control-for-control match; resume-position logic (5s start / 8s-from-end clear thresholds) is a
verified, well-tested port; the equalizer `af` filter-string construction (band widths, limiter,
clipping headroom) matches almost exactly; and marker/clip-preview/export announcements are
correctly localized.

## 2. Deviations in ported functionality

### PLAYER2-01 (P1): Re-verified PLAYER-01 exactly -- speed/pitch persist across tracks in Rust, always reset in Python

Precise re-check with corrected Python citation (the function lives in `apricot/player/mpv.py`,
not `player.py` as the first auditor wrote -- `apricot/ui/player.py` only calls it via a bound
mixin method at line 411):

- Python `apricot/player/mpv.py:44-176` `start_mpv()` is called for every new item start
  (initial play, Next, Previous, Related -- there is no "already in a session" branch at all).
  Every call unconditionally does:
  - line 63: `target_speed = self.player_start_speed_value()`
  - line 84: `"--pitch=1.0",` (hardcoded literal, no conditional)
  - line 85: `f"--speed={target_speed:g}",`
  - `player_start_speed_value()` (`apricot/ui/player.py:2063-2071`) reads
    `pending_player_speed_override` (a one-shot podcast-speed-preset field cleared after use,
    `apricot/player/playback.py:342,609`) or else `self.settings.player_speed` (the persisted
    global default) -- never the previous track's live/adjusted speed.
  - Volume is the only session value that intentionally carries over, via
    `self.session_volume` / `player_start_volume_value()` (`apricot/player/volume.py:113-116`).
  - Net: Python always resets speed and pitch to configured defaults on every new track.
- Rust `crates/apricot-app/src/player_session.rs:164-192` `start_item_at()`:
  `if !self.is_open() { self.audio = Some(defaults.audio); self.enabled_toggles = defaults.enabled_toggles; }`
  `is_open()` (line 76-78) is `self.phase != PlaybackPhase::Closed`. Next/Previous/Related keep
  `phase` non-`Closed`, so this branch -- and therefore the whole `defaults.audio` (speed, pitch,
  output device, equalizer) -- is skipped, and the entire previous `AudioSession` carries over
  unchanged into the new track.
  `crates/apricot-app/src/application.rs:1620-1660` `start_player_item_at()` builds `defaults`
  correctly (`pitch: 1.0` hardcoded at line 1645, `speed: settings.player_speed.parse()...` at
  line 1644) -- but that computation is thrown away by the `is_open()` guard whenever a session
  is already open.
- Confirmed: this is a real, reproducible behavioral difference, not a citation error. Priority
  raised to P1 from the first auditor's P2 because it silently carries over output_device and
  equalizer state too (not just speed/pitch) across tracks -- e.g. a user who selects a
  non-default output device or enables the equalizer for one track keeps it (arguably desired),
  but combined with unwanted speed/pitch carryover this is an inconsistent, undocumented mix of
  "sticky" vs "per-track" state with no single coherent design rationale, diverging from
  Python's clean "only volume is sticky" model.
- Suggested fix: reset `audio.speed`/`audio.pitch` to defaults on every new-item start (keep
  volume), and decide+document explicitly whether output_device/equalizer are meant to be sticky.

### PLAYER2-02 (P1): Whole family of player status announcements are hardcoded, non-localized English in Rust, with wording/format that also differs from Python's locale strings

Grepped the full Rust tree for the exact locale keys Python uses for T/V/B/speed/pitch/reset/
repeat/shuffle/boost announcements (`time_announcement`, `volume_announcement`,
`bpm_announcement`, `speed_announcement`, `pitch_announcement`, `speed_pitch_reset`,
`repeat_on/off`, `shuffle_on/off`, `volume_boost_on/off`, `bass_boost_on/off`,
`seeked_to_start/end`, `bpm_not_available`, `bpm_analyzing`) -- none of these keys appear
anywhere in the Rust codebase. By contrast `format_status_*`, `playback_paused`,
`playback_playing`, `playback_finished`, `no_previous_item`, `no_next_item`, and
`timing_unavailable` are correctly routed through the locale catalog
(`catalog_text`/`.text()`) in `crates/apricot-app/src/player_information.rs` and
`crates/apricot-ui-windows/src/win32.rs`. So localization was clearly intended and implemented
for some player announcements but not others.

Concrete hardcoded-English call sites found in `crates/apricot-ui-windows/src/win32.rs`:

| Action (shortcut) | Python locale key : text | Rust code | Rust text |
|---|---|---|---|
| player_time (T) | `time_announcement`: "Elapsed {elapsed}, remaining {remaining}, total {total}." | `announce_player_time` (line 12428-12445) | `format!("Elapsed {elapsed}, remaining {remaining}, total {}", ...)` -- no trailing period; when duration is unknown Rust prints `"Elapsed {elapsed}"` alone (partial info) whereas Python announces `timing_unavailable` ("Timing is not available yet.") instead |
| player_volume_status (V) | `volume_announcement`: "Volume: {volume}" | `announce_player_volume` (line 12756-12764) | `format!("Volume {:.0}", audio.volume)` -- missing colon, not localized |
| player_speed_up/down (D/S) | `speed_announcement`: "Playback speed {speed}x." | `adjust_player_speed` (line 13055-13068) | `format!("Speed {speed:.2}")` -- different wording, no "x" suffix, always 2 decimals (e.g. "Speed 1.25" vs Python's rounded "Playback speed 1.3x.") |
| player_pitch_up/down (Ctrl+Up/Down) | `pitch_announcement`: "Pitch {pitch}x." | `adjust_player_pitch` (line 13070-13083) | `format!("Pitch {pitch:.2}")` -- same issues |
| player_reset_speed_pitch (Ctrl+0) | `speed_pitch_reset`: "Speed reset to {speed}x and pitch reset to {pitch}x." | `reset_player_speed_pitch` (line 13085-13106) | literal `"Speed and pitch reset"` -- no values announced at all |
| player_repeat (R) | `repeat_on`/`repeat_off`: "Repeat on."/"Repeat off." | `toggle_player_session_setting` (line 13108-13166) via `session_toggle_name()` (13168-13177) | `format!("{} {}", "Repeat", "on"/"off")` -> "Repeat on"/"Repeat off" (no periods, not localized) |
| player_volume_boost | `volume_boost_on`/`volume_boost_off`: "Volume boost on."/"Volume boost off." | same `toggle_player_session_setting` path | "Volume boost on"/"Volume boost off" (no periods, not localized) |
| player_bass_boost | `bass_boost_on`/`bass_boost_off`: "Bass boost on."/"Bass boost off." | same path | "Bass boost on"/"Bass boost off" (no periods, not localized) |

- Impact: for any non-English `settings.language`, an NVDA user pressing T/V/S/D/Ctrl+Up/Ctrl+
  Down/Ctrl+0/R/player_volume_boost/player_bass_boost hears raw English text instead of their
  configured language, unlike every other player announcement (play/pause, playback finished,
  no next/previous item, format status) which are correctly localized. Even for English-language
  users the wording, punctuation, and precision differ from Python's spec-defined strings, and the
  reset-speed-pitch announcement in Rust loses the actual resulting values entirely.
- Suggested fix: route all of these through `catalog_text(&state.application, "<key>")` /
  `catalog.text("<key>", ...)` with the same locale keys and argument substitution as Python,
  matching decimal formatting to `format_playback_rate`/`format_rate_for_speech` semantics.

### PLAYER2-03 (P1): Shift+S (player_shuffle) has no dispatch handler in Rust -- the shortcut is a silent no-op

- Python `apricot/ui/misc.py:1358-1360` `toggle_shuffle()`: flips `self.shuffle_current` and
  immediately announces `shuffle_on`/`shuffle_off` ("Shuffle on."/"Shuffle off."), independent of
  whether a player session is open -- it is a general queue/sequence setting.
- Rust: `player_shuffle` is registered as an action (`crates/apricot-core/src/action.rs:149`,
  default shortcut `Shift+S`, matching Python's `apricot/constants.py` table), and
  `SessionToggle::Shuffle` exists in the toggle enum and is referenced in
  `crates/apricot-ui-windows/src/win32.rs:13129,13174` (display name, and treated as
  "always succeeds" in the toggle-command match) -- but there is no `"player_shuffle" =>` arm
  anywhere in the action dispatch `match` in `win32.rs` (confirmed via full-file grep: the only
  occurrences of `"player_shuffle"` in the whole Rust tree are the single action-table
  registration in `action.rs`). Pressing Shift+S therefore falls through to whatever the
  dispatcher's default arm does (presumably nothing / "unhandled action") -- shuffle can only be
  set via `start_player_item_with_shuffle_at` (`application.rs:1662-1682`), which is reachable
  from list/queue "play with shuffle" entry points, not from a live in-player toggle.
- Correction after tracing the dispatcher's fallback arm: `activate_action()`
  (`win32.rs:10760-10839`) ends with `_ => show_unimplemented_action(window, action_id)`
  (line 10837), so `player_shuffle` does **not** silently do nothing -- it pops a blocking
  `MessageBoxW` (see PLAYER2-04 below for exact text/behavior). This is arguably a *worse* NVDA
  experience than a silent no-op: a modal system dialog interrupts playback and must be dismissed
  with Enter/Escape every time the user presses Shift+S, rather than either working or being
  quietly ignored.
- Impact: an NVDA user who wants to toggle shuffle on/off while a track is already playing (the
  primary documented use of this shortcut) cannot do so in Rust -- instead every press pops an
  interrupting, non-localized "not implemented" message box.
- Suggested fix: add a `"player_shuffle" => toggle_player_session_setting(window,
  SessionToggle::Shuffle)` arm (or equivalent), and give it a real command effect (currently
  `SessionToggle::Shuffle` is one of the toggles that trivially "always succeeds" with no actual
  backing command at line 13129 -- verify toggling it live actually changes upcoming-track
  selection order, not just the announced/displayed state).

### PLAYER2-04 (P1, Missing): F4 equalizer dialog, O output-device dialog, Ctrl+Shift+M comments dialog, B BPM, and Shift+S shuffle are ALL unimplemented placeholders in Rust -- each pops an English "not implemented" MessageBox

Systematically grepped the entire Rust tree (not just `win32.rs`) for a real handler/dialog
implementation for every player-reachable feature, using both the action id string and any
plausible function name:

- `player_equalizer` (F4): action registered (`crates/apricot-core/src/action.rs:125`, shortcut
  `F4`, matches Python `apricot/constants.py:692`) and wired to the visible "Equalizer" player
  button (`crates/apricot-app/src/player_model.rs:148`), but **there is no equalizer dialog
  implementation anywhere in the Rust codebase** -- no `equalizer_win32.rs` or similar file exists
  (only `crates/apricot-playback/src/equalizer.rs`, which is backend DSP filter-string math, not
  UI), and `"player_equalizer"` does not appear as a dispatch arm in `win32.rs`'s
  `activate_action()` match (10760-10838). Falls through to `_ => show_unimplemented_action(...)`.
- `player_output_devices` (O): same pattern -- registered
  (`crates/apricot-core/src/action.rs:124`, shortcut `O`, matches
  `apricot/constants.py:691`), wired to the "Output devices" button
  (`player_model.rs:147`), declared as `native_dialog!("output_device_picker", ...)` in
  `crates/apricot-core/src/screen.rs:178` (a metadata-only registry, not an implementation, since
  that same file's declarations are never consumed by any dialog-opening code in
  `apricot-ui-windows`), but has zero UI implementation. Falls through to
  `show_unimplemented_action`.
- `player_comments` (Ctrl+Shift+M): same pattern -- registered
  (`crates/apricot-core/src/action.rs:133`, matches `apricot/constants.py:700`), wired to the
  "Comments" button (`player_model.rs:157`), declared as `dialog!("comments", ...)` in
  `screen.rs:171`, but Python's entire comments dialog (search box, sort choice with 5 sort
  modes, list, Open/Copy/Copy-all-visible/Open-author-channel/Load-more/Back buttons --
  `apricot/ui/misc.py:2095-2149`+) has **no Rust counterpart at all**: no comments-dialog source
  file exists in `apricot-ui-windows`, and there is no `"player_comments"` dispatch arm.
- `player_bpm` (B): as found earlier, zero implementation anywhere (see PLAYER2-M-01).
- `player_shuffle` (Shift+S): as found in PLAYER2-03, zero dispatch arm.

All five fall through to the same catch-all:
```rust
// win32.rs:13548-13564
unsafe fn show_unimplemented_action(window: HWND, action_id: &str) {
    ...
    let message = wide(&format!(
        "{} is registered, but its Rust route is not implemented in this internal build yet.",
        catalog.text(label_key)
    ));
    let _ = MessageBoxW(Some(window), PCWSTR(message.as_ptr()), w!("ApricotPlayer 2 Beta"), MB_OK | MB_ICONINFORMATION);
```
i.e. pressing F4, O, Ctrl+Shift+M, B, or Shift+S in the Rust player (or activating the
corresponding player-screen button for the first four) pops a modal English-only Windows message
box reading e.g. *"Equalizer is registered, but its Rust route is not implemented in this internal
build yet."* -- the localized part is only the feature name (via `catalog.text(label_key)`); the
sentence itself is hardcoded English regardless of `settings.language`.

- Impact: five player features that Python implements as full accessible dialogs/toggles are
  completely absent in Rust, and each one interrupts the user with a blocking, non-localized
  system dialog instead of failing silently or (better) not being wired up as a button at all.
  This is the single largest functional gap found in this pass. P1 for all five: equalizer and
  output-device selection are core accessibility-relevant playback controls; comments, BPM, and
  shuffle are advertised, keyboard-bound, and currently broken in a user-facing way.
- Suggested fix: implement the four dialogs (equalizer sliders, output device picker, comments
  browser) and the shuffle toggle handler; until then, consider hiding/disabling the corresponding
  buttons and unbinding the shortcuts rather than shipping an English-only "not implemented" modal
  in a screen-reader-first accessibility app.

### PLAYER2-07 (P1, Missing): Local-file "edit mode" (E) and its Ctrl+S save-copy / Ctrl+R replace-original commands are entirely unimplemented in Rust

This is distinct from marker/clip-preview/export (P key), which **is** implemented in Rust (see
PLAYER2 tab 4 verification below) -- edit mode is Python's separate feature for permanently
baking the *current speed and audio filter chain* into a new or replacement local media file via
ffmpeg.

- Python: `apricot/ui/misc.py:2389-2396` `toggle_edit_mode()` -- E toggles
  `self.edit_mode_enabled` (local-media items only; announces `edit_mode_local_only` for
  remote/streamed items), announcing `edit_mode_on`/`edit_mode_off`.
  `local_edit_ffmpeg_args()` (`apricot/ui/misc.py:2418-2433`) builds an ffmpeg invocation that
  applies the current playback speed (via `ffmpeg_atempo_chain`, handling the atempo 0.5-2.0
  per-stage limit) and the current audio filter chain to produce an edited copy, with `-vf
  setpts=...` for matching video speed. `player_save_edit_copy` (Ctrl+S) and
  `player_replace_edit_original` (Ctrl+R) (`apricot/constants.py:704-705`) trigger this pipeline
  to either save a new file or overwrite the original.
- Rust: `player_edit_mode`, `player_save_edit_copy`, and `player_replace_edit_original` are all
  registered actions with matching shortcuts (`crates/apricot-core/src/action.rs:136-138`,
  E/Ctrl+S/Ctrl+R respectively, matching `apricot/constants.py:703-705`), and `edit_mode` is wired
  to a visible player button (`crates/apricot-app/src/player_model.rs:158`), but **none of the
  three action ids appear as dispatch arms anywhere in `win32.rs`** (confirmed via full-file grep
  for each string) -- all three fall through to `show_unimplemented_action` (the same "is
  registered, but its Rust route is not implemented" MessageBoxW described in PLAYER2-04).
- Impact: NVDA users cannot bake in a speed/EQ-adjusted edit of a local file at all in Rust; every
  attempt (E, then Ctrl+S or Ctrl+R) pops the same non-localized "not implemented" system dialog.
- Suggested fix: implement `toggle_edit_mode` state plus the ffmpeg-based save/replace pipeline,
  matching Python's `local_edit_ffmpeg_args`/`ffmpeg_atempo_chain` semantics (atempo chain
  splitting for speed factors outside 0.5-2.0, `-c:v copy`/`-c:a copy` passthrough when no
  filters/speed change apply).

### PLAYER2-05 (P2): Embedded/background-playback results list is not in the player screen's tab order in Rust

- Python `apricot/ui/player.py:651` `embedded_results = background_enabled and not fullscreen_mode`.
  When background playback is enabled (`enable_background_playback` setting) and not fullscreen,
  Python keeps the underlying results/queue list alive and embeds it directly into the player
  screen's tab order (`player_tab_order()`, `apricot/ui/player.py:856-866`, inserting `results`
  between the navigation buttons and the player panel whenever `self.in_player_screen` and a
  results list is live) -- letting an NVDA user Tab from the player controls straight into the
  list of other items without leaving playback, and `add_player_results_section()`
  (`apricot/ui/player.py:689-690`) additionally builds a visible section for it.
- Rust: `crates/apricot-ui-windows/src/player_controls_win32.rs` `PlayerControls` is a fixed set of
  navigation/video-host/action controls (`NAVIGATION_SPECS`, `ACTION_SPECS`) with no results-list
  entry at all, and `crates/apricot-app/src/player_model.rs` `PlayerScreenModel::build()` (lines
  85-126) only conditionally adds a `close_player` button when `enable_background_playback` is on
  (line 106-114) -- it never surfaces or tab-orders the underlying list. `active_primary_control()`
  (`win32.rs:13681-13702`) also only ever returns `state.player_controls.initial_focus()` for
  `MainView::Player`, never `state.list`.
- Impact: NVDA users who rely on background playback to keep browsing a list while listening lose
  the ability to Tab directly from the player into that list in Rust; they must leave the player
  screen entirely (Escape/back) to interact with the list, which is a real, if secondary,
  discoverability/efficiency regression. Marked P2 rather than P1 because the underlying data is
  presumably still reachable by leaving the player screen, just not embedded/tab-reachable from it.
- Needs runtime verification: confirm exactly what UX Rust substitutes when
  `enable_background_playback` is on (e.g., does leaving via `close_player` actually preserve the
  list's scroll position/selection the way Python's embedded section does).

### PLAYER2-06 (P2): `show_video_details_by_default` focuses the Details button in Rust instead of auto-opening the details dialog like Python

- Python `apricot/ui/player.py:805-806`: when `self.settings.show_video_details_by_default` is
  true, entering the player screen does `wx.CallAfter(self.show_video_details, False)` --
  the F7 details dialog is automatically opened as soon as the player starts.
- Rust `crates/apricot-app/src/player_model.rs:120-124`: the same setting only changes
  `initial_focus_id` to `"details"` (the Details *button*) instead of `"video_host"` -- it moves
  keyboard focus to the button but does not invoke the dialog-opening command, so the dialog never
  auto-opens; the user must press Enter/Space on the focused button (or F7) themselves.
- Impact: users who rely on this setting to get details announced immediately on every new track
  (a common workflow for e.g. podcast listeners wanting episode notes read automatically) get a
  silent, different experience in Rust -- they now hear only "Details, button" via normal focus
  announcement instead of the dialog's content.
- Suggested fix: after focusing/creating the player view with this setting on, trigger the same
  "show details" command Rust uses for the `player_details` action, mirroring Python's
  `show_video_details(False)` call.

### PLAYER2-08 (P2): "Audio quality when changing speed" setting (rubberband/scaletempo2/scaletempo/mpv) is stored but never applied to the actual mpv process in Rust

- Python `apricot/ui/player.py:598-609` `speed_audio_filter_args()` maps the
  `speed_audio_mode` setting to one of four concrete mpv argument sets, appended to every
  `start_mpv()` launch (`apricot/player/mpv.py:106`):
  - `mpv` mode: `--audio-pitch-correction=yes` only (mpv's own scaletempo2).
  - `scaletempo`: `--audio-pitch-correction=no`,
    `--af=@apricot_speed:scaletempo=stride=30:overlap=.50:search=10`.
  - `rubberband` (Python's default/recommended, `apricot/locales/en.json:594` "(recommended)"):
    `--audio-pitch-correction=yes`,
    `--af=@apricot_speed:rubberband=transients=smooth:formant=preserved:pitch=quality:engine=finer`.
  - default/`scaletempo2`: `--audio-pitch-correction=no`,
    `--af=@apricot_speed:scaletempo2=search-interval=50:window-size=20:max-speed=8.0`.
- Rust: the `speed_audio_mode` setting exists end-to-end in storage/settings UI
  (`crates/apricot-storage/src/settings.rs:54,179,356,549`, defaulting to
  `RUBBERBAND_SPEED_MODE` -- matching Python's default) and is presented as a 4-option choice in
  the Settings screen (`crates/apricot-app/src/settings_model.rs:405-408`), but it is **never read
  by the actual mpv launch code**: a full-tree grep for `rubberband`, `scaletempo`, and
  `audio-pitch-correction` inside `crates/apricot-playback` returns zero matches, and
  `crates/apricot-playback/src/mpv_process.rs:325-393` (`launch_arguments()`) only ever emits
  `--speed=` (line 337) and `--pitch=` (line 333) with no corresponding `--af=@apricot_speed:...`
  or `--audio-pitch-correction` flag at all, regardless of the configured mode.
- Impact: mpv falls back to its own compiled-in default time-stretch algorithm for every user,
  regardless of which of the four quality modes they select in Settings -- the setting is
  effectively a placebo. Since Python's *default* is Rubberband (the "recommended" high-quality
  option) and Rust's stored default setting value matches but is inert, **every** Rust user who
  changes playback speed gets different (likely lower-quality/different-latency) audio time-
  stretching than Python's out-of-the-box behavior, not just users who explicitly picked a
  non-default mode. Marked P2 (audio-quality regression, not a functional break) rather than P1
  since speed change itself still works and is still audible/correct-pitch.
- Suggested fix: thread `settings.speed_audio_mode` through to `MpvLaunchOptions`/
  `launch_arguments()` and emit the matching `--audio-pitch-correction`/`--af=@apricot_speed:...`
  argument set, mirroring `speed_audio_filter_args()` exactly (including the `@apricot_speed:`
  filter label so live speed adjustments can target/replace it, matching
  `rubberband_pitch_filter_active`/`equalizer_filter_ref` bookkeeping Python does at
  `apricot/player/mpv.py:164-166`).

## 3b. Player window tab order and initial focus (item 3 of task) -- verified structure

- Native Win32 tab order in Rust follows control creation Z-order in
  `player_controls_win32.rs:37-65` (`NAVIGATION_SPECS` then `video_host` then `ACTION_SPECS`,
  checkboxes last), which **matches Python's order well**: back/back-to-results button(s) ->
  [Python only: embedded results list, see PLAYER2-05] -> player/video panel -> previous,
  play/pause, next, playback_queue, add_to_playlist, add_bookmark, bookmarks, output_devices,
  equalizer, audio_normalization/replaygain, chapters, transcript, lyrics, comments, edit_mode,
  copy_location/copy_stream_url, save_podcast_speed, details, [close_player] -> fullscreen,
  repeat, session_autoplay_next, bass_boost checkboxes. Compare Python
  `apricot/ui/player.py:721-793` (button list construction order) and `player_tab_order()`
  (`apricot/ui/player.py:856-866`) with Rust `ACTION_SPECS`
  (`player_controls_win32.rs:40-65`) and `append_primary_controls`/`append_media_controls`/
  `append_toggle_controls` (`crates/apricot-app/src/player_model.rs:129-`). Item-for-item order
  is identical; this part of the port is faithful.
- Initial focus: Python focuses `player_panel` (the black video/audio canvas) by default
  (`apricot/ui/player.py:717-718,801-809`), same as Rust's default `initial_focus_id: "video_host"`
  (`player_controls_win32.rs:145`, `player_model.rs:121-124`) -- matches, **except** for the
  `show_video_details_by_default` case, see PLAYER2-06 above.


## 3. Missing functionality

- PLAYER2-M-01 (P1): BPM analysis (`B` shortcut, `player_bpm` action) is entirely absent from
  Rust. Python: `apricot/ui/misc.py:2710-2803`+ (`announce_bpm_async`, `analyze_bpm_worker`,
  ffmpeg-based tempo detection, `bpm_analysis_state_key`/`current_bpm_analysis_state` caching by
  item+speed+pitch, `effective_playback_bpm` speed-scaling, `bpm_analysis_window`). Rust: zero
  matches anywhere in the tree for "bpm" outside the single action-table registration
  (`crates/apricot-core/src/action.rs:115`); falls through to `show_unimplemented_action`.
- PLAYER2-M-02 (P1): F4 equalizer dialog UI is entirely absent from Rust (only the backend `af`
  filter-string builder exists, `crates/apricot-playback/src/equalizer.rs`). See PLAYER2-04.
- PLAYER2-M-03 (P1): O output-device picker dialog is entirely absent from Rust. See PLAYER2-04.
- PLAYER2-M-04 (P1): Ctrl+Shift+M comments dialog (search/sort/list/open/copy/copy-all/
  open-author-channel/load-more) is entirely absent from Rust. Python:
  `apricot/ui/misc.py:2095-2149`+. See PLAYER2-04.
- PLAYER2-M-05 (P1): Shift+S live shuffle toggle has no handler in Rust (shuffle can only be set
  when starting a new sequence, not toggled mid-playback). See PLAYER2-03.
- PLAYER2-M-06 (P1): Local-file "edit mode" (E) and its Ctrl+S save-copy / Ctrl+R
  replace-original ffmpeg/mpv-render pipeline (`apricot/ui/misc.py:2389-2433`,
  `local_edit_ffmpeg_args`, `local_edit_mpv_render_args`, `ffmpeg_atempo_chain`) is entirely
  absent from Rust. See PLAYER2-07.
- PLAYER2-M-07 (P2): The "Audio quality when changing speed" setting
  (rubberband/scaletempo2/scaletempo/mpv) is never applied to actual playback in Rust -- see
  PLAYER2-08.
- PLAYER2-M-08 (P2): Embedded/background-playback results list in the player screen's tab order
  is absent from Rust -- see PLAYER2-05.
- PLAYER2-M-09 (P3, needs runtime verification): Python's independent pitch-mode setting
  (`pitch_mode`: rubberband / mpv / linked-speed-and-pitch,
  `apricot/ui/player.py:616-635` `normalized_pitch_mode()`, used in the local-edit render path and
  presumably in live pitch changes too via `rubberband_pitch_filter`/
  `rubberband_pitch_filter_active`, `apricot/player/mpv.py:164`) was not fully traced on the
  Rust side within this pass's budget beyond the initial `--pitch=` launch argument (PLAYER2-01)
  and the missing local-edit pipeline (PLAYER2-M-06); it is plausible live in-session pitch changes
  in Rust also bypass the rubberband/linked-pitch distinction the same way speed mode does
  (PLAYER2-08), but this needs a dedicated live-pitch-change trace to confirm.

## 4. Verified as matching

- PLAYER-01 speed/pitch-carryover bug: precisely re-verified against corrected Python line
  references; the first auditor's description and Rust citations are accurate.
- Player screen tab order (native Win32 Z-order): item-for-item match between Python's
  `player_tab_order()`/button construction order and Rust's `NAVIGATION_SPECS`/`video_host`/
  `ACTION_SPECS`/checkbox order (see section 3b above).
- Default initial focus (player panel/video host) matches, except the
  `show_video_details_by_default` case (PLAYER2-06).
- Resume-position logic: Rust's `playback_position_controller.rs` explicitly documents and
  implements Python's 5-second minimum and "within 8 seconds of the end clears the saved position"
  thresholds (`resume_position()` line 77-86, `update()` line 94-121), gated by the same
  `resume_playback` setting and live-stream exclusion.
- Equalizer `af` filter-string construction: band Q-widths, `@apricot_eq:lavfi=[...]` label,
  `alimiter=limit=0.95:attack=5:release=80` clipping limiter, and the
  `-min(EQ_CLIPPING_HEADROOM_LIMIT_DB, max_positive_gain)` headroom formula all match between
  Python `apricot/ui/equalizer.py:70-113` and Rust `crates/apricot-playback/src/equalizer.rs`
  (including Rust's own unit tests asserting the exact filter strings).
- Marker set/clear announcements (`clip_start_marker_set/cleared`,
  `clip_end_marker_set/cleared`) and clip export announcements (`clip_export_started/done/failed`)
  are correctly routed through the locale catalog in Rust
  (`crates/apricot-ui-windows/src/win32.rs:12714-12717,7126-7163`), unlike the T/V/speed/pitch/
  repeat/boost family (PLAYER2-02).
- `player_format_status` (F) is fully and correctly localized across all 7 format variants in
  both Python (`apricot/locales/en.json:1041-1047`) and Rust
  (`crates/apricot-app/src/player_information.rs:51-82`).
- mpv launch flags for volume/speed/pitch/loop/gapless/replaygain/cache/EQ (already confirmed by
  the first auditor) remain consistent with this pass's closer reading of `mpv.py`/
  `mpv_process.rs`.

## 5. Needs runtime/NVDA verification

- PLAYER2-01/PLAYER-01: confirm live that Next/Previous/Related during an open Rust player session
  keeps an adjusted speed/pitch from the prior track (static analysis is conclusive on the code
  path, but not yet observed in a running build).
- PLAYER2-02: confirm the exact NVDA-spoken text for each hardcoded-English announcement in a
  non-English `settings.language` build, to demonstrate the localization gap audibly.
- PLAYER2-03/04/07: confirm live that pressing Shift+S, F4, O, Ctrl+Shift+M, B, E, Ctrl+S, and
  Ctrl+R from the player screen each pop the "is registered, but its Rust route is not
  implemented" MessageBoxW, and confirm whether that dialog is itself announced/focusable
  correctly by NVDA (a native MessageBoxW should be, but the interruption itself is the concern).
- PLAYER2-05: confirm what Rust actually does today when `enable_background_playback` is on and
  the player is left via `close_player` -- does the underlying list's selection/scroll position
  survive, even though it is not embedded in the player's own tab order.
- PLAYER2-08: confirm audibly/measurably that Rust's speed-changed audio uses a different
  time-stretch algorithm than Python's Rubberband default (e.g., compare artifact character at
  2x speed) to corroborate the source-level finding that `--af=@apricot_speed:...` is never
  emitted.
- PLAYER2-M-09: live-pitch-change pitch-mode (rubberband vs mpv vs linked) parity was not traced
  in this pass; needs a dedicated follow-up comparing `change_pitch_async`
  (`apricot/ui/misc.py:2959-3020`) against Rust's `adjust_player_pitch`
  (`win32.rs:13070-13083`) and whatever backs `PlaybackCommand::SetPitch`.
