# ApricotPlayer macOS feature-parity port plan

Status: confirmed future work. No macOS implementation has started yet.

Baseline: the current `main` branch, version `1.0.3`, plus every feature added
before the macOS branch is merged.

## Non-negotiable product contract

The macOS edition is not a reduced companion app. It must expose every feature,
setting, action, context-menu command, keyboard workflow, error state, and data
operation that the Windows edition exposes. A feature is not considered ported
when only its happy path works or when it can be used only with a mouse.

The parity rules are:

1. Keep one shared application and domain codebase. Platform modules may differ,
   but product behavior must not fork into separate Windows and macOS editions.
2. Preserve all existing Windows behavior while the port is developed.
3. Make every feature operable with a keyboard and a screen reader on both
   platforms: NVDA on Windows and VoiceOver on macOS.
4. Use the same navigation model, focus destinations, Escape behavior, session
   state, list ordering, queue semantics, and announcements on both platforms.
5. Map the logical primary modifier to Control on Windows and Command on macOS.
6. Treat native OS surfaces as equivalent rather than pixel-identical. Finder,
   Keychain, the macOS menu bar, notification prompts, and file dialogs should
   use native macOS behavior while preserving the same ApricotPlayer action.
7. Do not ship a stable macOS release with an intentionally missing feature.
   A platform limitation must be documented, proven, and explicitly approved by
   the product owner before it can become an exception.
8. After macOS support is released, every new ApricotPlayer feature must be
   implemented and tested on Windows and macOS in the same development cycle.
9. The user-facing macOS release artifact is a DMG. A separate macOS ZIP is not
   required. Until an Apple Developer account is available, the DMG may use an
   ad-hoc application signature and remain unnotarized, but the release must say
   this clearly and document the expected Gatekeeper warning.

## Supported targets and release artifacts

The first target is Apple Silicon (`arm64`) on the user's MacBook Air. The
minimum supported macOS version will be fixed during the compatibility spike,
after checking wxPython, Python, mpv, FFmpeg, Node, and Sparkle deployment
targets. Intel (`x86_64`) can be added as a second DMG later without changing the
shared application architecture.

Once the macOS release pipeline is enabled, a normal release must contain:

- `ApricotPlayerSetup.exe`
- `ApricotPlayer.zip`
- `ApricotPlayer-macOS-arm64.dmg`

The two existing Windows names remain stable for updater compatibility. The
macOS updater selects only the DMG for its architecture and release channel.
An early unsigned/unnotarized build must still use ApricotPlayer's independent
cryptographic update verification, trusted-host checks, bounded downloads, and
validated package layout. Apple notarization is additional platform trust, not a
replacement for updater integrity checks.

## Complete feature-parity manifest

This manifest is an acceptance checklist, not a suggestion. Before each macOS
beta, it must be compared with the current code so newly added Windows features
cannot be omitted.

### Application shell, navigation, and accessibility

- First-run language selection and all shipped UI languages.
- Main menu, screen transitions, Back controls, Escape behavior, and restoration
  of the previous screen and focused item.
- Normal, classic, background, and fullscreen player layouts.
- Tab, Shift+Tab, arrow-key, Home, End, Enter, Space, Escape, context-menu, and
  letter navigation on every applicable control.
- Focus preservation while results, metadata, notifications, or downloads update
  in the background.
- Screen-reader names, roles, values, states, status changes, speech, and braille
  output without duplicate announcements.
- Dynamic shortcut labels and the option to hide shortcut text from labels.
- Action Finder from every screen.
- Customizable main menu without disabling hidden actions' global shortcuts.
- Single-instance behavior, window activation, current-media window title,
  reopen behavior, and opening new media in the existing instance.
- Background playback, menu-bar/tray controls, application restore, and clean
  shutdown.
- Diagnostic report, logs, localized errors, loading states, empty states, and
  cancellation paths.

### Main menu

All customizable items must exist and preserve their current ordering and
visibility rules:

- Current downloads (`current_downloads`).
- Playback queue (`playback_queue`).
- Search YouTube / SoundCloud (`search`).
- Resume last session (`resume_last_session`).
- Trending (`trending`).
- AudioVault (`audiovault`).
- Play folder (`play_folder`).
- Play file (`play_file`).
- Direct link (`direct_link`).
- Favorites (`favorites`).
- Bookmarks (`bookmarks`).
- Playlists (`playlists`).
- Subscriptions (`subscriptions`).
- Notification center (`notification_center`).
- History (`history`).
- Podcasts and RSS (`rss_feeds`).
- File converter (`file_converter`).
- Folder converter (`folder_converter`).
- Copy diagnostic report (`diagnostic_report`).

The following non-customizable items also remain available when applicable:

- Update available.
- Settings.
- Exit or Quit.

### Search, discovery, and online sources

- YouTube and SoundCloud search, stale-search protection, result columns, and
  configurable dynamic result batches.
- Trending with country/category filters and optional YouTube Data API key.
- YouTube videos, Shorts, playlists, channels, channel videos, channel playlists,
  and whole-channel popular-video ordering.
- Dynamic loading for search, channel, playlist, popular, and folder results,
  including End-to-load and loading while Next advances past the current page.
- Exact selected-item playback while metadata or another result page is loading.
- Open channel, subscribe, unsubscribe, copy, favorite, playlist, queue, download,
  and browser actions from results and context menus.
- Direct playback and download for yt-dlp-supported URLs.
- Browser-cookie and cookies-file fallback only after the normal fast path proves
  that authentication is required.
- Optional age-restricted playback support, EJS/Node fallback, proxy, retries,
  socket timeout, rate limit, and advanced network settings.
- Stream URL cache, expiration, next-item prefetch, and failure fallback without
  slowing the normal successful path.

### Player and playback behavior

- Internal mpv playback for YouTube, SoundCloud, direct links, local media,
  podcasts, RSS episodes, AudioVault, history, favorites, playlists, bookmarks,
  subscriptions, notification items, and queued items.
- Embedded video, audio-only playback, fullscreen, background playback, and the
  optional external-player/browser path.
- Play, pause, restart-after-end, previous, next, related next, repeat, shuffle,
  autoplay next, autoplay related, and session-only autoplay.
- Queue-aware Next/Previous behavior, source-aware dynamic loading, correct end
  announcements, and no random or skipped items.
- Small, large, and huge seek; jump to start/end; held-key scrubbing; immediate
  time/status updates; and smooth seeking in long media.
- Session volume, configured default volume, volume boost, volume steps, output
  device selection, missing-device fallback, and reset when the player session
  really closes.
- Playback speed, pitch, configurable steps, held-key delay/interval, reset to
  configured defaults, and per-podcast speed presets.
- Rubberband, scaletempo2, scaletempo, and mpv-default audio processing modes.
- Gapless playback, ReplayGain track/album modes, stream cache, resume positions,
  last-session restoration, start paused, and playback-finished announcements.
- Local-file edit mode, saving an edited copy, replacing the original, and safe
  cancellation/error handling.

### Audio processing and equalizer

- Ten independent bands: 31, 62, 125, 250, 500, 1000, 2000, 4000, 8000, and
  16000 Hz.
- Global and player-session equalizers using the same gain model.
- 6, 12, 18, and 24 dB ranges with stable keyboard steps and correct spoken
  values.
- Flat and built-in presets, custom profiles, naming, saving, editing, deleting,
  import/export, and A/B comparison.
- Output-device-linked presets and fallback when a device disappears.
- Bass boost, volume boost, EQ clipping protection, ReplayGain interaction, and
  protection against startup pops, crackle, clipping, channel collapse, or
  unintended mono output.
- Live filter replacement without one band changing another band and without
  playback stalls.
- BPM analysis, including a fresh analysis every time the user presses `B`.

### Media information and reading features

- Details for online and local media with source-appropriate labels.
- Elapsed, remaining, total time, live-stream state, upload time, duration,
  channel, view, and source metadata where available.
- YouTube and podcast chapters, list navigation, Enter-to-jump, previous/next
  chapter, and unavailable states.
- Timed transcripts and captions, local VTT/SRT sidecars, searching, Enter-to-
  jump, copy line, copy all, export, and timestamp links.
- Local and online lyrics, read-only navigation, copy, export, and unavailable
  states.
- YouTube comments through the API or yt-dlp, sorting, searching, replies,
  copying, author-channel access, and unavailable/rate-limit states.
- Playback bookmarks: add, list, rename, delete, jump, resume, and copy timestamp
  link.
- Clip start/end markers, marker toggling, marked preview, audio/video export, and
  preservation of the normal playback state.
- Copy page link, timestamped link, local path, and direct media stream URL only
  in contexts where each action is meaningful.

### Local media, library, and collections

- Open individual audio/video files from ApricotPlayer and from Finder.
- Open and play folders, natural numeric sorting, play entire folder, shuffle
  folder, dynamic result batches, and queue creation only when playback starts.
- Media file associations, Open With support, default-player guidance, and
  second-launch routing to the existing process.
- Favorites, history, user playlists, playback queue, and mixed-source items.
- Playlist create, rename where supported, add, remove, play, shuffle, download,
  queue, and clear operations.
- History for played and downloaded media, including working direct URLs after an
  application restart.
- Named playback bookmarks and resume positions bound to the correct media item.
- YouTube subscriptions, manual/automatic refresh, new-video views, categories,
  notifications, and notification-center playback.
- Podcast/RSS libraries, Apple Podcasts directory search, direct RSS/Atom URLs,
  refresh, categories, played state, progress reset, per-feed speed, full archive
  loading beyond 500 episodes, download feed, queue, copy URL, and browser actions.
- AudioVault login/logout, persistent credentials, movies, TV shows, recent
  content, search, show/episode navigation, streaming, history, queue, audio
  download, whole-show download, progress, cancellation, session expiry, and safe
  archive extraction.

### Downloads and conversion

- Single audio and video downloads with the current format/quality choices.
- Playlist, channel, selected-result, podcast-feed, AudioVault-show, and other
  batch downloads.
- Continue a batch after an individual item fails, report the failed item, and
  provide a final success/failure summary.
- Current downloads screen, independent progress window, item/total progress,
  hide, details, cancellation, completion, and continued use of the main app.
- Configurable default folder and Ask every time behavior for every source,
  including YouTube, podcasts, local exports, and AudioVault.
- Filename template, playlist order, collision numbering, restricted names,
  metadata, thumbnails, descriptions, info JSON, subtitles, automatic subtitles,
  subtitle languages, archive, notifications, and open-folder-after-completion.
- File and folder converter, detected input, output format, recursive conversion,
  audio/video formats, image-backed audio-to-video, new destination or safe
  replacement, progress, cancellation, collisions, and completion messages.
- Marked-clip and edited-media FFmpeg exports with equivalent codec behavior.

### Settings parity

Every field in `apricot.models.Settings` is part of the port contract, including
settings that are renamed for platform-neutral UI. The current field inventory is:

```text
language, download_folder, results_limit, audio_format, video_format,
max_video_height, player_command, autoplay_next, autoplay_related,
prefer_browser_playback, player_fullscreen, player_start_paused,
announce_play_pause, announce_playback_finished, enable_background_playback,
player_speed, speed_audio_mode, show_video_details_by_default,
direct_link_enter_action, enable_age_restricted_videos, enable_stream_cache,
enable_stream_url_cache, stream_url_cache_minutes, prefetch_next_stream_url,
gapless_playback, replaygain_mode, enable_online_lyrics, cache_folder,
cache_size_mb, resume_playback, show_resume_in_menu, audio_output_device,
speed_step, pitch_step, speed_pitch_hold_delay_ms,
speed_pitch_hold_interval_ms, pitch_mode, global_equalizer_enabled,
global_equalizer_preset, global_equalizer_gains, equalizer_preset_gains,
equalizer_custom_names, equalizer_device_presets, equalizer_db_range,
equalizer_clipping_protection, ask_download_location_each_time,
quiet_downloads, keep_playlist_order, filename_template, audio_quality,
seek_seconds, volume_step, default_volume, volume_boost_by_default,
write_thumbnail, write_description, write_info_json, write_subtitles,
auto_subtitles, subtitle_languages, embed_metadata, embed_thumbnail,
restrict_filenames, open_folder_after_download, popup_when_download_complete,
popup_when_conversion_complete, auto_update_ytdlp, auto_update_app,
app_update_interval_hours, app_update_notifications, skipped_update_version,
update_channel, confirm_before_download, download_archive, rate_limit, proxy,
youtube_data_api_key, audiovault_email, audiovault_password_protected,
cookies_file, cookies_source_file, cookies_source_signature,
cookies_from_browser, cookies_browser_profile,
show_advanced_network_settings, cookie_user_agent, ffmpeg_location,
concurrent_fragments, retries, socket_timeout, close_to_tray,
start_with_windows, tray_notification, subscription_check_enabled,
subscription_check_interval_hours, windows_notifications,
download_notifications, subscription_notifications, last_subscription_check,
enable_trending, enable_history, enable_podcasts_rss,
show_shortcuts_in_labels, main_menu_hidden_actions, podcast_search_provider,
podcast_search_country, podcast_search_limit, rss_max_items,
rss_refresh_on_startup, rss_auto_refresh_enabled,
rss_refresh_interval_hours, history_limit, keyboard_shortcuts,
media_association_prompted_version, language_prompted
```

Platform-neutral migrations must preserve existing Windows JSON keys. For
example, the UI may say Start at login and System notifications while the data
loader continues accepting `start_with_windows` and `windows_notifications`.

Settings parity includes General, Customize main menu, Playback, Equalizer,
Downloads, Library, Podcasts, Notifications, Cookies/Network, AudioVault, and
Keyboard shortcuts; Reset section and Reset all settings must work identically.

### Keyboard shortcut parity

`DEFAULT_KEYBOARD_SHORTCUTS` and `SHORTCUT_DEFINITIONS` are the canonical action
catalog. Automated tests must fail if an action exists without a macOS mapping,
capture path, display label, handler, or accessibility test.

The current action inventory is:

```text
open_main_menu, open_search, open_audiovault, open_play_from_folder,
open_play_file, open_direct_link, open_favorites, open_bookmarks,
open_playlists, open_subscriptions, open_current_downloads, open_history,
open_podcasts_rss, open_settings, open_action_finder,
background_play_pause, copy_diagnostic_report, download_audio,
download_video, subscribe_channel, unsubscribe_channel, open_channel,
queue_audio, result_column_previous, result_column_next,
add_to_playback_queue, remove_from_playback_queue, open_playback_queue,
create_playlist, add_favorite, remove_favorite, add_to_playlist,
remove_from_playlist, copy_link, copy_stream_url, context_menu,
open_selected, new_subscription_videos, remove_selected,
toggle_podcast_played, clear_podcast_progress, save_podcast_speed_preset,
player_copy_link, player_copy_timestamp_link, player_play_pause, player_time,
player_bpm, player_speed_down, player_speed_up, player_reset_speed_pitch,
player_pitch_up, player_pitch_down, player_volume_status, player_details,
player_output_devices, player_equalizer, player_fullscreen,
player_replaygain, player_add_bookmark, player_bookmarks, player_chapters,
player_transcript, player_lyrics, player_comments, player_previous_chapter,
player_next_chapter, player_edit_mode, player_save_edit_copy,
player_replace_edit_original, player_marker_start, player_marker_end,
player_preview_marked_clip, player_previous, player_next,
player_next_related, player_back, player_volume_boost, player_bass_boost,
player_repeat, player_shuffle, player_seek_back, player_seek_forward,
player_seek_back_large, player_seek_forward_large, player_seek_back_huge,
player_seek_forward_huge, player_seek_start, player_seek_end,
player_volume_up, player_volume_down
```

Mapping rules:

- `Ctrl` is stored as a logical primary modifier and displayed/handled as
  `Command` on macOS.
- `Alt` is displayed/handled as `Option` on macOS.
- Shift, letters, digits, arrows, brackets, Escape, Enter, Space, and function
  keys retain their logical meaning.
- `Command+Space` conflicts with Spotlight. The macOS default for background
  play/pause is `Command+Shift+Space`, while normal player play/pause remains
  Space. The user can reassign it.
- MacBook keyboards expose PageUp/PageDown/Home/End through Fn combinations. The
  shortcut editor stores logical keys and documents their physical Fn form.
- Mac keyboards have no Applications key. Support Shift+F10 and the native
  VoiceOver context-menu command without removing Windows Applications-key
  support.
- Held seek, volume, pitch, and speed keys must be processed as bounded repeating
  state, not as an unbounded backlog of wx key events.

## Target architecture

Create deep platform adapters instead of scattering platform checks through UI
and domain modules:

```text
apricot/platform/
  __init__.py
  base.py
  windows.py
  macos.py

apricot/player/ipc.py
  WindowsNamedPipeTransport
  UnixSocketTransport
```

The platform facade owns:

- application data, cache, log, download, and temporary paths;
- bundled binary names and discovery;
- single-instance locking, activation, and open-file forwarding;
- opening files, folders, URLs, and system settings;
- startup/login registration;
- menu-bar/tray and desktop notifications;
- secure credential storage;
- browser process/profile/cookie helpers;
- file associations and default-app integration;
- screen-reader announcements;
- primary-modifier display and event matching;
- application update installation.

Shared UI and domain modules continue owning search, player state, queue, library,
downloads, metadata, EQ, and settings semantics.

## Windows-specific code that must be isolated

- `wx_main.py`: `winreg`, `user32` activation, `kernel32` mutex, and handle cleanup.
- `apricot/constants.py`: `%APPDATA%`, `.exe` names, and Windows directory layout.
- `apricot/player/mpv.py`: `mpv.exe`, Windows named pipes, and window embedding.
- `apricot/network/cookies.py` and `apricot/ui/cookies.py`: browser discovery,
  process handling, Chromium DPAPI behavior, and cookie export diagnostics.
- `apricot/network/audiovault.py`: Windows DPAPI credential protection.
- `apricot/system/registry.py`: registry file associations and default-app repair.
- `apricot/ui/misc.py` and `apricot/ui/system.py`: NVDA DLL, tray, notifications,
  startup, and Windows-specific text.
- `apricot/download/download.py`: `.exe` paths and `os.startfile`.
- `apricot/updater/updater.py`: Windows asset selection, PowerShell replacement,
  installer/ZIP validation, restart, and rollback.
- `apricot/system/diagnostics.py` and `apricot/utils.py`: path redaction, trusted
  binaries, process flags, and named-pipe assumptions.
- `ApricotPlayer.spec`, PowerShell build scripts, Inno Setup, and the Windows-only
  GitHub Actions release job.

## macOS service implementations

- Use `~/Library/Application Support/ApricotPlayer` for durable application data.
- Use `~/Library/Caches/ApricotPlayer` for cache and
  `~/Library/Logs/ApricotPlayer` for logs.
- Use a macOS Keychain item for AudioVault credentials and other secrets.
- Use Unix-domain sockets for mpv JSON IPC.
- Bundle arm64 mpv, FFmpeg, Node, yt-dlp, yt-dlp-ejs, Rubberband support, locales,
  assets, and required dynamic libraries inside `ApricotPlayer.app`.
- Use `wx.App.MacOpenFiles`, reopen events, and an app-instance transport so
  Finder media opens in the existing application process.
- Declare supported audio/video document types and URL handling in `Info.plist`.
- Implement Start at login using a supported login-item or LaunchAgent path.
- Provide a keyboard-accessible macOS menu-bar item equivalent to the Windows
  tray menu and preserve close-to-background behavior.
- Use native notifications with permission handling and VoiceOver-readable text.
- Use a native accessibility announcement adapter for VoiceOver; do not replace
  it with shelling out to `say`.
- Use a native macOS updater adapter, planned around Sparkle 2, while keeping the
  existing ApricotPlayer update prompts, stable/beta choice, release notes, skip,
  progress, verification, and rollback semantics.

## Implementation sequence and gates

### Phase 0: freeze and generate the baseline

- Record the exact source commit and current version.
- Generate machine-checkable main-menu, settings, shortcuts, context-menu, and
  Action Finder manifests from the current code.
- Run and record the complete Windows baseline before structural changes.
- Add platform-contract tests that initially exercise the Windows adapter.

Gate: no unexplained Windows test, startup, playback, focus, or timing regression.

### Phase 1: prove the highest-risk macOS paths

- Build a minimal arm64 wxPython app on the MacBook.
- Verify VoiceOver names, list navigation, sliders, checkboxes, focus changes,
  context menus, and announcements.
- Launch bundled mpv, connect through a Unix socket, and prove audio playback,
  embedded video, fullscreen, output-device enumeration, EQ, speed/pitch, and
  held seeking.
- Build an ad-hoc-signed PyInstaller `.app` and launch it through Finder.

Gate: do not continue to the broad port until embedded video and VoiceOver meet
the product contract. If `--wid` cannot embed reliably in wx/Cocoa, evaluate a
native child-window bridge or libmpv without changing player semantics.

### Phase 2: introduce the platform boundary

- Add platform services and move Windows behavior behind the Windows adapter.
- Add IPC transports and binary/path resolvers.
- Keep Windows behavior byte-for-byte equivalent where practical.
- Add contract tests that run against fake and real platform services.

Gate: the Windows build and existing regression suite remain green.

### Phase 3: port application shell and accessibility

- Implement macOS paths, single instance, app activation, Finder open/reopen,
  menu-bar behavior, native dialogs, notifications, and login startup.
- Implement primary-modifier shortcuts, capture/editing, conflict detection,
  labels, held keys, and MacBook physical-key handling.
- Implement VoiceOver announcements and complete keyboard focus audit.

Gate: every shell, navigation, main-menu, and shortcut manifest item passes.

### Phase 4: port player and audio

- Complete mpv process lifecycle, IPC requests/events, embedding, cache, resume,
  queues, autoplay, dynamic loading, and session-state parity.
- Verify all EQ, boost, clipping, ReplayGain, output device, speed, pitch, BPM,
  edit, bookmark, chapter, transcript, lyrics, comments, and marker paths.

Gate: the full playback and audio matrix passes on local/online short and long
media, with no added startup delay on the normal path.

### Phase 5: port sources, libraries, downloads, and converters

- Complete YouTube/SoundCloud, cookies, AudioVault, podcasts/RSS, local files,
  histories, favorites, playlists, queue, subscriptions, notifications,
  downloads, batch progress, FFmpeg export, and converters.
- Verify every Ask where to save path and source-specific context menu.

Gate: every complete feature-parity manifest section passes.

### Phase 6: package, distribute, and update

- Add a macOS PyInstaller spec with `BUNDLE`, bundle ID, icon, document types,
  minimum system version, entitlements, and version metadata.
- Ad-hoc sign nested executables/libraries and the outer app for the initial
  no-Developer-account build, then create the DMG and verify its hashes and
  package layout.
- Clearly label early GitHub assets as not Developer ID signed or notarized and
  document the current macOS System Settings path for approving the first launch.
- When an Apple Developer account becomes available, replace the distribution
  step with Developer ID signing, hardened runtime, `notarytool`, ticket stapling,
  and `codesign`/`spctl` verification without changing application features.
- Integrate the stable/beta updater and test a real N-1 update plus forced
  rollback.

Gate: a clean Mac installs, launches, updates, and reopens media after the one-time
documented Gatekeeper approval. After Developer ID is introduced, the same gate
must pass without a manual security override.

### Phase 7: dual-platform release automation

- Split GitHub Actions into test/build jobs for Windows and macOS arm64.
- Download pinned platform runtimes and verify their SHA-256 digests.
- Keep Sparkle/update signing material in GitHub encrypted secrets. Add Developer
  ID and notarization credentials there only after the account exists.
- Build and verify all artifacts before publishing the GitHub release, so an
  updater never sees a half-published release.
- Publish the same version and changelog for both platforms.
- Generate stable and beta update metadata only after all required artifacts pass.

Gate: one tag reproducibly produces verified Windows artifacts and the notarized
macOS DMG from the same source commit.

### Phase 8: beta and stable rollout

- Install a local development build on the user's MacBook.
- Run the full keyboard and VoiceOver script on real hardware.
- Publish macOS builds to the beta channel first without changing stable users.
- Test clean install, update, downgrade rejection, corrupt download rejection,
  offline behavior, uninstall, and preservation of user data.
- Promote to stable only when the parity manifest has no unapproved gaps.

## Required verification matrix

Automated CI must include:

- shared unit tests on Windows and macOS;
- platform-adapter contract tests;
- shortcut catalog and duplicate/conflict tests;
- settings serialization and migration tests;
- updater asset-selection, signature/hash, installation, and rollback tests;
- player IPC protocol tests for named pipes and Unix sockets;
- safe archive extraction and untrusted-URL tests;
- wx smoke tests for key screens, Enter/Space actions, focus, and Escape returns;
- `compileall`, the repository's critical Ruff subset, and `git diff --check`.

Real-device acceptance must cover:

- VoiceOver on every screen and dialog;
- every main-menu item, context menu, Action Finder action, and shortcut;
- YouTube, SoundCloud, direct links, local files/folders, podcasts/RSS,
  AudioVault, history, favorites, playlists, bookmarks, notifications, and queue;
- audio-only and embedded video, fullscreen, background, menu-bar, and external
  player paths;
- Next/Previous at start, middle, dynamic-page boundary, and true end;
- seek/scrub on short and multi-hour media;
- all ten EQ bands and every audio-processing combination;
- single, batch, playlist/channel/feed/show, clip, and conversion downloads;
- cookies from each supported browser plus manual cookies files;
- app update on stable and beta channels and the yt-dlp component updater;
- Finder Open With, second launch, start at login, system notifications, output
  devices, sleep/wake, network interruption, and clean shutdown.

## Inputs needed from the product owner

Implementation and early public GitHub distribution can begin with an ad-hoc
signed, unnotarized build. The product owner accepts the resulting Gatekeeper
warning for this phase. Implementation requires:

- access to the MacBook Air for local build and VoiceOver testing;
- the Mac model/chip and macOS version;
- final confirmation of the minimum supported macOS version and whether an Intel
  DMG is required.

Developer ID Application membership, certificate, and notarization credentials
are deferred. They are required only when the project chooses warning-free public
distribution.

## Definition of done

The macOS port is complete only when:

1. Every item in this manifest is implemented or explicitly approved as a proven
   OS-level exception.
2. The current Windows suite still passes and Windows behavior has not regressed.
3. The full macOS automated suite passes from a clean checkout.
4. The real MacBook VoiceOver and keyboard acceptance run passes.
5. The DMG installs on a clean account using the documented one-time Gatekeeper
   approval. Once Developer ID is enabled, it installs without that approval.
6. Stable and beta app updates plus the yt-dlp updater work on both platforms.
7. One release tag produces all required Windows assets and the macOS DMG from
   the same commit.
8. Current user data survives app updates, and optional imported Windows data is
   migrated without copying machine-specific paths, devices, or encrypted
   credentials blindly.
