# ApricotPlayer 2.0 Rust parity manifest

Status: mandatory acceptance checklist for the Rust rewrite.

Baseline source: current Python code, tests, `CHANGELOG.md`, release notes, and
the existing macOS parity plan. The manifest must be regenerated and reviewed
against the exact baseline commit when implementation begins and before every
parity-complete beta or release candidate.

An item is complete only when implementation, automated evidence where
practical, and keyboard/NVDA behavior all pass. A parent checkbox cannot be
closed while a child behavior is missing.

Spotify is intentionally not part of this manifest.

## Baseline cardinalities

- [ ] 116 `Settings` fields represented, migrated, resettable, and tested.
- [ ] 91 shortcut actions represented, editable, scoped, displayed, and tested.
- [ ] 19 customizable main-menu actions represented in exact order.
- [ ] Update Available, Settings, and Exit remain permanent menu items.
- [ ] 27 shipped languages load with English fallback and valid placeholders.
- [ ] 10 independent EQ bands and 21 current preset/profile slots represented.
- [ ] Every durable Python data file has a migration and rollback test.
- [ ] Every current Python test has an equivalent Rust regression purpose.

## Application shell, navigation, and accessibility

- [ ] First-run language selection and all shipped languages.
- [ ] Main menu, route transitions, Back controls, and one-step Escape behavior.
- [ ] Restoration of previous screen, collection context, selected item, column,
  and focused control.
- [ ] Search/channel/playlist nesting unwinds one level at a time.
- [ ] Normal, classic, background, and fullscreen player layouts.
- [ ] Tab and Shift+Tab move once in declared order without duplicate speech.
- [ ] Arrow, Home, End, PageUp, PageDown, Enter, Space, Escape, Applications,
  Shift+F10, and letter navigation work in every applicable control.
- [ ] Result-list keys never leak to player actions; player-only actions never
  run from results, settings, text fields, or unrelated dialogs.
- [ ] Focus remains stable while metadata, upload times, notifications, result
  pages, podcast pages, or download progress update asynchronously.
- [ ] Every control exposes correct accessible name, role, value, state, focus,
  and invocation behavior to UI Automation/NVDA.
- [ ] Speech and braille receive useful row/value text without duplicate or
  stale announcements.
- [ ] Dynamic shortcut labels reflect user-configured shortcuts.
- [ ] `show_shortcuts_in_labels` hides label suffixes without disabling actions.
- [ ] Action Finder works globally and lists only currently meaningful actions
  with correct shortcuts and enablement.
- [ ] Customizing the main menu never disables a hidden action's shortcut.
- [ ] Single-instance lock, second-launch activation, current-media window title,
  Open With routing, and new-media forwarding use the existing process.
- [ ] Background playback, tray controls, restore, close-to-tray, Alt+F4, Exit,
  and clean shutdown retain their distinct semantics.
- [ ] Reopening from desktop, tray, file association, or second launch restores
  the window promptly and focuses the correct Apricot control.
- [ ] Loading, empty, error, cancellation, offline, retry, and unavailable states
  are localized and keyboard-recoverable.
- [ ] Diagnostic report and logs include actionable state but redact secrets and
  personal paths.

## Main menu

Customizable items in exact order:

- [ ] `current_downloads`: Current downloads.
- [ ] `playback_queue`: Playback queue.
- [ ] `search`: Search YouTube / SoundCloud.
- [ ] `resume_last_session`: Resume last session, only when available/enabled.
- [ ] `trending`: Trending, according to feature setting.
- [ ] `audiovault`: AudioVault.
- [ ] `play_folder`: Play folder.
- [ ] `play_file`: Play file.
- [ ] `direct_link`: Direct link.
- [ ] `favorites`: Favorites.
- [ ] `bookmarks`: Bookmarks.
- [ ] `playlists`: User playlists.
- [ ] `subscriptions`: YouTube subscriptions.
- [ ] `notification_center`: Notification center.
- [ ] `history`: History, according to feature setting.
- [ ] `rss_feeds`: Podcasts and RSS, according to feature setting.
- [ ] `file_converter`: File converter.
- [ ] `folder_converter`: Folder converter.
- [ ] `diagnostic_report`: Copy diagnostic report.

Permanent items:

- [ ] Update Available appears when applicable and cannot be hidden.
- [ ] Settings is always visible.
- [ ] Exit is always visible.

## Search, discovery, and online sources

- [ ] Provider selection for YouTube and SoundCloud.
- [ ] Search edit Enter starts search and focuses the complete result list.
- [ ] Stale-search generations cannot replace a newer search.
- [ ] Results preserve title, type, channel/author, duration, upload time, views,
  live-stream state, URL, source IDs, and source-specific metadata.
- [ ] Configurable result count and unlimited dynamic mode.
- [ ] Dynamic batches use current batch settings and append without replacing or
  refocusing existing rows.
- [ ] End/down-at-end loading for search, folder, channel, playlist, popular,
  podcast, and other paged collections.
- [ ] Next can request the next result page without skipping, random selection,
  or losing the focused result.
- [ ] YouTube trending with country and category filters.
- [ ] Trending behavior with and without YouTube Data API key.
- [ ] YouTube videos, live streams, Shorts, playlists, and channels.
- [ ] Channel tabs: videos, playlists, and all-time popular videos.
- [ ] All-time popular loads the whole channel progressively and orders by views,
  not only the newest page.
- [ ] Playlist open, play, shuffle, queue, download, and return behavior.
- [ ] SoundCloud tracks, playlists/sets, users/artists, and artist tracks.
- [ ] Exact selected item starts even while metadata or another page is loading.
- [ ] Result metadata hydration runs in bounded batches and updates every loaded
  page, including channel and playlist results.
- [ ] Upload-time `unknown` is replaced when metadata arrives without blocking
  initial list display.
- [ ] Open channel, subscribe/unsubscribe, favorite, user playlist, queue,
  download, copy, browser, and source-specific context actions.
- [ ] Direct playback and download for yt-dlp-supported URLs.
- [ ] Direct MP3/MP4/M3U8/media fallback to mpv when generic extraction fails.
- [ ] Normal extraction fast path before requested-format recovery, alternate
  client, EJS, cookies, or browser-cookie fallback.
- [ ] Cookie fallback only for relevant authentication/age/JS failures.
- [ ] Stream URL cache keys include source and stream-format preference.
- [ ] Stream cache expiration, HLS manifest expiration, invalidation, and
  cross-session safety.
- [ ] Next-item stream prefetch does not reorder, skip, or slow current playback.
- [ ] Automatic, Prefer video, and Prefer audio select distinct documented
  formats while preserving highest practical audio quality and seeking.
- [ ] Progressive, dual HLS video/audio, direct audio, requested-format,
  web-safari, truncated-stream, and fallback selection match 1.0.21 behavior.
- [ ] Proxy, rate limit, retries, timeout, fragments, user agent, FFmpeg path,
  cookies file, browser/profile cookies, and age-restricted option.

## Player and playback behavior

- [ ] Internal mpv playback for YouTube, SoundCloud, direct links, local files,
  folders, podcasts/RSS, AudioVault, history, favorites, playlists, bookmarks,
  subscriptions, notifications, queue, and restored sessions.
- [ ] Audio-only and embedded video modes.
- [ ] Optional external player/browser path with current confirmation/fallback.
- [ ] Play/pause, restart after end, previous, next, related next, repeat,
  shuffle, autoplay next, autoplay related, and session-only autoplay.
- [ ] Queue has documented precedence and consumes only the intended item.
- [ ] Previous/next use exact current source sequence and never skip valid items.
- [ ] True source end announces no next/previous item only when no page/item/queue
  can satisfy the action.
- [ ] Local folder queue is created only by Play entire folder or Shuffle folder.
- [ ] Playlist queue/sequence is created only when playlist playback starts.
- [ ] Small seek uses configured seconds.
- [ ] Large and huge seeks preserve current increments.
- [ ] Held left/right scrubbing is smooth, bounded, immediate, and backlog-free.
- [ ] Keyframe seeking is used for ordinary/held seeks; chapters, bookmarks,
  transcript rows, markers, start/end, and resume use exact jumps where current
  behavior requires it.
- [ ] Jump to start and end.
- [ ] Immediate elapsed/remaining/total updates after seeks.
- [ ] Multi-hour local and online media remain seekable without lockup.
- [ ] Player session volume begins at configured default and persists across new
  items until the player session actually closes.
- [ ] Volume boost changes maximum between 100 and 300 without startup spikes or
  stuck controls.
- [ ] Output device persists for the player session across searches/sources and
  falls back safely when unavailable.
- [ ] Speed and pitch use configured defaults, steps, modes, hold delay, and hold
  interval; held input cannot create delayed announcements or pauses.
- [ ] Reset speed/pitch returns speed to configured default and pitch to 1.0.
- [ ] Podcast per-feed speed presets override/use global exactly as today.
- [ ] Rubberband, scaletempo2, scaletempo, and mpv-default modes.
- [ ] Gapless playback and autoplay-next remain separate settings/behaviors.
- [ ] ReplayGain off/track/album modes.
- [ ] Resume positions and last session bind to the correct media identity.
- [ ] Start paused, playback-start announcement, optional play/pause announcement,
  and playback-finished announcement.
- [ ] Local-file edit mode, save edited copy, replace original, progress,
  cancellation, safe temp file, and rollback.
- [ ] Live and exported edit-mode speed/pitch/EQ/clipping chain are equivalent.
- [ ] Format announcement reports container, resolution, codecs, channels, sample
  rate, and stable nominal bitrate where available.
- [ ] Details use source-appropriate labels and Copy details.

## Audio processing and equalizer

- [ ] Independent 31 Hz band.
- [ ] Independent 62 Hz band.
- [ ] Independent 125 Hz band.
- [ ] Independent 250 Hz band.
- [ ] Independent 500 Hz band.
- [ ] Independent 1000 Hz band.
- [ ] Independent 2000 Hz band.
- [ ] Independent 4000 Hz band.
- [ ] Independent 8000 Hz band.
- [ ] Independent 16000 Hz band.
- [ ] Changing one slider never changes any other stored or visible gain.
- [ ] Player-session and global EQ use the same gain/filter model.
- [ ] 6, 12, 18, and 24 dB ranges with correct steps, clamps, visible values,
  accessible values, and spoken signed dB values.
- [ ] Flat plus every current factory preset.
- [ ] Three current custom slots plus additional current profile behavior.
- [ ] Create with blank name field, save, rename, edit, delete, import, export,
  and cancel without modifying Flat or another profile.
- [ ] A/B comparison and restoration.
- [ ] Device-linked presets and missing-device fallback.
- [ ] Bass boost preserves/restores prior EQ state and remains compatible with
  volume boost.
- [ ] Clipping protection accounts for positive EQ/preamp/boost gain and does not
  collapse stereo, duck vocals unexpectedly, or cause startup crackle.
- [ ] ReplayGain, volume boost, bass boost, EQ, speed, and pitch combinations.
- [ ] Filter replacement is atomic enough to avoid stalls, watery artifacts,
  temporary double filters, or a loud unprotected frame.
- [ ] Fresh BPM analysis every time the BPM action is invoked.
- [ ] Unavailable/low-confidence BPM state is announced accurately.

## Media information and reading features

- [ ] Online and local details with title, type, source, channel/artist, duration,
  upload date/time, views, path/URL, and available technical metadata.
- [ ] Elapsed, remaining, total, live-stream state, and unavailable timing.
- [ ] YouTube chapters and podcast chapters.
- [ ] Chapter list, Enter-to-jump-and-close, previous/next chapter, Escape return,
  and no-chapters state.
- [ ] Timed online transcripts/captions with rate-limit fallback/error behavior.
- [ ] Local VTT/SRT sidecars.
- [ ] Transcript search, selection, Enter-to-jump, copy line, copy all, export,
  and timestamp link.
- [ ] Local and online lyrics, read-only navigation, copy, export, synchronization
  where currently supported, and unavailable state.
- [ ] YouTube comments through API or yt-dlp fallback.
- [ ] Comment sorting, filtering/search, pagination, details, replies, copy one,
  copy visible, author channel, rate-limit/unavailable state, and focus return.
- [ ] Playback bookmarks: add, list all/current, rename, delete, play/jump, resume,
  copy timestamp, and context menu.
- [ ] Marker start/end, clearing/toggling, marked preview, audio/video export, and
  preservation/restoration of normal playback state.
- [ ] Copy page link, timestamped link, local path, and direct stream URL only in
  contexts where each is meaningful.

## Local media, collections, and library

- [ ] Open supported individual audio/video files from Apricot and Explorer.
- [ ] Open folder and enumerate all supported current media extensions.
- [ ] Natural numeric sorting (`name`, `name(1)`, `name(2)`, `name(10)`).
- [ ] Play selected, Play entire folder, and Shuffle folder.
- [ ] Folder dynamic batches and large-folder bounded metadata cache.
- [ ] File associations, Open With, default-player guidance, and safe registry
  operations.
- [ ] Favorites add/remove/list/play/copy/queue/playlist/download actions.
- [ ] History of played and downloaded items, limit, clear/remove, direct URLs
  surviving restart, and history-disabled behavior.
- [ ] User playlists create, rename where current UI supports it, add/remove,
  open, play, shuffle, queue, download, and persistence.
- [ ] Playback queue add/remove/reorder/clear/play, persisted state, and mixed
  source items.
- [ ] Named bookmarks and resume positions remain independent per item.
- [ ] Last player session restores item, sequence, return route, index, player
  options, and resume position safely.
- [ ] YouTube subscriptions add/remove/category/sort/manual refresh/automatic
  refresh/new-video view/notifications/playback/context actions.
- [ ] Notification center list, open, mark/read behavior where applicable,
  remove/clear, and source return.

## Podcasts and RSS

- [ ] Apple Podcasts directory search with provider, country, and result limit.
- [ ] Add direct RSS/Atom URL and import/export OPML.
- [ ] Feed add/remove/refresh/manual refresh/refresh on startup/automatic interval.
- [ ] Categories, filtering, alphabetical grouped ordering, and focus preservation
  after refresh/reorder.
- [ ] Full feed archive beyond 500 when the source provides it.
- [ ] `rss_max_items` controls visible batch size, not archive truncation.
- [ ] End/down-at-end appends older episodes while preserving focus.
- [ ] Played/unplayed state, progress marker, reset progress, and resume.
- [ ] Per-feed speed preset save/use/reset behavior.
- [ ] Podcast chapters, details, transcript/lyrics where available.
- [ ] Feed/episode play, queue, favorite/playlist where current UI allows it,
  download episode/feed, copy URL, and open browser.
- [ ] New episode notifications without reporting historical archive migration as
  new content.
- [ ] Malformed/huge XML, redirects, network errors, and empty feeds.

## AudioVault

- [ ] First-use login focuses the email field.
- [ ] Persistent Windows-protected credentials and explicit logout.
- [ ] Register opens the exact registration page.
- [ ] Missing/expired session returns to login and retries only after success.
- [ ] Search mode selection for movies and TV shows.
- [ ] Recently viewed movies and shows; Enter equals Open.
- [ ] Search results, show details, episodes, and one-step return navigation.
- [ ] Movie/episode streaming through the internal player.
- [ ] AudioVault pages never surface login URLs as playable media.
- [ ] History, queue, copy/open actions appropriate to the source.
- [ ] Individual movie/episode audio download.
- [ ] Whole-show download with independent progress and cancellation.
- [ ] Global Ask where to save setting applies to AudioVault.
- [ ] Safe archive/cache extraction, final file copy, collision handling, and
  cleanup without closing the player/application.
- [ ] AudioVault rows omit irrelevant YouTube upload labels.
- [ ] Authentication, unavailable content, malformed pages/manifests, partial
  show failures, and network interruption.

## Downloads and conversion

- [ ] Single audio download in MP3, M4A, Opus, WAV, and FLAC modes.
- [ ] Audio quality setting and honest source-quality/transcode behavior.
- [ ] Single video download in every current video-format and height mode.
- [ ] Playlist, channel, selected results, podcast feed, and AudioVault show batch
  downloads.
- [ ] One child failure is announced/recorded and remaining batch items continue.
- [ ] Final batch success/failure/cancel summary.
- [ ] Current downloads list and independent progress window.
- [ ] Item progress, aggregate progress, processing state, hide, details,
  cancellation, completion, and continued app/player use.
- [ ] Default download folder and Ask every time for every source and export.
- [ ] Quiet downloads and confirmation-before-download behavior.
- [ ] Keep playlist order and safe collision numbering.
- [ ] Filename template validation and restricted filenames.
- [ ] Thumbnail, description, info JSON, subtitles, automatic subtitles,
  subtitle languages, metadata, embedded thumbnail, and download archive.
- [ ] Open folder after completion and download/conversion completion popups.
- [ ] Fast path and fallback path preserve current download throughput.
- [ ] Requested-format unavailable fallback only when needed.
- [ ] File converter detects input and offers valid output formats.
- [ ] Folder converter, recursion/current behavior, progress, cancellation, safe
  replacement/new destination, collisions, and partial failures.
- [ ] Audio formats: MP3, M4A, AAC, WAV, FLAC, OGG, Opus, WMA, AIFF, ALAC, AC3,
  and MP2.
- [ ] Video formats: MP4, MKV, WebM, MOV, AVI, WMV, M4V, MPG/MPEG, FLV, 3GP, OGV,
  TS, M2TS, and ASF.
- [ ] Image-backed audio-to-video where current converter supports it.
- [ ] Marked clip and edit-mode export codec behavior.

## Settings UI behavior

- [ ] Sections: General, Customize main menu, Playback, Equalizer, Downloads,
  Library, Podcasts, Notifications, Cookies/Network, AudioVault, Shortcuts.
- [ ] Initial focus is the section list exactly once.
- [ ] Tab from section list enters the selected section's first control.
- [ ] Shift+Tab returns to the section list with one announcement.
- [ ] Section changes apply visible values without losing unsaved edits.
- [ ] Every checkbox reports checked/unchecked with Space and Enter behavior
  matching standard controls.
- [ ] Customize main menu uses actual checkboxes navigable by arrows and Space.
- [ ] Shortcut editor captures, validates, rejects conflicts, restores defaults,
  and remains fully keyboard accessible.
- [ ] Reset current section resets exactly that section and updates controls.
- [ ] Reset all resets every field, confirms appropriately, and updates menu,
  shortcuts, audio, updater, and feature visibility.
- [ ] Save persists normalized values atomically; Cancel preserves prior values.

## Exact settings field inventory

These JSON IDs are compatibility keys. A platform-neutral UI label may differ,
but Rust must continue to read them and must not discard unknown fields.

General and identity:

- [ ] `language`
- [ ] `download_folder`
- [ ] `results_limit`
- [ ] `direct_link_enter_action`
- [ ] `show_shortcuts_in_labels`
- [ ] `main_menu_hidden_actions`
- [ ] `media_association_prompted_version`
- [ ] `language_prompted`

Playback:

- [ ] `player_command`
- [ ] `autoplay_next`
- [ ] `autoplay_related`
- [ ] `prefer_browser_playback`
- [ ] `player_fullscreen`
- [ ] `player_start_paused`
- [ ] `announce_play_pause`
- [ ] `announce_playback_finished`
- [ ] `enable_background_playback`
- [ ] `player_speed`
- [ ] `speed_audio_mode`
- [ ] `show_video_details_by_default`
- [ ] `enable_age_restricted_videos`
- [ ] `enable_stream_cache`
- [ ] `enable_stream_url_cache`
- [ ] `stream_url_cache_minutes`
- [ ] `stream_format_preference`
- [ ] `prefetch_next_stream_url`
- [ ] `gapless_playback`
- [ ] `replaygain_mode`
- [ ] `enable_online_lyrics`
- [ ] `cache_folder`
- [ ] `cache_size_mb`
- [ ] `resume_playback`
- [ ] `show_resume_in_menu`
- [ ] `audio_output_device`
- [ ] `speed_step`
- [ ] `pitch_step`
- [ ] `speed_pitch_hold_delay_ms`
- [ ] `speed_pitch_hold_interval_ms`
- [ ] `pitch_mode`
- [ ] `seek_seconds`
- [ ] `volume_step`
- [ ] `default_volume`
- [ ] `volume_boost_by_default`

Equalizer:

- [ ] `global_equalizer_enabled`
- [ ] `global_equalizer_preset`
- [ ] `global_equalizer_gains`
- [ ] `equalizer_preset_gains`
- [ ] `equalizer_custom_names`
- [ ] `equalizer_device_presets`
- [ ] `equalizer_db_range`
- [ ] `equalizer_clipping_protection`

Downloads and conversion:

- [ ] `audio_format`
- [ ] `video_format`
- [ ] `max_video_height`
- [ ] `ask_download_location_each_time`
- [ ] `quiet_downloads`
- [ ] `keep_playlist_order`
- [ ] `filename_template`
- [ ] `audio_quality`
- [ ] `write_thumbnail`
- [ ] `write_description`
- [ ] `write_info_json`
- [ ] `write_subtitles`
- [ ] `auto_subtitles`
- [ ] `subtitle_languages`
- [ ] `embed_metadata`
- [ ] `embed_thumbnail`
- [ ] `restrict_filenames`
- [ ] `open_folder_after_download`
- [ ] `popup_when_download_complete`
- [ ] `popup_when_conversion_complete`
- [ ] `confirm_before_download`
- [ ] `download_archive`

Updates:

- [ ] `auto_update_ytdlp`
- [ ] `auto_update_app`
- [ ] `app_update_interval_hours`
- [ ] `app_update_notifications`
- [ ] `skipped_update_version`
- [ ] `update_channel`

Cookies and network:

- [ ] `rate_limit`
- [ ] `proxy`
- [ ] `youtube_data_api_key`
- [ ] `cookies_file`
- [ ] `cookies_source_file`
- [ ] `cookies_source_signature`
- [ ] `cookies_from_browser`
- [ ] `cookies_browser_profile`
- [ ] `show_advanced_network_settings`
- [ ] `cookie_user_agent`
- [ ] `ffmpeg_location`
- [ ] `concurrent_fragments`
- [ ] `retries`
- [ ] `socket_timeout`

AudioVault credentials:

- [ ] `audiovault_email`
- [ ] `audiovault_password_protected`

Windows shell and notifications:

- [ ] `close_to_tray`
- [ ] `start_with_windows`
- [ ] `tray_notification`
- [ ] `windows_notifications`
- [ ] `download_notifications`
- [ ] `subscription_notifications`

Library, subscriptions, and podcasts:

- [ ] `subscription_check_enabled`
- [ ] `subscription_check_interval_hours`
- [ ] `last_subscription_check`
- [ ] `enable_trending`
- [ ] `enable_history`
- [ ] `enable_podcasts_rss`
- [ ] `podcast_search_provider`
- [ ] `podcast_search_country`
- [ ] `podcast_search_limit`
- [ ] `rss_max_items`
- [ ] `rss_refresh_on_startup`
- [ ] `rss_auto_refresh_enabled`
- [ ] `rss_refresh_interval_hours`
- [ ] `history_limit`

Shortcut map:

- [ ] `keyboard_shortcuts`

## Exact shortcut action inventory

The values below are current Windows defaults. User overrides remain data, not
hard-coded UI behavior. The later macOS adapter maps logical primary `Ctrl` to
`Command`, with documented conflict exceptions.

Global and navigation actions:

- [ ] `open_main_menu` = `Ctrl+Alt+M`
- [ ] `open_search` = `Ctrl+Alt+Y`
- [ ] `open_audiovault` = `Ctrl+Alt+A`
- [ ] `open_play_from_folder` = `Ctrl+Alt+O`
- [ ] `open_play_file` = `Ctrl+Alt+I`
- [ ] `open_direct_link` = `Ctrl+Alt+L`
- [ ] `open_favorites` = `Ctrl+Alt+F`
- [ ] `open_bookmarks` = `Ctrl+Alt+K`
- [ ] `open_playlists` = `Ctrl+Alt+P`
- [ ] `open_subscriptions` = `Ctrl+Alt+B`
- [ ] `open_current_downloads` = `Ctrl+Alt+D`
- [ ] `open_history` = `Ctrl+Alt+H`
- [ ] `open_podcasts_rss` = `Ctrl+Alt+R`
- [ ] `open_settings` = `Ctrl+Alt+S`
- [ ] `open_action_finder` = `Ctrl+Shift+J`
- [ ] `background_play_pause` = `Ctrl+Space`
- [ ] `copy_diagnostic_report` = `Ctrl+Alt+Shift+D`

List, result, collection, and download actions:

- [ ] `download_audio` = `Ctrl+Shift+A`
- [ ] `download_video` = `Ctrl+Shift+D`
- [ ] `subscribe_channel` = `Ctrl+Shift+S`
- [ ] `unsubscribe_channel` = `Ctrl+Shift+U`
- [ ] `open_channel` = `Ctrl+Shift+O`
- [ ] `queue_audio` = `Shift+A`
- [ ] `result_column_previous` = `Ctrl+Alt+Left`
- [ ] `result_column_next` = `Ctrl+Alt+Right`
- [ ] `add_to_playback_queue` = `Ctrl+Shift+Q`
- [ ] `remove_from_playback_queue` = `Ctrl+Shift+Delete`
- [ ] `open_playback_queue` = `Ctrl+Alt+Q`
- [ ] `create_playlist` = `Ctrl+Shift+N`
- [ ] `add_favorite` = `Ctrl+F`
- [ ] `remove_favorite` = `Ctrl+Shift+F`
- [ ] `add_to_playlist` = `Ctrl+P`
- [ ] `remove_from_playlist` = `Ctrl+Shift+P`
- [ ] `copy_link` = `Ctrl+L`
- [ ] `copy_stream_url` = `Ctrl+D`
- [ ] `context_menu` = `Applications`
- [ ] `open_selected` = `Enter`
- [ ] `new_subscription_videos` = `Ctrl+Shift+V`
- [ ] `remove_selected` = `Delete`
- [ ] `toggle_podcast_played` = `Ctrl+Shift+X`
- [ ] `clear_podcast_progress` = `Ctrl+Shift+R`
- [ ] `save_podcast_speed_preset` = `Ctrl+Shift+E`

Player information and commands:

- [ ] `player_copy_link` = `L`
- [ ] `player_copy_timestamp_link` = `Ctrl+Shift+L`
- [ ] `player_play_pause` = `Space`
- [ ] `player_time` = `T`
- [ ] `player_bpm` = `B`
- [ ] `player_speed_down` = `S`
- [ ] `player_speed_up` = `D`
- [ ] `player_reset_speed_pitch` = `Ctrl+0`
- [ ] `player_pitch_up` = `Ctrl+Up`
- [ ] `player_pitch_down` = `Ctrl+Down`
- [ ] `player_volume_status` = `V`
- [ ] `player_format_status` = `F`
- [ ] `player_details` = `F7`
- [ ] `player_output_devices` = `O`
- [ ] `player_equalizer` = `F4`
- [ ] `player_fullscreen` = `F11`
- [ ] `player_replaygain` = `Ctrl+Shift+G`
- [ ] `player_add_bookmark` = `Ctrl+Shift+B`
- [ ] `player_bookmarks` = `Ctrl+Shift+K`
- [ ] `player_chapters` = `Ctrl+Shift+C`
- [ ] `player_transcript` = `Ctrl+Shift+T`
- [ ] `player_lyrics` = `Ctrl+Shift+Y`
- [ ] `player_comments` = `Ctrl+Shift+M`
- [ ] `player_previous_chapter` = `Alt+Left`
- [ ] `player_next_chapter` = `Alt+Right`
- [ ] `player_edit_mode` = `E`
- [ ] `player_save_edit_copy` = `Ctrl+S`
- [ ] `player_replace_edit_original` = `Ctrl+R`
- [ ] `player_marker_start` = `LeftBracket`
- [ ] `player_marker_end` = `RightBracket`
- [ ] `player_preview_marked_clip` = `P`
- [ ] `player_previous` = `Ctrl+PageUp`
- [ ] `player_next` = `Ctrl+PageDown`
- [ ] `player_next_related` = `Ctrl+Shift+PageDown`
- [ ] `player_back` = `Escape`
- [ ] `player_volume_boost` = `F2`
- [ ] `player_bass_boost` = `F3`
- [ ] `player_repeat` = `R`
- [ ] `player_shuffle` = `Shift+S`

Player seek and volume actions:

- [ ] `player_seek_back` = `Left`
- [ ] `player_seek_forward` = `Right`
- [ ] `player_seek_back_large` = `Ctrl+Left`
- [ ] `player_seek_forward_large` = `Ctrl+Right`
- [ ] `player_seek_back_huge` = `Ctrl+Shift+Left`
- [ ] `player_seek_forward_huge` = `Ctrl+Shift+Right`
- [ ] `player_seek_start` = `Ctrl+Home`
- [ ] `player_seek_end` = `Ctrl+End`
- [ ] `player_volume_up` = `Up`
- [ ] `player_volume_down` = `Down`

## Persistent files and data operations

- [ ] `settings.json`.
- [ ] `favorites.json` plus legacy favorites migration.
- [ ] `bookmarks.json`.
- [ ] `history.json`.
- [ ] `subscriptions.json`.
- [ ] `rss_feeds.json`.
- [ ] `playlists.json`.
- [ ] `notifications.json`.
- [ ] `playback_positions.json`.
- [ ] `playback_queue.json`.
- [ ] `last_player_session.json`.
- [ ] `stream_url_cache.json`.
- [ ] `cookies.txt`, configured source path, and source signature behavior.
- [ ] `download-archive.txt`.
- [ ] cache folder and size/cleanup behavior.
- [ ] `components` state for yt-dlp updates.
- [ ] updater/error/mpv logs with rotation/size limits.
- [ ] legacy `UrhasaurusYouTubePlayer` settings/favorites import.
- [ ] atomic writes, generation guards, corrupt-file preservation, backups, and
  migration rollback for every durable collection.

## System integration, diagnostics, and updates

- [ ] Start with Windows, close to tray, tray notification, tray menu, restore,
  and Exit.
- [ ] Windows notifications and separate download/subscription/app notification
  preferences.
- [ ] Output-device enumeration and refresh.
- [ ] File associations/default-app repair without unsafe registry writes.
- [ ] Diagnostic report includes versions, paths redacted, player/session/audio,
  current item, results/queue, settings, and bounded logs.
- [ ] Stable channel never offers beta; beta channel handles both according to
  current version policy.
- [ ] Update interval, notifications, skip version, manual update, release notes,
  progress, restart, and offline/error states.
- [ ] App update requires trusted HTTPS metadata/redirects, expected asset names,
  hash/signature, bounded download, strict ZIP/installer layout, transactional
  replacement of executable plus runtime, and rollback.
- [ ] yt-dlp automatic/manual update, version comparison, bounded verified
  package, compatibility, restart/reload behavior, and diagnostics.
- [ ] Installer and portable ZIP are built from the same final commit.

## Security and robustness

- [ ] No untrusted string enters a shell command.
- [ ] All child-process arguments are structured and executable paths trusted.
- [ ] IPC messages and responses are bounded and timed out.
- [ ] Remote responses are bounded; redirects and final URLs validated.
- [ ] XML cannot resolve DTDs/entities/external references.
- [ ] Cookie import/export includes only approved YouTube/Google domains and
  rejects malformed/control-character records.
- [ ] Browser profile handling and Chrome DPAPI errors are localized and safe.
- [ ] Archive extraction rejects traversal, links/reparse points, ADS, devices,
  duplicate paths, special/encrypted files, bombs, and oversized members.
- [ ] Generated paths and templates reject reserved names and escapes.
- [ ] Update metadata/assets are authenticated and rollback tested.
- [ ] Diagnostics redact credentials, cookie values/signatures, proxy users,
  sensitive URL query/fragment values, and user profile paths.
- [ ] Caches are bounded and cleanup cannot escape owned directories or follow
  links/reparse points.
- [ ] Stale asynchronous writes/results cannot overwrite newer state.
- [ ] Every `unsafe` Rust block has a reviewed invariant and minimal scope.

## Performance and soak evidence

- [ ] Cold/warm startup baseline and Rust comparison.
- [ ] Local and YouTube result-to-player latency comparison.
- [ ] Escape/route/focus transition latency comparison.
- [ ] Seek, volume, speed, pitch held-key latency/backlog test.
- [ ] Large result, channel, playlist, and 14,000-file folder test.
- [ ] Multi-hour stream immediate seek/resume test.
- [ ] Download throughput comparison with Python on the same media/network.
- [ ] Main menu, audio, video, large folder, and long-session memory comparison.
- [ ] Paused/playing CPU and idle wakeup comparison.
- [ ] 8-hour playback/navigation/download soak with bounded tasks, handles,
  memory, caches, and logs.

## Final parity gate

- [ ] No unchecked item or undocumented approved OS exception.
- [ ] Machine-generated registry counts match the frozen Python baseline.
- [ ] Full automated suite passes from a clean checkout.
- [ ] Full NVDA keyboard-only acceptance passes on the installed local beta.
- [ ] Real-source and real-account acceptance passes.
- [ ] Python data migration plus rollback passes on a copied real profile.
- [ ] Performance gates pass without adding normal-path delay.
- [ ] Clean install, side-by-side beta, update, rollback, uninstall, associations,
  second launch, tray/background, and offline tests pass.
- [ ] User explicitly approves the first public Rust 2.0 release.
