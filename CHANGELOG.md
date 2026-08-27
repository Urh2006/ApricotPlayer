# v1.0.15 - Robust Stream Formats & Seamless Seeking Buffer

- Fixed: Prioritized unthrottled progressive MP4 streams (format 18 / 360p) for rock-solid instant seeking across YouTube videos.
- Fixed: Resolved stream freezing and EOF lockup on M4A DASH audio streams by removing disk caching stalls and enabling smooth RAM buffering on network seeks.

# v1.0.14 - Instant Seeking & Resume Position on Long M4A/YouTube Streams

- Fixed: Eliminated freezing, stalls, and playback lockups when seeking immediately after opening or starting long M4A/YouTube streams (2+ hours) by configuring fast keyframe-level seeking.
- Fixed: Resolved issue where opening a stream with a late saved resume position (e.g. elapsed 2:23:00) failed to start by preventing linear byte-range decoding over HTTP.

# v1.0.13 - Lossless MPV Native PCM Audio Capture & Studio Encoding

- Fixed: Eliminated subtle amplitude fluctuation, tremolo effect, and phase beating when saving edited files by capturing MPV's exact uncompressed 32-bit floating point audio driver output (--ao=pcm) before encoding.
- Changed: Seamlessly integrated MPV's real-time audio pipeline with FFmpeg high-bitrate output encoding (320 kbps MP3, 256 kbps AAC, FLAC), ensuring 100% acoustic identicality to live playback.

# v1.0.12 - Stutter-Free, Seamless Audio Export in Edit Mode

- Fixed: Eliminated periodic audio stutter, buffer underruns, and frame repetition when saving edited files by configuring laminar phase preservation, compound transient detection, and transition smoothing.
- Fixed: Ensured perfectly continuous sample-accurate playback duration and zero dropout rate across all audio formats.

# v1.0.11 - Direct MPV Native Engine File Export in Edit Mode

- Changed: Switched Edit Mode file export to directly use MPV's native rendering engine (--pitch, --audio-pitch-correction=yes, --speed, and live equalizer), guaranteeing identical acoustic results between live playback and exported files.
- Added: Automatic fallback to FFmpeg when MPV is unavailable.

# v1.0.10 - Studio-Quality Pitch-Shifted File Export in Edit Mode

- Fixed: Eliminated audio degradation, phase smearing, and metallic distortion when saving edited files with changed pitch by replacing legacy asetrate resampling with high-fidelity rubberband pitch filtering.
- Changed: Configured FFmpeg export to use rubberband pitch processing with maximum quality mode, preserving natural harmonics and original sample rate.

# v1.0.9 - Bitrate Reporting in Format Announcement

- Added: Format announcement (shortcut F) now includes the exact audio bitrate in kbps across all playback sources (local MP3, M4A, FLAC, AAC, YouTube, podcasts, and online streams).
- Added: Integrated local file metadata inspection via mutagen with live playback stream metrics to guarantee accurate bitrate reporting.

# v1.0.8 - Player Format Announcement Shortcut

- Added: Added player shortcut F (Player: announce format) to speak the active container, video resolution, and audio codec to screen readers during playback.
- Added: Configurable format announcement shortcut in Settings > Keyboard shortcuts under player actions.

# v1.0.7 - Smooth Seeking, Instant Resume, and Stream Preference

- Fixed: Eliminated playback stalls and socket freezes when jumping forward or backward by removing force-seekable socket constraints on HTTP streams.
- Fixed: Restored instant start when resuming playback from a saved position by prioritizing index-based M4A and MP4 streams that support direct HTTP range seeking.
- Fixed: Prioritized fast M4A audio formats over WebM to prevent cluster search latency during rapid seeking.
- Added: Added Playback stream format option in Settings > Playback allowing users to choose between Automatic, Prefer video format, or Prefer audio-only.

# v1.0.6 - Smooth Seeking and Buffering Elimination

- Fixed: Eliminated buffering stalls when seeking forward and backward on YouTube, podcasts, and direct web streams by enabling 2-minute readahead pre-buffering.
- Fixed: Prioritized unthrottled progressive streams for fast initial buffer fill.
- Changed: Increased back-cache memory to ensure backward seeking is instant without re-requesting data from remote servers.
- Changed: Enabled forced seekable stream handling for direct HTTP/HTTPS media connections.

# v1.0.5 - Faster Playback Start and Smooth Seeking

- Fixed: Stream format selection now includes direct high-quality audio fallback on the very first attempt, cutting video startup latency in half on YouTube tracks without legacy formats.
- Changed: Seeking forward and backward is instant and smooth with immediate audio buffer preloading.
- Changed: Automatically selects the highest available audio quality from YouTube for fast playback.

# v1.0.4 - yt-dlp 2026.8.19, Robust JS Runtime & Direct Link Playback

## Fixed
- **Restored YouTube stream extraction & playback.** Bundled Node.js executable discovery now searches all runtime and package layouts (`_internal/node`, `vendor/node`, local paths), allowing yt-dlp to solve YouTube JS challenges and extract playable progressive and adaptive formats.
- **Fixed playback failure on direct media links from other websites.** When yt-dlp generic webpage extraction fails or returns HTTP 403 on direct audio/video/stream URLs (MP3, MP4, M3U8, AAC), ApricotPlayer falls back directly to mpv streaming.
- **Improved YouTube format fallback.** Added audio and adaptive stream fallback to ensure playback never fails with "Requested format is not available" when progressive streams are absent.

## Changed
- Updated the bundled and pinned yt-dlp dependency to 2026.8.19.
- Enhanced runtime executable resolution for Node and FFmpeg across development, portable, and installed environments.

## Verification
- Added unit tests for bundled Node/FFmpeg executable resolution and direct media URL stream fallback.
- Verified YouTube playback, SoundCloud, direct MP3, direct MP4, and direct M3U8 stream resolution.
- Passed all 108 automated tests, source compilation, critical Ruff checks, and whitespace validation.

# v1.0.3 - Complete Podcast Archives

## Fixed
- Podcast and RSS feeds no longer discard episodes beyond the first 500 when the feed itself contains a larger archive.
- Existing feeds saved by an older ApricotPlayer version are refreshed once when opened so previously truncated archives can be restored.
- Restoring an older archive does not report its historical episodes as newly published notifications.

## Changed
- The former maximum-episode setting now controls how many podcast episodes are shown in each batch, from 25 to 500.
- Pressing End or moving down from the last visible episode appends the next batch while preserving list focus and screen-reader navigation.

## Verification
- Added parsing, archive migration, notification, dynamic loading, and real wx list regression coverage.
- Passed all 105 automated tests, source compilation, critical Ruff checks, and whitespace validation.

# v1.0.0-beta.47 - Categorized Libraries and Accessible Media Fields

## Added
- Saved YouTube subscriptions and RSS/podcast feeds can now be assigned a category. Entering a category creates it, clearing the field removes it, and the category is retained when RSS feeds refresh.
- Subscription and RSS screens include category filters. Stored collections group by category and then title while preserving focus on the selected item after reordering.
- `Ctrl+Alt+Left` and `Ctrl+Alt+Right` announce the previous or next individual metadata field for the selected media item. They work in search results, local folders, favorites, history, subscriptions, RSS, podcast, playlist, and queue lists, and are configurable under Settings > Keyboard shortcuts.
- The main menu and Action Finder now identify the search screen as `Search YouTube / SoundCloud`, making the existing provider selector discoverable.

## Completed Issues
- Podcast catalogue category browsing, SoundCloud search, YouTube Shorts URL playback, main-menu focus restoration, and the updater version mismatch report are verified in the current codebase.

## Verification
- Added offline regression coverage for category filtering/sorting, focused RSS reordering, individual media fields, SoundCloud search routing, and Shorts URL parsing.

# v1.0.0-beta.46 - Repository Cleanup and Library Ordering

## Fixed
- Saved YouTube subscriptions and saved RSS/podcast feeds now sort alphabetically by title when loaded and whenever they are saved.
- Updated the release and security workflows to current SHA-pinned `actions/checkout` and `actions/setup-python` releases, removing the obsolete Node 20 action runtime warning.

## Maintenance
- Removed the unused pre-wx Tkinter app, PyInstaller extraction, and one-off refactor scripts from the repository.
- Added narrow ignore rules for local recovery, extraction, diagnostic, and old-archive artifacts so they cannot be committed accidentally.

## Verification
- Added regression coverage for persisted collection ordering and ran the full test suite, dependency audit, critical Ruff checks, source startup, and a release build.

# v1.0.0-beta.45 - Security, Data Integrity, and Updater Hardening

## Security
- App and yt-dlp updates now require published SHA-256 digests, trusted HTTPS redirect hosts, bounded downloads, and validated package layouts before installation.
- ZIP validation now rejects traversal, absolute paths, alternate data streams, reserved Windows devices, duplicate paths, encrypted entries, links, special files, oversized members, and compression bombs.
- Portable updates now lock the verified archive while extracting it, replace both `ApricotPlayer.exe` and `_internal` as one transaction, and restore both components after a failed copy. The legacy executable updater also preserves its original rollback copy across retries.
- XML imports and podcast/RSS parsing now use `defusedxml` with DTD, entity, and external-reference protection. Remote feeds, chapters, transcripts, lyrics, API responses, pages, and DevTools responses have explicit size limits and redirect validation.
- Browser-cookie export now requests and stores only YouTube/Google-related cookies, validates exact cookie domains and fields, and rejects control-character injection. Diagnostic reports remove credentials, URL query/fragment secrets, proxy user information, cookie signatures, and user-profile paths.
- Windows helper executables resolve from trusted system locations. Release dependencies and runtimes are version-locked, hash-checked, and covered by a weekly dependency/security workflow.

## Reliability and Performance
- Settings, history, favorites, bookmarks, queues, playback positions, playlists, subscriptions, notifications, the last player session, and stream-cache state now use atomic replacement writes. Generation guards prevent older background saves from overwriting newer state.
- mpv IPC reads are now genuinely nonblocking and bounded, so a missing response respects its timeout instead of hanging on `readline()`.
- Custom mpv cache cleanup is restricted to ApricotPlayer's own child directory and avoids links/reparse points. Local-folder metadata caching is bounded.
- Download templates, generated media extensions, Windows device namespaces, and generated folder names are validated before filesystem use.
- Updated the bundled runtimes to yt-dlp 2026.7.4, mpv 0.41.0-744-g304426c39, FFmpeg N-125515-g35f8f4bdc0, and Node.js 24.18.0 LTS.

## Verification
- Added 26 focused security regression tests, dependency auditing, critical Ruff checks, and Dependabot configuration.
- Verified source GUI startup/shutdown, real mpv IPC, EQ plus limiter filters, Rubberband speed/pitch, current yt-dlp extraction, FFmpeg media generation, strict portable-package validation, and both successful and forced-rollback updater paths.
- Player startup timing, seek/cache policy, volume, bass boost, clipping protection, and EQ behavior were not changed by this release.

# v1.0.0-beta.44 - Player Seek Shortcuts and Batch Download Recovery

## Added
- Added `Ctrl+0` to reset playback speed to the user's configured default speed and pitch to `1.0x`.
- Added `Ctrl+Home` and `Ctrl+End` for quick jumps to the start and end of the current video or local file.
- Added the new reset and jump actions to Settings > Keyboard shortcuts.

## Fixes
- Batch downloads now continue after one video fails instead of stopping the whole queue.
- Playlist and channel downloads now skip unavailable entries/fragments and keep the collection download moving.
- YouTube download recovery now tries the `web_safari` client after recoverable format or unavailable-video failures, then cookies only if needed and configured.
- Playback stream resolution has the same conservative YouTube client recovery after normal extraction and format fallback fail.

## Verification
- Confirmed the successful download path still makes only one yt-dlp call.
- Confirmed fallback recovery runs only after recoverable download errors.
- Confirmed batch download processing continues after a failed item and reports the failure count.
- Reviewed mpv cache and seek settings; no cache behavior was changed.

# v1.0.0-beta.43 - Reliable Truncated Stream Recovery

## Fixes
- **Fixed the Beta 42 `Requested format is not available` player error.** The recovery path no longer asks yt-dlp to satisfy a speculative HLS format expression.
- **Recovery now inspects the formats YouTube actually provides.** For affected videos, ApricotPlayer uses the `web_safari` client to expose valid combined HLS streams and selects a 360p stream with the original audio language.
- **Normal YouTube playback remains unchanged.** The extra extraction runs only after the selected progressive MP4 is demonstrably truncated.

## Verification
- Reproduced with YouTube video `ezFU3UrbcTI` using the same cookie fallback path that failed in Beta 42.
- The recovered `93-9` stream started from 0:00 and completed a direct seek to 10:00 with the bundled mpv.

# v1.0.0-beta.42 - Truncated YouTube Stream Recovery

## Fixes
- **Fixed some YouTube videos opening at their total duration and refusing to play.** ApricotPlayer now detects progressive MP4 URLs whose real content length is drastically smaller than yt-dlp's expected media size.
- **Affected videos fall back to a valid combined HLS stream.** The fallback preserves video and the original audio language, starts at 0:00, and remains seekable.
- **Normal YouTube videos keep the existing fast path.** No additional extraction or delay is added unless the selected stream is demonstrably truncated.
- **HLS stream cache expiry is now read from YouTube manifest paths.** Permanent URL caching no longer keeps an expired HLS fallback for a year.
- **Removed the unrelated Beta 41 resume/restart workaround.** The reported failure was caused by a malformed YouTube stream, not a saved playback position.

## Verification
- Reproduced with YouTube video `ezFU3UrbcTI`: yt-dlp selected a 31:40 MP4 whose actual size was only 974,656 bytes instead of the expected approximately 54.7 MB.
- The replacement HLS stream started from 0:00 and completed an exact seek to 10:00 through the bundled mpv.

# v1.0.0-beta.41 - End-of-Video Resume Recovery

## Fixes
- **Videos with a stale saved position at 100% now start from the beginning.** Resume positions are checked against the resolved media duration before mpv receives its start position.
- **Play at the end now verifies that mpv actually restarted.** ApricotPlayer no longer announces a successful restart after merely sending a seek command.
- **Failed end-of-file seeks recover with a fresh player load.** The stale saved position is removed and the same item is reopened from zero without closing the player session.
- **Immediate EOF after a resume is recovered automatically.** This covers media whose duration was unavailable until mpv started.

## Unchanged
- Valid resume positions in the middle of media continue to work.
- Cookie selection, stream caching, EQ, audio processing, downloads, and normal player startup were not changed.

# v1.0.0-beta.40 - Cookie Playback Fallback

## Fixes
- **Unrestricted YouTube videos no longer enter the cookie fallback after a successful format fallback.** If the fast preferred format is unavailable but yt-dlp successfully returns other playable formats without cookies, ApricotPlayer now uses that result directly.
- **Cookie loading is deferred until an authentication, age restriction, or JavaScript extraction failure actually requires it.** Importing a cookies file no longer changes the normal playback path for unrestricted videos.

## Unchanged
- Age-restricted and sign-in-protected videos still retry with valid YouTube login cookies.
- Player caching, seeking, audio processing, and download behavior were not changed.

# v1.0.0-beta.39 - Reliable Cookie File Refresh

## Fixes
- **Updated external cookies files are now detected by their contents.** ApricotPlayer uses a SHA-256 signature instead of only the file timestamp and size, so replacing or editing the same `cookies.txt` file is picked up automatically without selecting it again.
- **Cookie source refresh failures are now included in diagnostic reports** instead of silently falling back to an older cached copy with no explanation.
- **Removed the ineffective Chrome DevTools fallback for the default Chrome profile.** Chrome 136 and newer block remote debugging of the default data directory, so ApricotPlayer now avoids the long failed retry and gives accurate guidance to use an exported Netscape `cookies.txt`, Firefox, or Brave.

## Unchanged
- Playback, media cache, and long-video scrubbing behavior were not modified.

# v1.0.0-beta.38 - Settings Reset and Related Autoplay

## Fixes
- **Fixed every settings reset action.** The modular settings screen was missing its `Settings` model import, so Restore to defaults and each per-section reset raised a hidden event-handler error and appeared to do nothing.
- **Removed the duplicate Reset all settings button from General.** Restore to defaults in the main settings button row is the single global reset action; per-section reset buttons remain available in each section.
- **Related autoplay no longer repeatedly chooses videos already played in the current player session.** ApricotPlayer now tracks YouTube video IDs until the player is closed and selects the first unseen related suggestion.
- **Expanded cookie diagnostics.** Diagnostic reports now show the configured cookie cache, source file, source signature, browser, and browser profile so remaining machine-specific refresh problems can be identified without changing working cookie behavior.

## Verification
- The external cookies source refresh was tested after changing the source file and after a simulated application restart; it refreshed correctly without selecting the file again.
- Long-video mpv cache and seek arguments are unchanged from the existing fixed implementation because no code regression was found.

# v1.0.0-beta.37 - Player Start Hotfix

## Fixes
- **Fixed online media failing to start with `MainFrame object has no attribute is_requested_format_error`.** The shared format-error detector used by stream resolution is restored, including playback started from History.
- **Kept the Beta 36 download-speed improvement intact.** Authenticated downloads still skip the redundant cookie-only request.

# v1.0.0-beta.36 - Faster VPN Download Start

## Fixes
- **Removed a redundant authenticated yt-dlp attempt from VPN-restricted downloads.** After YouTube explicitly requests authentication, ApricotPlayer now retries with configured cookies and its bundled JavaScript solver in one step instead of first making a cookie-only request that cannot expose the requested formats.
- **Normal download speed and quality remain unchanged.** Successful anonymous downloads still use one request with no cookies or JavaScript solver. Format selection, yt-dlp, FFmpeg, fragment settings, audio conversion, and the 250 ms worker startup timing are unchanged.

# v1.0.0-beta.35 - VPN Download Format Recovery

## Fixes
- **YouTube downloads now recover when a VPN triggers the authenticated JavaScript challenge path.** ApricotPlayer still tries anonymously first and then retries with configured cookies only after YouTube requests authentication. If that cookie attempt reports that the requested format is unavailable because challenge solving hid the media formats, Apricot retries that failed attempt with its bundled Node and yt-dlp EJS solver.
- **Normal downloads remain unchanged.** The JavaScript solver adds no request or delay to successful anonymous downloads or successful cookie downloads, and no fragment, FFmpeg, format-quality, or startup timing settings were changed.

# v1.0.0-beta.34 - Conservative Cookie Download Fallback

## Fixes
- **Restored the established cookie fallback order for YouTube downloads.** ApricotPlayer first attempts the download without cookies, exactly as in v0.9.20 and earlier beta builds. The configured cookie file is used only when YouTube returns an authentication or sign-in error.
- **Kept the safe Beta 33 improvements.** Single downloads still begin after 250 ms, the original Documents cookie path remains persistent and automatically refreshed, and cookie-dependent stream cache entries are invalidated when the source changes.

# v1.0.0-beta.33 - Faster Authenticated Downloads and Cookie Source Recovery

## Fixes
- **YouTube downloads with valid login cookies now use them on the first attempt.** ApricotPlayer no longer performs a full anonymous extraction/download attempt before retrying the same URL with the configured cookies file. This avoids duplicated network work on connections where YouTube requires sign-in.
- **Single audio and video downloads begin 650 ms sooner.** The worker startup delay was reduced from 900 ms to 250 ms while retaining the screen-reader download-start announcement.
- **Legacy cookie selections are reconnected to the original Documents file.** Users whose saved setting only points to ApricotPlayer's internal AppData copy are migrated automatically when a valid `cookies.txt` is found in the Windows Documents folder, including OneDrive Documents.
- **Edited cookie files now invalidate cookie-dependent stream cache entries.** Replacing or editing the selected source is reflected immediately instead of leaving playback tied to a stream URL resolved with older cookies.
- **The cookie picker now announces the selected source path.** It no longer reports ApricotPlayer's normalized AppData copy as though that were the file selected by the user.

## Verified
- The complete download methods and packaged yt-dlp, FFmpeg, mpv, and Python runtime were compared against the pre-refactor v0.9.20/v0.9.22 build. The format selectors, post-processing settings, downloader options, and bundled media tools are unchanged.
- Cookie source migration, automatic source refresh, stream-cache invalidation, and first-pass cookie use were exercised with focused local tests.

# v1.0.0-beta.32 - Download Performance and Background Progress

## Fixes
- **Restored audio download behaviour from v0.9.57.** Beta 26 applied video-specific DASH fragment, HTTP range chunk, and socket buffer tuning to audio-only downloads. Cross-testing showed that this made some connections dramatically slower and left downloads at 100% processing for much longer. Audio downloads once again use yt-dlp's proven audio defaults while retaining the same format conversion and metadata behaviour.
- **Playlist, channel, and large queued downloads now use a separate non-blocking progress window.** The window can be hidden while ApricotPlayer and the player remain fully usable. Closing or hiding it no longer affects the main application, and completing a playlist closes only the progress window.
- **Multiple long-running downloads now share the progress window safely.** Finishing one task switches the window to another active task instead of destroying its progress UI.
- **Updated cookie files are detected automatically.** ApricotPlayer now remembers the original selected cookie file separately from its normalized internal copy. When the same source file is edited or replaced, the internal yt-dlp cookie file is refreshed automatically without requiring the user to select it again.
- **Cookie login detection is invalidated after every successful refresh.** Playback and downloads immediately see newly imported YouTube login cookies instead of reusing an old in-memory result.

## Verified
- The video download format, fragment, chunk, buffer, FFmpeg, and post-processing path remains unchanged from v0.9.57.
- The installer `_internal` replacement fix remains intact.

# v1.0.0-beta.31 - Per-Item Resume Position Fix

## Fixes
- **Resume positions are now saved against the media item that mpv is actually playing.** If a file/video was still playing in the background and another local file was opened, Apricot could replace the UI's current item before stopping the old mpv process. The old file's timestamp was then saved under the new file's key, so the new file incorrectly resumed at the old file's position. Apricot now keeps a separate active-playback item snapshot for position saving, so resume timestamps remain independent for local files, YouTube items, playlists, folders, and podcasts.

# v1.0.0-beta.30 - Autoplay, Resume Position and Podcast Fixes

## Investigation
- **Video download speed: no code regression found.** Following tester reports that video downloads felt slower than before the modular refactoring, the entire download pipeline was compared line-by-line against the pre-refactor monolith (v0.9.18). The download options, format selectors, yt-dlp invocation, tuning constants (8 concurrent fragments, 10 MiB HTTP chunks, 1 MiB buffer), ffmpeg resolution, threading model, and progress hook are all byte-identical between the old and new code. The bundled yt-dlp is current (2026.03.17) and ffmpeg is bundled correctly. Any perceived slowdown is therefore from YouTube server-side throttling, network conditions, or a per-machine `concurrent_fragments` setting — not from a code change. No speed-affecting code was altered on the video path.

## Fixes
- **"Autoplay related suggested videos" now requires "Automatically play next item" to be on, matching pre-refactor behaviour.** After the refactor, enabling only "Autoplay related" caused a related YouTube video to auto-start at the end of every video even when "Automatically play next item" was turned off — so playback never stopped. Autoplay-related is once again treated as a refinement of autoplay-next (it chooses *what* plays next), so with autoplay-next off, playback stops at the end of the current item.
- **Resume position no longer gets stuck near the end of long videos and podcasts.** When saving the playback position, the code read the human-formatted duration string (e.g. "3:45") instead of the numeric `duration_seconds`, which always failed and forced a 50 ms mpv query. When that query timed out, a position a few seconds from the end was *saved* instead of cleared, so the next session resumed near the very end. The numeric duration is now read directly, and the fallback query timeout was widened, so finished items correctly start fresh next time.
- **Podcast episodes are no longer marked "played" on every repeat loop.** When repeating a single episode, the "mark played" step ran on each loop instead of only when playback actually moves on. It now runs after the repeat check, so a looped episode keeps its unplayed state until you stop or move to another item.

## Changed
- **Audio downloads now also set `progress_delta`, matching the video download path.** This is a minor alignment that slightly reduces yt-dlp's internal progress-callback overhead during audio downloads.

# v1.0.0-beta.29 - Step-by-Step Results Back Navigation

## Fixes
- **Escape now steps back correctly after opening channel or playlist results from the player.** Beta 28 returned from the player to the channel/playlist results, but the next Escape could still jump to the main menu because the original result list was not saved when the collection was opened from the player. Apricot now preserves the player's previous search/trending result snapshot before loading that collection, so the back chain works one screen at a time.

# v1.0.0-beta.28 - Channel and Playlist Return Fix

## Fixes
- **Channel and playlist result lists opened from the player now use the proper results screen.** Opening a video's channel or a playlist while the player was active could load the collection without first restoring the normal search/results UI. After playing from that collection, Escape could therefore fall back to the main menu instead of returning one screen back to the channel or playlist results. Collection tabs now prepare the results screen before loading, so closing the player returns to the expected list.

# v1.0.0-beta.27 - Session Sequence Restore and Release Channel Polish

## New
- **Resume last playback session now restores the active player sequence.** When a session is saved from a folder, playlist, playback queue, or other explicit player sequence, Apricot stores a slim sequence snapshot. Resuming the session restores that sequence so Next/Previous keep working in the same context instead of falling back to empty or stale results.

## Changed
- **Update channel defaults now match the build type.** Pre-release builds still default and migrate to the beta channel, while future stable builds default to the stable channel for fresh installs. This keeps beta testing smooth without accidentally putting stable users onto beta releases.

## Notes
- **Installer `_internal` update fix preserved.** The Inno Setup cleanup that removes the old `{app}\_internal` folder before installing remains untouched, so bundled DLL/module updates are not left half-old during in-app updates.

# v1.0.0-beta.26 - Settings Returns to Player; Audio Downloads Match Video Speed

## Fixes
- **Escape from Settings (and the Back button) now returns to the player when Settings was opened from the player via shortcut.** Previously, opening Settings from the player screen via shortcut and then pressing Escape or clicking Back dumped the user on the main menu, losing the player screen, the embedded result list, and the next/previous navigation. The shortcut now remembers it was invoked from the player, and `back_from_settings` rebuilds the player page (including the result list when background playback is on). Opening Settings from the main menu still returns to the main menu.
- **Audio downloads now match the throughput tuning used for video downloads.** Audio downloads were left on yt-dlp's defaults (4 concurrent DASH fragments, no HTTP chunk size, no socket buffer size) while video downloads use 8 concurrent fragments, 10 MiB HTTP range chunks, and a 1 MiB buffer. Audio downloads now use the same tuning and finish several times faster on high-bandwidth connections.

# v1.0.0-beta.25 - Hide Background Player Controls in Settings Screen

## Fixes
- **Background player controls no longer appear in the Settings screen.** When background playback was active and the user opened Settings, the player panel and play/pause/skip buttons were inserted at the top of the screen. Tab and Shift+Tab navigation would cycle through the player buttons instead of the settings sections and controls. The controls are now suppressed via a `settings_screen_active` flag that blocks `add_background_player_section()` — including the deferred `wx.CallAfter` path — for the entire duration of the Settings screen. All keyboard shortcuts for controlling playback still work.

# v1.0.0-beta.24 - Fix Seek Stalling After Playing Multiple Videos

## Fixes
- **Fixed seek stalling and requiring a full network reload after playing several videos.** mpv is killed (not gracefully exited) when the user stops or changes a video, so it cannot clean up its own demuxer cache files. These stale files accumulate in the cache folder across sessions — after 5–6 videos the folder can hold several GB of leftover data. When the disk fills up, mpv silently falls back to no disk cache, and every backward or forward seek requires a fresh network round-trip instead of reading from the local buffer. The cache folder is now wiped before each new mpv session starts, ensuring mpv always has space to write its cache and backward seeks within the cached window are instant.

# v1.0.0-beta.23 - Fix Player Silently Stopping on Network Drop or URL Expiry

## Fixes
- **Fixed player silently stopping without any notification when mpv exits unexpectedly.** Previously, if mpv crashed or was killed by the OS (e.g. due to a YouTube CDN URL expiring mid-playback, a network drop, or an out-of-memory event), the player monitor thread simply exited without doing anything — the UI remained frozen in "playing" state with no audio and no error message. Now the monitor detects when mpv exits for any reason other than a normal end-of-file and triggers the standard EOF handler, which correctly updates the UI, announces the end of playback, and advances to the next item if autoplay is enabled.

# v1.0.0-beta.22 - Tab Navigation and Play/Pause Announcement Fixes

## Fixes
- **Fixed Tab key not working from background player buttons and most other controls.** `on_char_hook` was consuming Tab events without calling `event.Skip()` when no custom navigation handler claimed them. This meant Tab from the play/pause, next, previous and other background player buttons produced no focus movement at all — only the first button (reachable from screen content) and the second button (via a dedicated custom handler) were Tab-accessible. All other controls in the window (search, results, settings sections) appeared unreachable. Now Tab is explicitly passed through to native wx focus traversal when no custom handler handles it.
- **Fixed Shift-Tab not working from background player buttons.** Same root cause as above — Shift-Tab from player buttons other than the first one was silently consumed. Native wx now handles Shift-Tab for those buttons correctly.
- **Fixed play/pause state being announced twice by screen readers.** When the play/pause button had keyboard focus, pressing it caused the screen reader to read both the new button label ("Play") from the automatic accessibility name-change event, and the explicit programmatic announcement ("Paused.") — resulting in "Paused. Play" or similar double reads. The explicit announcement is now suppressed when a play/pause button has focus, since the label change already provides the feedback.

# v1.0.0-beta.21 - System Tray Icon Fix

## Fixes
- **Fixed system tray icon never appearing.** The `ApricotTaskBarIcon` class was lost during the modular refactoring, so the tray icon setup silently failed with a `NameError` that was swallowed by an `except Exception: pass` guard. The class has been restored: it handles left-click and double-click to restore the window, provides a right-click context menu with Show, Settings, Check Subscriptions, and Exit options, and uses the standard information icon with the app name as the tooltip.

# v1.0.0-beta.20 - CapsLock Shortcut False-Positive Fix

## Fixes
- **Fixed CapsLock (and other special keys) incorrectly triggering letter-bound shortcuts.** `key_event_matches_letter` always included the Ctrl-character code for a letter in its match set — for 'T' that code is 20, which is identical to the Windows virtual key code for CapsLock (VK_CAPITAL = 0x14). `GetRawKeyCode()` returns the virtual key code on every key press, so pressing CapsLock produced a code-20 event that matched the 'T' shortcut, causing `player_time` (show duration) to fire on CapsLock. The same root cause affected other keys: Backspace (VK=8) matched Ctrl+H shortcuts, Tab (VK=9) matched Ctrl+I shortcuts, etc. The Ctrl-character code is now only added to the match set when Ctrl is actually held, eliminating all such false positives.

# v1.0.0-beta.19 - Persistent Stream URL Cache Fix

## Fixes
- **Fixed stream URL cache effectively not persisting across restarts.** The cache was already saved to disk on clean shutdown, but the default TTL was 20 minutes — meaning any restart more than 20 minutes after the previous session resulted in an empty cache. The default is now 6 hours (360 minutes), which matches the typical lifespan of a YouTube stream URL. YouTube's own URL expiry (embedded in the URL's `expire` parameter) is still respected as an upper bound, so entries never stay valid longer than the stream URL itself allows.
- **Stream URL cache now survives crashes and force-quits.** Previously the cache was only written to disk on a clean shutdown. It is now also flushed to disk in a background thread each time a new entry is added, so a crash or task-kill no longer empties the cache.

# v1.0.0-beta.18 - Resume Session Improvements

## New
- **Added setting to control whether "Resume last playback session" appears in the main menu.** A new checkbox in the Playback settings section lets you hide the resume option from the main menu if you prefer a cleaner list.
- **"Resume last playback session" now appears below "Search YouTube" instead of at the top of the menu.** The option no longer pushes other items down when it appears — it sits naturally after the primary search action.
- **Resume last session now starts from where you left off.** When "Resume where you left off" is enabled in Playback settings, the resume menu action will seek to the last saved playback position instead of always starting from the beginning.

# v1.0.0-beta.17 - Podcast Speed Presets Beta

## New
- **Added per-podcast playback speed presets.** RSS/podcast feeds can now remember their own speed instead of always using the global player speed.
- **Added a feed context action.** Use the podcast/RSS feed context menu to choose a speed preset or return that feed to the global player speed.
- **Added a player shortcut and action.** While a podcast episode is playing, use `Ctrl+Shift+E`, the player button, the player context menu, or Action finder to save the current speed for that podcast.

## Notes
- **Preserves presets across feed refreshes.** Refreshing a feed keeps its saved podcast speed.
- **Kept playback scope narrow.** Non-podcast playback still starts from the existing global speed setting; the mpv startup speed only changes when an RSS/podcast episode has a feed preset. This beta does not change YouTube resolving, seek timing, EQ, bass boost, or volume boost.

# v1.0.0-beta.16 - Full Screen Shortcut and Podcast Resume Polish Beta

## New
- **Added a configurable Player full-screen shortcut.** `F11` now toggles full screen from the player, background player controls, and Action finder, and appears in Settings > Keyboard shortcuts.
- **Added shortcut labels to full-screen controls.** The player full-screen checkbox and the background-player full-screen button now show the configured shortcut when shortcut labels are enabled.
- **Added podcast resume progress labels.** Podcast/RSS episode lists now show `resume at ...` when Apricot has a saved resume position for an unfinished episode.
- **Added clear episode progress.** Use `Ctrl+Shift+R` or the episode context menu to clear a podcast/RSS episode's saved resume position.

## Notes
- **Kept playback hot paths scoped.** This beta does not change mpv startup arguments, YouTube resolving, seek timing, EQ, bass boost, volume boost, or normal Next/Previous sequence logic.

# v1.0.0-beta.15 - Podcast Played State Beta

## New
- **Added podcast episode played/unplayed state.** Feed item lists now announce a `played` marker, and feed lists show how many episodes are played.
- **Added a podcast played toggle.** Use `Ctrl+Shift+X` or the episode context menu to mark the selected podcast episode played or unplayed.
- **Auto-marks completed podcast episodes.** When an RSS/podcast episode reaches EOF, Apricot stores it as played.
- **Preserves played state across feed refreshes.** Refreshing a podcast/RSS feed keeps existing played markers by episode URL, GUID, webpage URL, or title.

## Notes
- **Kept player hot paths scoped.** This beta only adds RSS/podcast metadata state and does not change mpv startup, YouTube resolving, seek controls, EQ, bass boost, volume boost, or normal Next/Previous logic.

# v1.0.0-beta.14 - Session Restore Beta

## New
- **Added Resume last playback session.** Apricot now stores a small snapshot of the last played item and shows a main-menu and Action finder entry to reopen it later.
- **Reuses existing resume-position behavior.** If Resume where you left off is enabled, the restored item starts from the saved playback position; live streams still start live.
- **Added last-session diagnostics.** Diagnostic reports now include the title of the saved player session to make tester reports easier to line up.

## Notes
- **Kept playback hot paths untouched.** This beta does not change mpv startup arguments, stream resolving, seek controls, EQ, bass boost, volume boost, or Next/Previous sequence logic.

# v1.0.0-beta.13 - Per-Device Equalizer Presets Beta

## New
- **Added per-output-device equalizer defaults.** When global EQ is enabled, Apricot can now select a different EQ preset for each output device, including `auto`.
- **Added Equalizer Settings support for device presets.** The Equalizer section now lets the user choose the preset for the current default output device.
- **Added Player Equalizer actions for device presets.** The player EQ dialog can save the current preset as the default for the current output device or clear that device default.
- **Improved diagnostics for EQ testing.** Diagnostic reports now include the effective EQ preset and the number of saved output-device EQ mappings.

## Notes
- **Kept EQ audio processing unchanged.** This beta only chooses which preset is active for a device; it does not change slider math, EQ filters, bass boost, clipping protection, mpv startup, or stream resolving.

# v1.0.0-beta.12 - Shortcut Discoverability Beta

## New
- **Added optional shortcut labels throughout the main menu and player.** Menu items and player buttons now show the user's configured shortcuts by default, while Settings includes a new checkbox to hide those labels.
- **Added a Play file shortcut.** `Ctrl+Alt+I` now opens Play file, appears in the shortcut editor, and is displayed beside the Play file menu item.

## Changed
- **Removed Action finder from the main menu.** It remains globally available with `Ctrl+Shift+J`.
- **Kept YouTube playback hot paths untouched.** This beta does not change stream resolving, mpv startup, volume, bass boost, or equalizer processing.

# v1.0.0-beta.11 - Comments 2.0 Beta

## New
- **Expanded the YouTube Comments screen.** Comments now have a search box, a sort selector, Copy comment, Copy visible comments, and Open author channel actions.
- **Added local comment sorting.** Loaded comments can be shown in original order, newest first, oldest first, most liked, or most replies without reloading playback.
- **Made comments more useful without an API key.** yt-dlp-loaded comments now keep timestamps and author-channel metadata when available, so sorting and author actions work there too.
- **Included replies in search and copied text.** Replies already shown in comment details are now searchable and included when copying a comment.

# v1.0.0-beta.10 - Fast Startup Restoration Hotfix

## Fixes
- **Removed the beta 7 startup pause/mute gate.** Apricot now lets mpv start immediately again while still passing the intended volume, volume limit, speed, ReplayGain, output device, and initial EQ filter directly on the mpv command line.
- **Kept Start paused behavior correct.** The only time Apricot now starts mpv paused is when the user's Start paused setting is enabled.
- **Kept the beta 9 old-player handoff protection.** Track changes still silence the previous mpv process with a no-wait best-effort write, but new playback is no longer delayed by waiting for IPC before unpausing.

# v1.0.0-beta.9 - No-Lag Audio Handoff Hotfix

## Fixes
- **Kept the beta 8 loud-burst protection but made it non-blocking.** Apricot now sends the stop-time mute/volume-zero/pause handoff to mpv in one best-effort IPC write instead of three separate commands, so the protection should not add noticeable delay when changing tracks or opening the next player item.

# v1.0.0-beta.8 - Volume Boost Handoff Hotfix

## Fixes
- **Silenced the old mpv instance before replacing playback.** When Apricot switches to the next file/video, the previous player is now muted, set to volume 0, and paused before it is terminated. This targets the tester report where audio briefly sounded like it jumped to boosted volume before settling back to the intended volume.
- **Preserved session volume before silencing the old player.** Internal handoffs still remember the user's current volume first, so the stop routine cannot accidentally learn the temporary muted handoff volume.
- **Invalidated old player workers earlier during stop.** Startup/volume workers from the previous playback generation are now stopped before Apricot saves position or terminates mpv, reducing the chance of a stale worker unmuting a player that is already being replaced.

# v1.0.0-beta.7 - Startup Audio Gate Hotfix

## Fixes
- **Fixed the remaining startup volume burst on instant-starting local files.** mpv now starts briefly muted and paused while Apricot applies the target volume, volume limit, and startup audio state, then playback is released immediately. This prevents the first 0.0-second audio frame from playing before volume/EQ state is ready.
- **Kept Start paused behavior intact.** If the user has Start paused enabled, Apricot prepares the same safe startup audio state but leaves playback paused.

# v1.0.0-beta.6 - Action Finder and EQ Startup Smoothing Beta

## New
- **Added Action finder.** Use `Ctrl+Shift+J` or the main menu item to search ApricotPlayer actions by name and run them directly. It includes global navigation actions and player actions when playback is active.
- **Added the Action finder shortcut to Settings > Keyboard shortcuts.** The default shortcut avoids `Ctrl+Shift+P`, which is already used by another command.

## Fixes
- **Reduced the startup loudness jump with EQ/clipping protection.** Apricot now passes the active EQ/clipping-protection filter directly to mpv at playback start and then verifies it shortly after startup, instead of waiting before applying it. This targets the “volume gets loud, then settles” report from local files with boosted EQ profiles.

# v1.0.0-beta.5 - Related Playback and EQ Profiles Beta

## New
- **Added a dedicated related-video next command.** `Ctrl+Shift+PageDown` now plays a related/suggested video from the current YouTube item, while `Ctrl+PageDown` keeps moving through the normal results, folder, playlist, or queue sequence.
- **Started Equalizer Profiles 2.0.** Player EQ profiles can now be imported/exported as JSON, and the player EQ dialog has an A/B compare button for quickly switching between the original EQ state and the edited profile preview.

## Fixes
- **Fixed Autoplay related suggested videos actually using related playback.** When that setting is enabled, ending a YouTube video can now continue with a related suggestion even when normal autoplay-next is disabled.
- **Removed the accidental F1 marked-preview shortcut.** Marked clip preview remains on the configured player preview shortcut, which defaults to `P`.

# v1.0.0-beta.4 - Audio Normalization Beta

## New
- **Added a player audio-normalization control.** The player now exposes ReplayGain / loudness normalization directly as a button, context-menu action, and configurable shortcut.
- **Added `Ctrl+Shift+G` for audio normalization.** The shortcut cycles Off, Track, and Album modes and announces the selected mode for screen reader users.

## Fixes
- **Made transcript loading less likely to hit YouTube caption rate limits.** Transcript downloads now reuse yt-dlp/YouTube request headers and fall back to yt-dlp's subtitle downloader when the direct caption request fails.
- **Replaced raw transcript HTTP 429 errors with a screen-reader friendly rate-limit message.**
- **Stopped plain F5 from announcing the player duration.** Time/duration announcement remains on the configured Player: announce time shortcut, which defaults to `T`.

# v1.0.0-beta.3 - Bookmarks Beta

## New
- **Added persistent bookmarks and named markers.** Bookmarks are stored in `bookmarks.json` and can point to local files or online media.
- **Added bookmark controls to the player.** Use Add bookmark to name the current timestamp, or Bookmarks to browse saved positions for the current item.
- **Added global bookmarks access.** The main menu now includes Bookmarks, and `Ctrl+Alt+K` opens the bookmark list.
- **Added player bookmark shortcuts.** `Ctrl+Shift+B` adds a bookmark at the current player position, and `Ctrl+Shift+K` opens bookmarks for the current player item.
- **Added bookmark management.** Bookmarks can be played, renamed, deleted, and copied as YouTube timestamp links when possible.

# v1.0.0-beta.2 - Transcript Browser Beta

## New
- **Added a transcript/captions browser in the player.** Open it with the new Transcript button, the player context menu, or `Ctrl+Shift+T`.
- **Added transcript search and timestamp jumping.** Press Enter on a transcript line to jump playback to that caption timestamp.
- **Added transcript copy actions.** Users can copy the selected transcript line, the full transcript, or a YouTube timestamp link for the selected line.
- **Added local transcript sidecar support.** Local media can use nearby `.vtt` or `.srt` caption files.
- **Added the missing Copy lyrics button.** The existing lyrics dialog now exposes the copy action directly.

# v1.0.0-beta.1 - Diagnostic Report Beta

## New
- **Added Copy diagnostic report.** Testers can now copy a support report from the main menu or with `Ctrl+Alt+Shift+D`. The report includes app/update-channel info, yt-dlp/mpv/FFmpeg status, active player state, audio/session state, current media metadata, queue/result counts, and redacted mpv/updater log tails.
- **Added the diagnostic shortcut to Settings > Keyboard shortcuts.** The default shortcut avoids the existing download and stream-copy shortcuts.
- **Saved the 1.0 roadmap in the repo.** `docs/ROADMAP_1.0.md` keeps the agreed beta feature list visible so the next beta can continue from the same plan.
- **Started the 1.0 beta line on the beta branch.** This build is intended for beta-channel updater testing and is published as a GitHub prerelease.

# v0.9.57 - Player Session Audio Hotfix

## Fixes
- **Fixed session volume persistence when changing videos.** If the user changes volume in the player and then plays another item without closing the player, Apricot keeps that session volume across search results, channel videos, playlists, queue playback, and background playback.
- **Made real player close reset all session-only audio state.** Closing the player with Escape or Close player now resets session volume, output device, bass boost, volume boost, repeat, autoplay-next session state, shuffle, and EQ session values back to defaults for the next player session.
- **Clarified the session boundary in code.** Internal handoffs that stop mpv only to start the next item now explicitly keep the player session open, so the short process restart between videos no longer looks like a fresh player session.

# v0.9.56 - Player Tab Responsiveness Hotfix

## Fixes
- **Reduced Tab and Shift+Tab lag in the player.** Apricot now uses native wx tab navigation for normal player buttons, checkboxes, and embedded result lists again, while keeping the custom fallback only for the embedded mpv player panel that needs it.
- **Kept the 0.9.55 background-player tab order fix.** Search, channel, and playlist screens still keep their normal control order while playback continues in the background.

# v0.9.55 - Playback Startup and Background UI Hotfix

## Fixes
- **Removed the YouTube audio-only playback fallback.** YouTube playback now sticks to progressive audio+video streams for the fast-start/fast-seek path, and the stream URL cache profile was bumped so old audio-only cached URLs are not reused.
- **Kept non-YouTube audio sources working.** SoundCloud and other audio-only providers still use an audio-compatible stream format, but YouTube videos no longer fall back to m4a/bestaudio.
- **Fixed background-player controls disrupting search/channel/playlist tab order.** When a video keeps playing in the background and the user opens search, a channel, or a playlist, Apricot now appends the background player section without forcing it into the middle of the active screen's controls.

# v0.9.54 - Seek Hold and Focus Fixes

## Fixes
- **Restored hold-to-seek scrubbing in the player.** Holding the seek forward/backward shortcuts now keeps moving through playback continuously, instead of requiring repeated key presses. The hold path uses short non-blocking mpv seek sends so key repeat does not stall the UI.
- **Made player Tab and Shift+Tab navigation deterministic again.** The player page now routes focus through the same ordered controls even when focus starts on embedded results, navigation buttons, the player panel, or player action buttons, reducing cases where focus could wander into unexpected controls.
- **Fixed playlist context menus using stale result selection.** The application/context menu now builds playlist/channel actions from the actually focused result row, so Play playlist and Shuffle playlist should not disappear after returning from the player or reopening search results.

# v0.9.53 - Restore Fast YouTube Seeking

## Fixes
- **Restored the old fast-seek playback stream selection.** The 0.9.45 stream selector tried audio-only m4a first to lower RAM, but that changed the behaviour users relied on: immediately after starting a YouTube video, repeated large seek commands could feel stuck or sequentially buffered. Playback now prefers a small progressive MP4 with audio+video first, matching the pre-refactor behaviour where mpv receives one range-requestable file and can jump far ahead right away. Audio-only remains as a fallback for videos without a progressive MP4.
- **Invalidated old audio-first stream URL cache entries.** The stream URL cache key now includes a playback format profile, so existing cached m4a URLs from 0.9.45-0.9.52 are not reused after this update.

# v0.9.52 - Faster Player Transitions

## Fixes
- **Reduced the small delay when switching from results into the player or back through active playback paths.** Recording the previous item in history now updates memory immediately and writes the history file in the background, so pressing Enter on a result no longer waits for a JSON disk write before the player page appears.
- **Stopped slow mpv IPC calls from blocking UI transitions.** Saving resume position now uses a short IPC timeout and prefers already-known duration metadata before asking mpv, which keeps switching tracks responsive even if mpv is momentarily slow to answer.
- **Shortened mpv shutdown waits when replacing playback.** Starting a new item no longer allows the old mpv process to hold the UI for up to two seconds during termination.
- **Avoided treating web URLs as possible local paths.** YouTube and other streamed URLs now skip unnecessary filesystem checks in player UI paths.
- **Kept history safe on exit.** Pending background history writes are superseded by one final synchronous save during shutdown, so the faster hot path does not lose recent playback history.

# v0.9.51 - Dynamic Results Cookie Retry Fix

## Fixes
- **Fixed dynamic result loading still bypassing cookie retry.** The v0.9.50 search fix restored cookie retry and the YouTube auth hint for the first page of search results, but the follow-up paths used by dynamic "load the next 20 results" and channel/playlist collection loading still explicitly disabled that retry and suppressed the helpful error text. Those paths now use the same yt-dlp retry behaviour as normal search, so navigating beyond the first page should no longer produce avoidable "sign in / cookies" failures.

# v0.9.50 - Restore True Pre-Refactor Keyboard Routing and Search Behaviour

## Fixes
- **Fixed all player shortcuts being blocked when a dropdown (Choice/ComboBox) held focus.** A post-modularisation guard unconditionally called `event.Skip()` and returned whenever focus was on any `wx.Choice` or `wx.ComboBox` widget — before `handle_player_shortcut_event` was consulted. This meant that pressing `Space` (play/pause), `n`/`p` (next/previous), seek keys, or any other player shortcut while the search-type or provider dropdown was focused silently did nothing. The guard is removed; the original code had no such block.
- **Fixed player shortcuts losing priority over results-list native navigation.** `results_list_owns_key` (Up/Down/Home/End/PageUp/PageDown in the results list) was evaluated before `handle_player_shortcut_event` was called. If a user had PageUp/PageDown bound to a player action (seek, speed, etc.) and the results list held focus, the native list scroll always won. Player shortcut dispatch now runs first; the results-list native handler is consulted only if no player shortcut matched — matching the original `wx_main.py` ordering.
- **Fixed player-checkbox and edit-mode handlers incorrectly firing when focus is in the results list.** `handle_player_shortcut_event` had no early exit for the results-focus case; after handling `player_previous`/`player_next`/results-owned keys it fell through into the context-menu, checkbox-toggle, edit-mode, and details-navigation branches. A key that matched none of those player-specific actions (e.g. a bare letter) could therefore accidentally trigger an unrelated player handler. Now returns `False` after the results block, letting `on_char_hook`'s dedicated results-shortcut branch handle the event.
- **Fixed `n`/`p` (next/previous track) global shortcuts not working when focus is on any button, checkbox, slider, or list outside the player screen.** `handle_active_player_global_shortcut_event` contained an `isinstance` guard that blocked the shortcut for `wx.ListBox`, `wx.Button`, `wx.CheckBox`, `wx.Choice`, `wx.ComboBox`, `wx.Slider`, and `wx.SpinCtrl` — covering almost every interactive widget. The original code only excluded the results list (which has its own dedicated path). The isinstance guard is removed; only `focus_in_results_control` remains.
- **Fixed Ctrl/Alt-modified shortcuts being swallowed by text fields.** The `focus_accepts_text` guard sent every non-Enter, non-Escape key through `event.Skip()` immediately, including `Ctrl+L` (copy link), `Ctrl+D` (download audio), `Ctrl+B` (add to favourite), etc. Shortcuts that work throughout the original `wx_main.py` — even while typing in the search field — were silently discarded. The guard now passes through any key that has Ctrl or Alt held, allowing global shortcuts to reach their handlers; plain alphanumeric and navigation keys continue to go to the text field natively.
- **Fixed YouTube search silently disabling cookie retry and auth hint.** `search_worker` called `ydl_extract_info` with `allow_cookie_retry=False` on every branch (YouTube video, playlist/channel, SoundCloud), preventing yt-dlp from automatically retrying with browser cookies when YouTube returns "Sign in to confirm you're not a bot". The error handler also passed `include_youtube_auth_hint=False` to `friendly_error`, suppressing the helpful sign-in suggestion in the error dialog. Both parameters are removed, restoring the original behaviour.

# v0.9.49 - Restore True Pre-Refactor Player Behaviour

## Fixes
- **Reverted three incorrect "restore" changes from v0.9.47 in `handle_player_eof`.** A prior analysis incorrectly claimed that the pre-refactoring code (`wx_main.py` v0.9.18) contained `shuffle_current` in the autoplay guard, an unconditional `else:`-branch queue-pop when autoplay is off, and a "playback finished" announcement at the terminal state. Direct comparison against the original monolith shows none of these existed — all three were new additions mistakenly labelled as restorations. The handler is now an exact mirror of the original: only `effective_autoplay_next()` gates track advancement; the queue is never popped when autoplay is off; and `handle_player_eof` is silent on exhaustion (the announcement lives only in `play_next_standard_fallback`, which is called by manual-next actions).
- **Fixed player keyboard shortcuts not firing when focus is on a slider or checkbox.** A block added post-modularisation caused `Space`, arrow keys, `Home`, `End`, `PageUp`, and `PageDown` to skip straight to the native widget handler when a `wx.CheckBox` or `wx.Slider` held focus — before `handle_player_shortcut_event` was ever consulted. This meant pressing `Space` on a player checkbox triggered the checkbox toggle instead of play/pause, and pressing arrow keys on the volume slider moved the slider instead of seeking. The block is now limited to `wx.SpinCtrl` only (where native increment is the correct behaviour). `wx.CheckBox` and `wx.Slider` now follow the original flow: player shortcut wins if bound, native widget handles if not.
- **Fixed results list "freezing" after a handler exception.** `on_results_key` wrapped its entire body in `try/except: pass`, which meant that if any shortcut handler raised an exception, the `else` branch's `event.Skip() + wx.CallAfter(self.maybe_extend_results)` was silently skipped — leaving the list unable to scroll further. The exception handler now explicitly calls `event.Skip()` and `wx.CallAfter(self.maybe_extend_results)` so the list remains responsive even when a handler fails.

# v0.9.48 - Settings and Cache Key Fixes

## Fixes
- **Fixed update channel defaulting to "stable" instead of "beta".** Both the settings render path and the save path used `"stable"` as the fallback when the stored `update_channel` value was empty or missing, overriding the model default of `"beta"`. Users who had never explicitly chosen a channel were silently placed on the stable channel. Both paths now fall back to `"beta"` to match the model default.
- **Fixed stream URL cache not varying by browser cookie source.** `stream_url_cache_key` used `getattr(self.settings, "cookies_browser", "")` — a typo: the model field is `cookies_from_browser`, not `cookies_browser`. The fallback always returned an empty string, so all cache keys looked identical regardless of which browser was configured for cookie extraction. Fixed to use the correct attribute name `cookies_from_browser` with fallback `"none"`.

# v0.9.47 - Player Regression Fixes (v0.8 parity)

## Fixes
- **Fixed session volume leaking across playback sessions.** `stop_player(reset_session=True)` did not reset `session_volume` to `None`, so the volume level from the previous session (including any boost or manual change) bled into the next fresh session. The original pre-refactoring code reset it; the refactored version accidentally dropped that line. Fixed.
- **Fixed shuffle mode not advancing to next track.** `handle_player_eof` checked only `effective_autoplay_next()` (settings.autoplay_next + session override), omitting `shuffle_current`. In the original code the condition was `shuffle_current OR autoplay_next`, so shuffle always advanced the track even when autoplay was off. Fixed: condition is now `effective_autoplay_next() OR shuffle_current`.
- **Fixed playback queue not playing when autoplay is off.** Manually queued items were only dequeued inside the `effective_autoplay_next()` block, so with autoplay disabled the queue was never consumed. In the original code the queue check was unconditional (ran before the autoplay guard). Fixed: when autoplay is off the queue is still checked and played.
- **Restored "Playback finished" announcement.** After a track ends with nothing next to play, the original code announced/set status "Playback finished." The refactored `handle_player_eof` dropped this line entirely. `play_next_standard_fallback` in misc.py had it but was only reached via the autoplay-related code path. Fixed: the announcement is restored in the terminal block of `handle_player_eof`.

# v0.9.46 - Playlist Navigation Fix and Context Menu Fixes

## Fixes
- **Fixed playlist navigation playing wrong video (search result instead of next playlist item).** When starting playlist playback `set_player_sequence` was called before `play_url`, which internally calls `stop_player(reset_session=True)` and wipes `player_sequence_results`. Next-track navigation therefore found an empty sequence and fell back to the search results list, playing a seemingly random video. Fix: `set_player_sequence` is now called after `play_url` so the sequence is established after the reset.
- **Fixed context menus showing wrong options or triggering wrong actions on repeated open.** All context menu handlers used `self.Bind(wx.EVT_MENU, handler, item)` (binding to the frame) instead of `menu.Bind(wx.EVT_MENU, handler, item)` (binding to the temporary menu object). wx recycles freed menu-item IDs; when the same ID was reused in a new menu, both the old and new handler fired together. This caused wrong actions, phantom options, and missing items (e.g. missing video-type combobox after closing a context menu with Escape). All context menus across `menus.py`, `library.py`, `search.py`, and `download.py` are now bound to their menu/submenu object so handlers are released when the menu is destroyed.
- **Fixed stale search results playing on empty query or fast navigation.** `show_search(restore_search=False)` did not clear `self.results` / `self.all_results`. If the user pressed Enter before focus moved to the query field, the old results list was still populated and `play_selected()` returned the first item from the previous search. Results are now cleared immediately when opening a fresh (non-restored) search screen.
- **Improved reconnection on flaky networks.** Added `reconnect_on_network_error=1` to the mpv `--stream-lavf-o` option string so that HTTP-level errors (5xx, dropped connections) trigger a reconnect attempt in addition to pure network-drop reconnects. This reduces stucking/buffering on unstable mobile connections.

# v0.9.45 - Stream Format Fix

## Fixes
- **Fixed broken/slow seeking and high RAM on YouTube streams.** The format selector `best[ext=mp4]/best` fell back to a DASH segment URL (a few seconds of audio) for content with no combined progressive MP4 (YouTube Music, very long videos, some regional content). mpv received the segment, played it, then stopped; seeking had no backward buffer. Combined 720p video+audio was also selected for audio-only playback, filling the demuxer cache with video data and forcing mpv to initialise a full video pipeline (~300 MB RAM). New selector: `bestaudio[ext=m4a][protocol=https]/bestaudio[protocol=https]/best[ext=mp4][protocol=https]/…/best`. `[protocol=https]` ensures a whole-file range-requestable URL (not a DASH segment). m4a audio is preferred: ~30× smaller, instant seeking, ~30–80 MB RAM instead of ~300+ MB.

# v0.9.44-beta.12 - JAWS Shortcut Fix and Crash Prevention

## Fixes
- **JAWS now announces F2, F3, V and other player shortcuts from the results list.** `player_shortcuts_allowed` was incorrectly returning `False` when focus was in the results list, preventing all player shortcuts from firing. A secondary bug — a stray `return True` in `handle_player_shortcut_event` — consumed non-navigation keys silently without executing their actions. Both are fixed.
- **Fixed crashes when pressing keys in the results list.** `on_results_key` had no exception guard; any unhandled error (e.g. during download or playback initiation) propagated to wxPython's main loop and terminated the app. The handler is now protected with `try/except`.

# v0.9.44-beta.11 - Persistent Stream URL Cache

## Improvements
- **Stream URL cache now persists across restarts.** Previously the URL resolver cache (which makes second plays of the same track instant) was in-memory only — closing and reopening the app wiped it, so there was no speed benefit on the first play after a restart. The cache is now saved to disk (`ApricotPlayer/stream_url_cache.json`) when the app exits and reloaded on startup, with expired entries filtered out automatically. The first play after a restart is now as fast as the second play within a session, as long as the cached resolution hasn't expired.

# v0.9.44-beta.10 - Seek Stall Fix

## Fixes
- **Fixed seek stall regression from beta.7.** Seeking forward or backward in any web stream (YouTube, SoundCloud, etc.) now responds instantly. The cause was `--stream-lavf-o=reconnect_streamed=1` which tells ffmpeg to fully reconnect the HTTP connection after every seek on a streaming URL, adding 2–5 s of stall per keypress. That flag is removed; `reconnect=1` and `reconnect_delay_max=5` are kept for resilience against real network drops on seekable streams.

# v0.9.44-beta.9 - JAWS Screen Reader Support

## New features
- **JAWS screen reader support.** All player announcements (track started, volume, seek, speed, repeat, shuffle, etc.) now reach JAWS via its COM automation server (`FreedomSci.JawsApi`). Implemented entirely with `ctypes` — no extra packages required. JAWS is tried only when NVDA has not already handled the text, so there is no duplicate speech when both screen readers run simultaneously. On machines where JAWS is not installed the ProgID lookup is performed once and cached; all subsequent calls return immediately with near-zero overhead. `EVENT_SYSTEM_ALERT` is suppressed when JAWS (or NVDA) has already spoken the text; `EVENT_OBJECT_NAMECHANGE` and `EVENT_OBJECT_VALUECHANGE` are always fired for Narrator and other IAccessible-based screen readers.

# v0.9.44-beta.8 - NVDA Audio Ducking Fix

## Fixes
- Fixed Alt+Tab to player muting the system's main audio output while the window has focus when NVDA is running. Every `announce_player` call was sending text to NVDA twice: once via `nvdaController_speakText` (the explicit controller call) and once via `wx.ACC_EVENT_SYSTEM_ALERT` (a WinEvent NVDA monitors independently). NVDA would interrupt itself and restart speech from the top, keeping Windows audio ducking active far longer than expected — appearing as permanent speaker muting to anyone with "When Windows detects communications activity: Mute all other sounds" configured. Fix: `raise_accessibility_alert` now suppresses `EVENT_SYSTEM_ALERT` when the NVDA controller already handled the text, eliminating the double-announcement. The `EVENT_OBJECT_NAMECHANGE` and `EVENT_OBJECT_VALUECHANGE` WinEvents are still always fired for JAWS, Narrator, and other screen readers.

# v0.9.44-beta.7 - Critical Playback Fix, Reconnect, Stream Stability

## Fixes
- Fixed critical playback regression from beta.6: `--demuxer-back-bytes` is not a valid mpv 0.41 option; mpv exited with "Fatal error" on every launch when disk cache was enabled (the default). Reverted to `--demuxer-max-back-bytes`.
- Fixed stream stuck after network drop. Added mpv reconnect options (`--stream-lavf-o=reconnect=1`, `reconnect_streamed=1`, `reconnect_delay_max=5`) so mpv automatically retries the HTTP connection after a drop.
- Fixed stream stalling after a few seconds (regression from beta.5). The `bestaudio/…` format selector returns DASH segment URLs in some cases; mpv plays one segment then stops. Reverted format selector to `best[ext=mp4]/best` and removed the `requested_formats` fallback.
- Fixed background player Tab navigation broken by wrong dispatch order in `on_char_hook`. The tab-navigation handlers now run before the results-list key check, matching the original code structure.

# v0.9.44-beta.6 - Seek Performance and Keyboard Hook Fix

## Fixes
- Fixed seek performance regression. `start_mpv` was passing `--demuxer-max-back-bytes` to mpv; the correct option name is `--demuxer-back-bytes`. Mpv silently ignored the unknown option, leaving the back-buffer at zero. Every backward seek had to re-download from the network. The correct option name restores on-disk cache-backed seeking.
- Fixed keyboard lag and double-activation on buttons. Two pre-checks added during the modular refactor — an early Tab-key skip and an `activate_focused_button_from_key` call — were not present in the original codebase. The Tab block bypassed the proper Tab-navigation handlers in non-player contexts. The button-activation call double-fired handlers on focused buttons (once from the hook, once from the native `EVT_BUTTON` binding) and discarded the native keypress, losing visual press feedback. Both are removed; `on_char_hook` now matches pre-refactor behaviour.

# v0.9.44-beta.5 - Audio Quality, Startup Speed, and RAM Fix

## Fixes
- Fixed audio quality regression introduced during the modular refactor. `resolve_stream_url` was using a hardcoded `"best[ext=mp4]/best"` format selector, which always picks a combined 720p MP4 stream with AAC 128 kbps audio. The selector now prefers `bestaudio[ext=m4a]/bestaudio` first so YouTube delivers M4A 256 kbps or Opus 160 kbps — meaningfully better stereo and dynamic range. Falls back to the user-configured video format and known-safe format IDs (18/22) if no audio-only stream is found.
- Fixed slow song startup. The same format bug caused mpv to download a full 720p video stream (2–4 Mbit/s) for audio-only playback, discarding all the video data. With an audio-only stream at ~160 kbps the initial buffer fills much faster.
- Fixed DASH multi-track stream URL extraction. When `bestvideo+bestaudio` is selected, `info["url"]` is absent; the code now walks `info["requested_formats"]` (preferring audio) before falling back to the format list.

## Performance
- Reduced RAM use from the stream URL cache. The raw yt-dlp info dict includes formats (100+ entries), automatic captions (30+ languages of timed data), thumbnails, heatmap, etc. — 10–50 MB per video in Python memory. Only the small metadata fields needed by the player UI are now stored; heavy bulk fields are stripped before caching. Cache footprint drops from several GB (after extended sessions) to a few MB.

# v0.9.44-beta.4 - Updater and Installer Fix

## Fixes
- Fixed the in-app updater silently reporting "already up to date" for all beta testers. The update channel defaulted to "stable", so beta releases were never offered. Default is now "beta". Existing users whose settings have "stable" stored are migrated to "beta" automatically on first launch of this build.
- Fixed the installer resetting to the default install directory on update. `UsePreviousAppDir=no` caused the installer to ignore the previous install path and default back to `C:\Program Files\ApricotPlayer`, leaving existing shortcuts pointing at the old installation. Changed to `UsePreviousAppDir=yes`.

# v0.9.44-beta.3 - Crash Fix, Volume Persistence, and Memory Reduction

## Fixes
- Fixed automatic crash during navigation in results and menus. Background-worker results are delivered to the UI via a timer-driven queue (`process_queue`). The original drain loop wrapped both the queue read and every handler inside a single `try/except queue.Empty` block, so an unhandled exception from any handler (e.g. a `RuntimeError` when wxPython tries to access a destroyed list control during rapid navigation) propagated straight to wxPython's main-loop exception handler, which terminates the process. Queue read and handler dispatch are now guarded by separate try/except blocks; exceptions inside handlers are caught and discarded so the app keeps running.
- Fixed volume resetting to default when closing the player screen with Escape. `stop_player()` was clearing `session_volume` whenever `reset_session=True` (the default). Because `back_to_results()` always calls `stop_player()` with that default, every Escape press wiped the manually set volume. The clear is removed; volume now persists for the entire app session and resets naturally when the app closes.

## Performance
- Reduced memory use during long browsing sessions. `metadata_hydration_urls` is a set used to avoid re-fetching yt-dlp metadata for the same result URL twice. It was never cleared, so extended sessions with many searches could accumulate thousands of URL strings. The set is now cleared automatically once it exceeds 1 000 entries, capping steady-state size to a few tens of kilobytes. A handful of previously-hydrated results may be re-fetched once per reset, which is negligible.

# v0.9.44-beta.2 - Keyboard Lag and Checkbox Fix

## Fixes
- Fixed checkboxes not responding correctly to Space. When a checkbox had focus, Space was falling through to the player shortcut layer (triggering play/pause or another bound action) instead of toggling the checkbox. Root cause: `on_char_hook` had no early-exit for native checkbox/slider/spinner keys before running the full shortcut matching loop. Fix: `wx.CheckBox`, `wx.Slider`, and `wx.SpinCtrl` now pass Space and arrow/navigation keys straight through to the widget. Enter is deliberately excluded so player-screen checkbox toggles (repeat, bass boost, etc.) still work.

## Performance
- Eliminated the primary source of keyboard lag. `shortcut_matches()` is called more than 30 times per keypress inside the central `on_char_hook` handler. Each call bottomed out in `shortcut_key_code()`, which rebuilt a 30-entry alias dictionary and compiled two regex patterns from scratch every time. All three objects are now module-level constants built once at import time. Per-keypress shortcut-matching overhead drops to near zero, making Enter, arrow keys, and Space feel immediately responsive again.
- Pre-compiled the `|`-separator split pattern used in `event_matches_shortcut`. It was also compiled fresh on every shortcut check.
- Reduced startup activation overhead. `activate_window_later()` was scheduling `foreground_window()` (win32 `AttachThreadInput` + several other calls) at four delays (0 ms, 75 ms, 250 ms, 750 ms). Default is now two delays (0 ms, 250 ms). Tray-restore and post-update relaunch keep their own explicit lists and are not affected.

# v0.9.44-beta.1 - Performance Pass and Bug Fixes

## Performance
- Pre-compiled all hot-path regex patterns at module load time in `misc.py`, `utils.py`, and `lists.py`. Patterns previously recompiled on every call now pay their overhead once at startup. The most noticeable gains are in `strip_html` (called for every result description and RSS item), `natural_sort_key` (called O(n log n) when sorting local folders), and `safe_folder_name` (called during every download).
- Removed ~500 leftover blank lines from `wx_main.py` that were introduced during the modular refactor. Python parses every line in a file so these were dead parse-time cost on every startup.

## Fixes
- Fixed a file handle leak in `MpvMixin.start_mpv`. If `subprocess.Popen` raised an exception after the mpv log file had already been opened, the file handle was silently orphaned until the next playback attempt. The exception handler now closes and clears the handle immediately.
- Added `--prerelease` support to `publish_release.ps1`. The script now auto-detects beta/alpha/rc in the tag name and marks the GitHub release as a pre-release automatically, with an explicit `-PreRelease` switch for overrides.

# v0.9.33 - Combo Box Navigation, Updater Reliability, and Locale Completeness

## Fixes
- Fixed arrow key navigation on combo boxes (wx.Choice). Pressing Up/Down while a drop-down control had focus was silently swallowed by the global key hook before the OS COMBOBOX control could act on it. The hook now passes all key events straight through for focused `wx.Choice` and `wx.ComboBox` controls so native arrow, Home, End, and keyboard-search behaviour is fully restored.
- Fixed yt-dlp reload after a component update. `reload_ytdlp_after_component_update` used a `global` statement that resolved to updater.py's module namespace (a stale value copied in via `from apricot.constants import *`) instead of the `apricot.constants` module that `get_yt_dlp()` actually reads. The function now imports `apricot.constants` by name and resets the cached references directly on that module object, so the next call to `get_yt_dlp()` picks up the freshly extracted yt-dlp.
- Fixed update channel setting being silently discarded on load when the stored value is unrecognised. `load_settings` now validates `update_channel` and falls back to `"stable"` instead of passing the raw value through into the Settings object.
- Added missing update-channel locale keys (`update_channel`, `update_channel_stable`, `update_channel_beta`) to all 25 non-English locale files (ar, cs, de, el, es, fi, fr, hi, hr, hu, id, it, ja, ko, nl, pl, pt, ro, ru, sk, sr, sv, tr, uk, zh). Without these keys the Settings screen would display an empty label for the Update Channel control in every non-English language.
- Removed a redundant `import re` statement that was buried inside the `set_lyrics_text` nested function in `MiscUI.show_lyrics`. The `re` module is already imported at module level; the duplicate was dead code and slightly slowed down every lyrics refresh.

# v0.9.32 - Cookie, Navigation, and Modular Cleanup

## Fixes
- Fixed cookies not working on Chrome, Brave, and other Chromium-based browsers. The `youtube_auth_cookie_names` method was missing from `MiscUI` after the modular refactor, causing all cookie validation to crash silently. This unblocks Chrome 127+ App-Bound Encryption detection and the CDP devtools fallback path for Brave and Chrome users.
- Fixed arrow key navigation in the Settings sections list (General, Playback, Equalizer, Downloads, etc.). On Windows, `EVT_LISTBOX` does not reliably fire for keyboard navigation, so section switching now uses an explicit `wx.CallAfter` poll after each arrow/Home/End/PageUp/PageDown key to detect the new selection and render the correct section.
- Fixed `AttributeError` crash when pressing function keys (F1–F24, e.g. F7 for player details). `event_key_code` and `event_raw_key_code` were referenced on `MiscUI` but only existed in `ShortcutsUI`; both are now properly defined in `MiscUI`.
- Fixed `AttributeError` in the equalizer when applying EQ gains. The `equalizer_filter` classmethod incorrectly called `MiscUI.equalizer_clipping_headroom_db` and `MiscUI.equalizer_band_filter`; corrected to `cls.*` since both methods live in `EqualizerUI` itself.
- Fixed `AttributeError` in media-path detection. `looks_like_local_media_path` in `SystemUI` incorrectly called `MiscUI.local_media_path_from_input`; corrected to `SystemUI.local_media_path_from_input`.
- Fixed custom equalizer preset names so newly created profiles show as "Custom 1", "Custom 2", "Custom 3" instead of the literal placeholder "Custom $index" (f-string was missing the brace expression).
- Renamed the internal mpv IPC pipe from the legacy `urhasaurus-youtube` name to `apricotplayer`, removing a leftover identifier from the pre-rename code that could conflict with a second instance on the same machine.
- Hardened crash logging so the startup `error.log` is written to `%APPDATA%\ApricotPlayer\error.log` instead of the current working directory (which is often `C:\Windows\System32` when launched from the Start Menu, where ApricotPlayer has no permission to write).
- Initialised the settings UI control maps (`controls`, `choice_values`, `settings_control_order`) in `MainFrame.__init__` so any helper that runs before the settings screen is opened cannot raise `AttributeError`.
- Removed a dead `if startup_media_path: pass` branch in `App.OnInit` and duplicated module-level globals in `apricot.utils` left over from the modular refactor.

# v0.9.31 - Results and Keyboard Regression Hotfix

## Fixes
- Fixed result-list keyboard ownership so arrows, Space, and letter navigation stay in the results instead of leaking into player shortcuts.
- Fixed Space and Enter activation for command buttons such as Back and Back to main menu when the global key hook is active.
- Stopped normal YouTube search, channel, playlist browsing, subscription checks, and metadata hydration from doing cookie auto-retry or showing cookie sign-in hints.
- Disabled external yt-dlp plugins before loading yt-dlp, preventing stale third-party plugins from changing ApricotPlayer's YouTube behavior.

# v0.9.30 - Modular Codebase Safety Review

## What's New
- Added a stronger release-build preflight that syntax-checks every tracked Apricot module before PyInstaller runs, not just the tiny `wx_main.py` entry point.
- Synchronized the package `apricot.__version__` value with the app version again.

## Fixes
- Hardened locale loading so languages with missing newer keys automatically inherit the English fallback keys at runtime.
- Rechecked MainFrame mixin method availability, module imports, locale key coverage, settings rendering, dynamic results, direct links, local folder sorting, and the major navigable screens after the modular refactor.

# v0.9.29 - Search and Results Stability Hotfix

## What's New
- Restored Enter in the search query box so it reliably starts a search and moves users into the results workflow again.
- Restored Enter in the direct-link field so it still follows the selected direct-link action after the global keyboard hook refactor.

## Fixes
- Hardened result row rendering so partial provider results cannot break the results list when metadata such as channel or view count is missing.
- Rechecked search result loading, dynamic result extension, keyboard interception, syntax compilation, and runtime-risk static analysis after the modular refactor.

# v0.9.23 - The Modular Update & Caraoke

## What's New
- **Massive Codebase Split**: wx_main.py has been split into dedicated modules under the pricot directory! This makes the codebase vastly more maintainable, stable, and less prone to random errors without modifying existing functionality. The logic is now cleanly organized into mixins (PlaybackMixin, MpvMixin, DataManagerMixin, YoutubeMixin, etc.).
- **Caraoke Sync Improvements**: The synchronized lyrics UI was re-implemented using wx.TextCtrl. The UI is now fully copy-pasteable, scrollable, and it no longer reads out timestamps via NVDA when tracking lines, resulting in a much smoother and unintrusive screen-reading experience! 

## Fixes
- Addressed multiple import issues (Settings, Path, locales) and random Traceback popups during launch due to the codebase division. 
- Restored 0.9.23 to full working order with identical UI & features.

# Changelog

## 1.0.2

- Fixed non-YouTube direct links from History failing after an ApricotPlayer restart because an expired HLS or signed media URL could be restored from the persistent stream cache.
- Direct links without a verifiable remote expiry now remain cached only for the current app session. Replaying them after a restart resolves the saved source page again and obtains a fresh playable stream.
- Stream URLs with an explicit `expire`, `expires`, `exp`, or `/expire/` timestamp remain eligible for the persistent cache, preserving fast restarts for verifiably valid YouTube, SoundCloud, and similar URLs.
- Existing legacy cache entries without restart-safety metadata are discarded automatically. History entries, source URLs, volume, EQ, and normal YouTube playback behavior are unchanged.

## 1.0.1

- Added two native numeric controls under Settings > Playback for speed/pitch hold timing: initial hold delay and repeat interval, both expressed in milliseconds.
- Kept the 1.0 defaults at 180 ms before repetition and 110 ms between repeats.
- Users who prefer the faster Beta 61 behavior can set the controls to 90 ms and 45 ms.
- Values are safely bounded to 50-1000 ms for the initial delay and 20-500 ms for the repeat interval.
- The configured timing applies to held `S`, `D`, `Ctrl+Up`, and `Ctrl+Down` controls without changing one-step amounts, volume, seek, EQ, or playback startup.
- Diagnostic reports now include both configured timing values.

## 1.0.0

ApricotPlayer 1.0 is the stable result of the 1.0 beta cycle. Thank you to every tester who reported bugs, retested fixes, shared diagnostics, and helped turn dozens of beta iterations into this release.

### Accessibility and keyboard workflow

- Added Action Finder on `Ctrl+Shift+J`, with searchable global and context-sensitive player actions available from anywhere without adding another permanent main-menu item.
- Added optional live shortcut labels to main-menu actions, player controls, and context menus. Labels follow user-customized shortcuts and can be hidden in Settings.
- Added a configurable Play file shortcut, `F11` full screen, reset speed/pitch, jump-to-start, jump-to-end, BPM, transcript, bookmark, podcast, and metadata-field shortcuts.
- Added a fully customizable main menu. Optional actions use native accessible checkboxes, hidden actions keep their global shortcuts and Action Finder entries, and Settings, Exit, and available updates always remain visible.
- Fixed Tab and Shift+Tab focus order, duplicate screen-reader announcements, hidden background-player controls, Enter and Space activation, CapsLock/special-key false positives, system-tray restoration, and focus return across player, settings, results, channels, playlists, and AudioVault screens.
- Added `Ctrl+Alt+Left` and `Ctrl+Alt+Right` to announce individual metadata fields in results, local folders, favorites, history, subscriptions, RSS, playlists, podcasts, and queue lists.
- Added categories and filters for saved YouTube subscriptions and RSS/podcast feeds, with stable alphabetical ordering and focus preservation.

### Transcripts, bookmarks, comments, and diagnostics

- Added an accessible transcript/captions browser with search, Enter-to-jump timestamps, selected-line and full-transcript copy, timestamp-link copy, and local `.vtt`/`.srt` sidecar support.
- Added Copy lyrics directly to the lyrics window and improved transcript rate-limit handling with yt-dlp fallback and readable error messages.
- Added persistent named bookmarks for local and online media, including add, browse, play, rename, delete, and YouTube timestamp-link actions.
- Expanded YouTube Comments with search, original/newest/oldest/most-liked/most-replies sorting, replies, copy actions, author-channel opening, API-key support, and yt-dlp fallback metadata.
- Added a redacted diagnostic report containing app/runtime, player, audio, result, queue, update, and log context for support without exposing credentials or user-profile paths.

### Audio, speed, pitch, and tempo

- Added ReplayGain/loudness normalization modes with an accessible player control and configurable shortcut.
- Expanded Equalizer Profiles with JSON import/export, A/B comparison, rename/delete support, and per-output-device default presets without changing independent EQ band math.
- Applied active EQ and clipping-protection filters at player startup and hardened old-player handoff behavior to prevent brief boosted-volume or crackling bursts while preserving fast startup.
- Added accessible BPM detection on `B` for YouTube, SoundCloud, podcasts, local media, and AudioVault. It uses the current audio stream, accounts for playback speed, rejects weak/non-rhythmic material, and performs a fresh analysis on every manual request.
- A single `S`, `D`, `Ctrl+Up`, or `Ctrl+Down` press changes speed or pitch by one configured step immediately. Holding starts after 180 ms and repeats every 110 ms without native-repeat duplication.
- Serialized speed and pitch updates prevent thread backlogs, stale announcements, and lost held-key steps. Pending work is cancelled when playback closes or changes media.
- Preserved native held volume behavior and the independent continuous seek implementation.

### Playback, navigation, and sessions

- Added Resume last playback session with optional main-menu visibility, saved position, and restoration of folder, playlist, queue, and explicit player sequences so Next/Previous retain their context.
- Fixed per-item resume positions so opening a new file while another item plays in the background cannot inherit the previous item's timestamp.
- Fixed step-by-step Escape navigation from player to channel/playlist results and then to the originating results screen.
- Added a dedicated related-video command on `Ctrl+Shift+PageDown` while preserving `Ctrl+PageDown` for the normal result, folder, playlist, or queue sequence.
- Improved related autoplay gating and unseen-video selection so settings are respected and suggestions do not loop through the same items.
- Added `Ctrl+0` to restore configured speed and default pitch, plus `Ctrl+Home` and `Ctrl+End` to jump to the start or end of finite media.
- Fixed unexpected mpv exits, stale disk-cache sessions, stream-URL cache persistence, end-of-video resume recovery, background-player screen leakage, and player return/focus behavior.
- Restored fast normal startup after testing a conservative audio gate; rare recovery paths remain deferred until a real failure is detected.

### YouTube and SoundCloud

- Expanded SoundCloud discovery to tracks, playlists, and artists, using ApricotPlayer's accessible result navigation and collection loading.
- Added YouTube Shorts to ordinary search and channel-video results without delaying standard search results when the optional Shorts request is slow.
- Improved channel and playlist upload-time metadata with page-batched YouTube API requests and non-blocking yt-dlp fallback hydration for newly loaded pages.
- Added conservative playback recovery for unavailable formats, VPN/authentication challenges, truncated progressive streams, and valid combined HLS alternatives while keeping successful videos on the one-request fast path.
- Deferred cookies until authentication, age restriction, or extraction failure actually requires them, so importing cookies does not slow unrestricted videos.

### AudioVault

- Added AudioVault account login, exact registration link, Settings controls, DPAPI-protected persistent credentials, logout/reset handling, expired-session reauthentication, and automatic login prompts when credentials are missing.
- Added accessible movie and TV-show search, recently added movies/shows, direct movie playback, TV episode lists, Enter activation, episode Next/Previous navigation, and correct step-by-step return behavior.
- Added movie, episode, and complete-show audio downloads with the same global save-location rule used by other sources; video download clearly reports that AudioVault is audio-only.
- Made TV-show browsing fast by reading remote ZIP directories through bounded range requests, downloading only selected uncached episodes, and safely falling back to full packages when necessary.
- Added safe archive validation/extraction, temporary on-demand episode caching with LRU cleanup, protected active and complete-show files, permanent download destinations, and screen-reader progress through ApricotPlayer's stable status UI.
- Fixed bundled mpv discovery across installer, updater, portable, and source layouts; generic YouTube routing; expired login redirects; duplicate loading jobs; native progress crashes; and unrelated YouTube dynamic loading in AudioVault lists.

### Podcasts and libraries

- Added podcast/RSS played and unplayed state, played counts, automatic completion marking, configurable toggle shortcut, and state preservation across feed refreshes.
- Added podcast resume labels, clear-progress actions, and per-podcast playback-speed presets available from feed menus and the active player.
- Added complete-feed download destination prompts and retained podcast chapters, transcripts, downloads, and resume behavior in the unified player workflow.
- Restored and stabilized library sorting, category filtering, queue/session context, and selected-item focus after background updates.

### Downloads, cookies, and network recovery

- Moved playlist, channel, and large queued downloads into a separate non-blocking progress window so ApricotPlayer remains usable and only the progress window closes at completion.
- Restored fast audio-only download defaults, reduced unnecessary worker startup delay, and retained proven video fragment/FFmpeg behavior.
- Made batch downloads continue after individual unavailable or failed entries while announcing errors and reporting the final failure count.
- Unified Ask where to save each download across YouTube, SoundCloud, podcasts, AudioVault movies/episodes, complete shows, and complete podcast feeds.
- Made external cookie files refresh by content signature, preserve their original selected path, invalidate stale login/cache state, and avoid obsolete Chrome DevTools retries.
- Restored conservative anonymous-first cookie fallback, then added targeted Node/EJS and alternate-client recovery only for demonstrated authentication, VPN, format, or unavailable-video failures.

### Security, updates, and maintenance

- Hardened app and yt-dlp updates with required SHA-256 digests, trusted HTTPS redirects, bounded downloads, validated package layouts, transactional portable replacement, and rollback of both the executable and `_internal` runtime.
- Hardened ZIP/XML/network handling against traversal, alternate data streams, Windows device names, links, special files, compression bombs, DTD/entities, oversized responses, unsafe redirects, credential leakage, and generated-path escape.
- Switched settings, history, favorites, bookmarks, queues, positions, playlists, subscriptions, notifications, sessions, and stream-cache state to atomic writes with generation guards against stale background saves.
- Fixed installer updates that omitted `_internal`, release tags targeting the wrong commit, and update-channel defaults so beta builds follow beta while stable builds follow stable.
- Added dependency/security CI, Dependabot, current SHA-pinned GitHub Actions, locked release dependencies, strict release-package validation, and verified rollback tests.
- Updated bundled yt-dlp, mpv, FFmpeg, and Node runtimes during the beta cycle and cleaned obsolete extracted executables, refactor scripts, and local artifacts from the repository.
- Kept the modular wxPython architecture while adding focused regression coverage for AudioVault, security, downloads, menu customization, player resolution, key holds, BPM, and release packaging.

## 1.0.0 Beta 66

### Smooth held speed and pitch controls

- A single `S`, `D`, `Ctrl+Up`, or `Ctrl+Down` press still applies exactly one configured speed or pitch step immediately.
- Holding a speed or pitch shortcut now starts repeating after 180 milliseconds and continues every 110 milliseconds, matching the first stable hold cadence from Beta 60 rather than the over-fast Beta 61 cadence.
- Native Windows repeat events for the same held shortcut are ignored while ApricotPlayer controls the hold, preventing duplicate steps and growing input lag.
- Rapid speed and pitch requests are accumulated through one worker per control. They no longer create a backlog of competing threads, stale announcements, or unchanged pitch notifications after the key is released.
- Pending hold work is cancelled when playback closes or changes media. Volume remains on its existing native Windows key-repeat path and was not changed.

Beta 66 is the final beta for ApricotPlayer 1.0 and is functionally identical to the stable 1.0 release on the `main` branch. Thank you to every tester who reported bugs, retested fixes, sent diagnostics, and kept pushing the beta cycle toward a release we can trust.

### Complete changes from 0.9.57 to 1.0

#### Accessibility and keyboard workflow

- Added Action Finder on `Ctrl+Shift+J`, with searchable global and context-sensitive player actions available from anywhere without adding another permanent main-menu item.
- Added optional live shortcut labels to main-menu actions, player controls, and context menus. Labels follow user-customized shortcuts and can be hidden in Settings.
- Added a configurable Play file shortcut, `F11` full screen, reset speed/pitch, jump-to-start, jump-to-end, BPM, transcript, bookmark, podcast, and metadata-field shortcuts.
- Added a fully customizable main menu. Optional actions use native accessible checkboxes, hidden actions keep their global shortcuts and Action Finder entries, and Settings, Exit, and available updates always remain visible.
- Fixed Tab and Shift+Tab focus order, duplicate screen-reader announcements, hidden background-player controls, Enter and Space activation, CapsLock/special-key false positives, system-tray restoration, and focus return across player, settings, results, channels, playlists, and AudioVault screens.
- Added `Ctrl+Alt+Left` and `Ctrl+Alt+Right` to announce individual metadata fields in results, local folders, favorites, history, subscriptions, RSS, playlists, podcasts, and queue lists.
- Added categories and filters for saved YouTube subscriptions and RSS/podcast feeds, with stable alphabetical ordering and focus preservation.

#### Transcripts, bookmarks, comments, and diagnostics

- Added an accessible transcript/captions browser with search, Enter-to-jump timestamps, selected-line and full-transcript copy, timestamp-link copy, and local `.vtt`/`.srt` sidecar support.
- Added Copy lyrics directly to the lyrics window and improved transcript rate-limit handling with yt-dlp fallback and readable error messages.
- Added persistent named bookmarks for local and online media, including add, browse, play, rename, delete, and YouTube timestamp-link actions.
- Expanded YouTube Comments with search, original/newest/oldest/most-liked/most-replies sorting, replies, copy actions, author-channel opening, API-key support, and yt-dlp fallback metadata.
- Added a redacted diagnostic report containing app/runtime, player, audio, result, queue, update, and log context for support without exposing credentials or user-profile paths.

#### Audio, speed, pitch, and tempo

- Added ReplayGain/loudness normalization modes with an accessible player control and configurable shortcut.
- Expanded Equalizer Profiles with JSON import/export, A/B comparison, rename/delete support, and per-output-device default presets without changing independent EQ band math.
- Applied active EQ and clipping-protection filters at player startup and hardened old-player handoff behavior to prevent brief boosted-volume or crackling bursts while preserving fast startup.
- Added accessible BPM detection on `B` for YouTube, SoundCloud, podcasts, local media, and AudioVault. It uses the current audio stream, accounts for playback speed, rejects weak/non-rhythmic material, and now performs a fresh analysis on every manual request.
- Added deterministic held speed and pitch controls with immediate first steps, 180/110 ms repetition, native-repeat suppression, serialized updates, and clean cancellation across media changes.
- Preserved native held volume behavior and the independent continuous seek implementation.

#### Playback, navigation, and sessions

- Added Resume last playback session with optional main-menu visibility, saved position, and restoration of folder, playlist, queue, and explicit player sequences so Next/Previous retain their context.
- Fixed per-item resume positions so opening a new file while another item plays in the background cannot inherit the previous item's timestamp.
- Fixed step-by-step Escape navigation from player to channel/playlist results and then to the originating results screen.
- Added a dedicated related-video command on `Ctrl+Shift+PageDown` while preserving `Ctrl+PageDown` for the normal result, folder, playlist, or queue sequence.
- Improved related autoplay gating and unseen-video selection so settings are respected and suggestions do not loop through the same items.
- Added `Ctrl+0` to restore configured speed and default pitch, plus `Ctrl+Home` and `Ctrl+End` to jump to the start or end of finite media.
- Fixed unexpected mpv exits, stale disk-cache sessions, stream-URL cache persistence, end-of-video resume recovery, background-player screen leakage, and player return/focus behavior.
- Restored fast normal startup after testing a conservative audio gate; rare recovery paths remain deferred until a real failure is detected.

#### YouTube and SoundCloud

- Expanded SoundCloud discovery to tracks, playlists, and artists, using ApricotPlayer's accessible result navigation and collection loading.
- Added YouTube Shorts to ordinary search and channel-video results without delaying standard search results when the optional Shorts request is slow.
- Improved channel and playlist upload-time metadata with page-batched YouTube API requests and non-blocking yt-dlp fallback hydration for newly loaded pages.
- Added conservative playback recovery for unavailable formats, VPN/authentication challenges, truncated progressive streams, and valid combined HLS alternatives while keeping successful videos on the one-request fast path.
- Deferred cookies until authentication, age restriction, or extraction failure actually requires them, so importing cookies does not slow unrestricted videos.

#### AudioVault

- Added AudioVault account login, exact registration link, Settings controls, DPAPI-protected persistent credentials, logout/reset handling, expired-session reauthentication, and automatic login prompts when credentials are missing.
- Added accessible movie and TV-show search, recently added movies/shows, direct movie playback, TV episode lists, Enter activation, episode Next/Previous navigation, and correct step-by-step return behavior.
- Added movie, episode, and complete-show audio downloads with the same global save-location rule used by other sources; video download clearly reports that AudioVault is audio-only.
- Made TV-show browsing fast by reading remote ZIP directories through bounded range requests, downloading only selected uncached episodes, and safely falling back to full packages when necessary.
- Added safe archive validation/extraction, temporary on-demand episode caching with LRU cleanup, protected active and complete-show files, permanent download destinations, and screen-reader progress through ApricotPlayer's stable status UI.
- Fixed bundled mpv discovery across installer, updater, portable, and source layouts; generic YouTube routing; expired login redirects; duplicate loading jobs; native progress crashes; and unrelated YouTube dynamic loading in AudioVault lists.

#### Podcasts and libraries

- Added podcast/RSS played and unplayed state, played counts, automatic completion marking, configurable toggle shortcut, and state preservation across feed refreshes.
- Added podcast resume labels, clear-progress actions, and per-podcast playback-speed presets available from feed menus and the active player.
- Added complete-feed download destination prompts and retained podcast chapters, transcripts, downloads, and resume behavior in the unified player workflow.
- Restored and stabilized library sorting, category filtering, queue/session context, and selected-item focus after background updates.

#### Downloads, cookies, and network recovery

- Moved playlist, channel, and large queued downloads into a separate non-blocking progress window so ApricotPlayer remains usable and only the progress window closes at completion.
- Restored fast audio-only download defaults, reduced unnecessary worker startup delay, and retained proven video fragment/FFmpeg behavior.
- Made batch downloads continue after individual unavailable or failed entries while announcing errors and reporting the final failure count.
- Unified Ask where to save each download across YouTube, SoundCloud, podcasts, AudioVault movies/episodes, complete shows, and complete podcast feeds.
- Made external cookie files refresh by content signature, preserve their original selected path, invalidate stale login/cache state, and avoid obsolete Chrome DevTools retries.
- Restored conservative anonymous-first cookie fallback, then added targeted Node/EJS and alternate-client recovery only for demonstrated authentication, VPN, format, or unavailable-video failures.

#### Security, updates, and maintenance

- Hardened app and yt-dlp updates with required SHA-256 digests, trusted HTTPS redirects, bounded downloads, validated package layouts, transactional portable replacement, and rollback of both the executable and `_internal` runtime.
- Hardened ZIP/XML/network handling against traversal, alternate data streams, Windows device names, links, special files, compression bombs, DTD/entities, oversized responses, unsafe redirects, credential leakage, and generated-path escape.
- Switched settings, history, favorites, bookmarks, queues, positions, playlists, subscriptions, notifications, sessions, and stream-cache state to atomic writes with generation guards against stale background saves.
- Fixed installer updates that omitted `_internal`, release tags targeting the wrong commit, and update-channel defaults so beta builds follow beta while stable builds follow stable.
- Added dependency/security CI, Dependabot, current SHA-pinned GitHub Actions, locked release dependencies, strict release-package validation, and verified rollback tests.
- Updated bundled yt-dlp, mpv, FFmpeg, and Node runtimes during the beta cycle and cleaned obsolete extracted executables, refactor scripts, and local artifacts from the repository.
- Kept the modular wxPython architecture while adding focused regression coverage for AudioVault, security, downloads, menu customization, player resolution, key holds, BPM, and release packaging.

## 1.0.0 Beta 65

- Pressing `B` now always starts a fresh BPM analysis, even when the current item, speed, and pitch have not changed since the previous result.
- Removed the BPM result cache so a previous estimate or unavailable result is never replayed as a new measurement.
- Kept one analysis per playback state in flight at a time, preventing rapid repeated key presses from launching competing FFmpeg processes.

## 1.0.0 Beta 64

- Restored pitch Up/Down to the exact pre-Beta-60 input path. Every native Windows key-repeat event now applies one configured pitch step directly.
- Removed the custom 90/45 ms volume-and-pitch hold controller and the pitch request batching introduced after Beta 59.
- Volume and pitch now both rely on native key repeat without an ApricotPlayer delay or accelerated repeat timer.
- Kept the separate seek hold implementation unchanged.

## 1.0.0 Beta 63

- Fixed normal YouTube searches waiting for the optional Shorts endpoint. Standard results now appear as soon as their primary request finishes, while Shorts are included only when their parallel request is already ready.
- Reduced retries for the optional Shorts request so a slow or incomplete Shorts response does not keep unnecessary background work alive.
- BPM results are now tied to the current item, speed, and pitch. Changing speed or pitch and pressing `B` starts a fresh analysis instead of replaying the original cached result.
- Reported BPM now reflects playback speed, for example a detected 120 BPM track playing at `1.25x` announces 150 BPM.
- Restored the pre-Beta-60 native key-repeat path for volume Up/Down, removing the newer fast volume timer. Pitch keeps its current deterministic held-key behavior.
- BPM analysis code is loaded only when `B` is pressed and adds no import work to ordinary navigation, searching, playback, seek, or volume paths.
- Release builds now prefer the project's dependency-complete virtual environment, preventing packaged startup failures when the system Python is missing a required runtime module.

## 1.0.0 Beta 62

- Added accessible BPM detection in the player. Press `B` to analyze the actual audio and hear the estimated tempo; the shortcut is configurable under Settings > Keyboard shortcuts.
- BPM analysis works through the current resolved stream for YouTube, SoundCloud, podcasts, local media, and AudioVault without adding work to normal playback startup.
- The detector uses a two-band FFmpeg extraction plus onset strength, tempo autocorrelation, harmonic resolution, and multi-segment stability checks. Unrhythmic speech, silence, and weak results announce `BPM not available` instead of inventing a tempo.
- Analysis runs in the background, is cancelled when playback changes, and is cached per item so repeated checks answer immediately.
- Added synthetic tempo coverage from 60 through 180 BPM, speech-like rejection, steady-tone rejection, live-stream handling, safe HTTP headers, and a real bundled-FFmpeg integration check.

## 1.0.0 Beta 61

- Volume and pitch still react immediately on the first key press, but held-key repetition now starts after 90 ms instead of 180 ms.
- Continued held-key changes now run every 45 ms instead of every 110 ms for substantially smoother volume and pitch adjustment.
- The faster cadence applies equally to Up/Down volume controls and Ctrl+Up/Ctrl+Down pitch controls. Configured one-step amounts remain unchanged.

## 1.0.0 Beta 60

- Volume Up/Down and Pitch Up/Down now use the same deterministic hold controller as seek. A short press still changes exactly one configured step; holding starts repeating after 180 ms and continues every 110 ms until the key is released.
- Holding no longer depends on Windows delivering every native key-repeat event, so it remains consistent even when the player UI is handling background sections, results, or screen-reader updates.
- Native repeat events for the same held shortcut are ignored while ApricotPlayer's hold controller is active, preventing doubled steps.
- Rapid pitch changes are accumulated and applied through one worker instead of racing in separate threads. Ten `0.01x` changes now reliably produce a `0.10x` change.
- Pending holds and pitch work are cancelled when the player closes or switches media. Seek timing and the user's configured volume and pitch step sizes are unchanged.

## 1.0.0 Beta 59

- Made Ask where to save each download a single global rule for YouTube, SoundCloud, individual and complete podcast downloads, and AudioVault downloads.
- AudioVault movies and episodes now ask for an exact destination when the global option is enabled. Complete TV shows ask for a destination folder and keep downloading every episode into it.
- Complete podcast feed downloads now ask for one destination folder and place every episode there when the global option is enabled.
- Cancelling either destination dialog cleanly cancels the download before a background task starts.
- AudioVault destination dialogs start in the matching AudioVault movie, show, or episode folder. Copying a complete show into an existing chosen folder no longer removes unrelated files from that folder.

## 1.0.0 Beta 58

- Fixed native Windows crashes after an uncached AudioVault episode finished preparing or downloading. All AudioVault preparation, download, and extraction progress now uses the existing status area instead of a separate native progress dialog.
- AudioVault now explains that on-demand playback uses a temporary cache and that Download audio creates a permanent copy.
- On-demand AudioVault episode files use the configured playback-cache size as an LRU limit. The active episode is protected, and complete-show caches remain intact.
- Manual episode and show downloads now announce their exact destination path.
- Verified the original uncached playback crash, the separate manual-download crash, and the cached control path with the installed application and Windows UI automation before applying the fix.

## 1.0.0 Beta 57

- TV-show results now open their episode list from the remote ZIP directory using bounded HTTP range requests instead of downloading and extracting the complete show first.
- Selecting an uncached episode downloads and verifies only that episode; cached episodes reopen immediately and next/previous episode navigation keeps using the AudioVault sequence.
- Full-show downloads and servers without byte-range support use a visible, screen-reader-named progress dialog for download and extraction.
- AudioVault result navigation is isolated from YouTube dynamic loading, fixing the unrelated empty `youtube.com/results?search_query=&sp=` error at the end of AudioVault lists and after returning from playback.
- Repeated Enter presses no longer start duplicate show-manifest or episode jobs, and leaving a loading show prevents a late result from reopening it.

## 1.0.0 Beta 56

- Fixed Enter on the AudioVault submenu doing nothing for Search, recently added TV shows, and recently added movies.
- Fixed Enter on AudioVault movies and shows going through the generic YouTube results path, which could surface `Unsupported URL: https://direct.audiovault.net/login` instead of playing the selected content.
- Protected AudioVault catalog, stream, TV-show, and download requests now detect an expired server session, securely sign in again from saved DPAPI-protected credentials, and retry the original action once.
- If no saved AudioVault credentials are available, the login dialog still opens on the first AudioVault action. Normal application updates preserve the saved email and protected password.
- File access errors remain file/download errors and no longer risk being mistaken for an expired AudioVault session.
- Added focused regression tests for both Enter routes, missing credentials, catalog login redirects, stream reauthentication, and file-permission handling.

## 1.0.0 Beta 55

- SoundCloud search now offers Track, Playlist, and Artist types. Playlists open through the existing playlist workflow, while artists open their track list with the same accessible result navigation and dynamic loading.
- AudioVault now opens as an `AudioVault` submenu with Search, recently added TV shows, and recently added movies. Fixed TV-show searches silently failing when results were rendered, and restored step-by-step return navigation from episodes and playback.
- YouTube searches and channel video lists now include Shorts alongside ordinary videos. Shorts already present in playlists remain normal playable results, with no separate Shorts-only screen.
- Added regression coverage for SoundCloud result types, AudioVault section parsing and rendering, Shorts URL playback, channel merging, provider state, and the permanent application-update menu action.

## 1.0.0 Beta 54

- Application update notifications can no longer be hidden through Customize main menu.
- When an update is available, its action is always shown at the top of the main menu regardless of saved customization from earlier betas.
- Other optional and session-specific main-menu actions remain customizable.

## 1.0.0 Beta 53

- Replaced the Customize main menu checklist with individual native checkbox controls that screen readers identify as checked or unchecked.
- Up and Down move between menu-item checkboxes, Home and End jump to the first or last checkbox, and Space uses the native checkbox action with screen-reader state feedback.
- Main-menu visibility, saved preferences, resets, global shortcuts, and Action Finder behavior remain unchanged.

## 1.0.0 Beta 52

- Added a Customize main menu section in Settings with an accessible checklist of every permanent and conditional main-menu action.
- Unchecked actions are hidden after saving while their global keyboard shortcuts and Action Finder entries continue to work.
- Settings and Exit are permanent and cannot be hidden. All customizable actions are enabled by default and return to enabled when the section or all settings are reset.
- Dynamic entries such as application updates, current downloads, playback queue, and resume last session respect the same visibility preference whenever they become available.

## 1.0.0 Beta 51

- Channel and playlist video upload times now use one batched YouTube Data API request when an API key is configured, so the first visible page can open with complete upload metadata instead of waiting minutes for individual yt-dlp lookups.
- Loading more collection results fetches metadata only for the newly added page and does not repeat requests for videos already shown.
- If the YouTube API is unavailable or no key is configured, the existing non-blocking yt-dlp background hydration remains available as a fallback.

## 1.0.0 Beta 50

- AudioVault login now persists across ApricotPlayer restarts. The saved password is encrypted with Windows DPAPI for the current Windows user and is never stored as plain text.
- The initial AudioVault login dialog now focuses the email field instead of the OK button.
- Logging out, changing the saved email, resetting AudioVault settings, or restoring all defaults now also clears the protected saved login and active AudioVault session.
- Fixed channel and playlist upload ages often appearing only for the first few results. Metadata continues loading in small background batches without delaying the initial result list.

## 1.0.0 Beta 49

- Fixed AudioVault movie playback falsely reporting that the internal mpv player was missing in some installed or updater layouts. Apricot now checks the frozen runtime, installed `_internal` folder, portable folder, source vendor folder, and system PATH.
- Fixed Enter on an AudioVault TV show calling an obsolete parameter name instead of opening or preparing its episode list.
- AudioVault result lines no longer include irrelevant uploaded-age metadata.
- The experimental libmpv backend is not included in this release; beta 49 continues to use the established `mpv.exe` process backend.

## 1.0.0 Beta 48

- Added an accessible AudioVault integration with account login, exact registration link, movie and TV-show search, keyboard navigation, direct movie playback, episode lists, and audio downloads.
- TV-show packages are cached once, safely extracted with path and size validation, naturally sorted, and exposed as normal playable episode lists with previous and next navigation.
- Added AudioVault account controls in Settings and a configurable `Ctrl+Alt+A` shortcut. AudioVault video downloads are explicitly rejected because the service provides audio-only described content.
- AudioVault sessions remain in memory and passwords are never written to settings. Requests are restricted to trusted AudioVault HTTPS hosts and avoid background catalog crawling to respect the service's daily limits.
- The normal YouTube, local playback, mpv startup, seek, volume, bass boost, and equalizer paths are unchanged.

## What's new in version 0.9.20

- fixed an issue where searching on SoundCloud incorrectly treated single tracks as "Channels" and prevented them from loading. The search filters will now dynamically lock when SoundCloud is selected, ensuring all tracks load and play instantly. Direct SoundCloud profile links pasted into the search bar are also now fully supported!

## What's new in version 0.9.19

- added Synced Lyrics (.lrc) / Karaoke Mode! When reading lyrics for a song, the app will automatically fetch synchronized `.lrc` lyrics from the web and highlight the currently playing line, keeping in perfect sync with the playback.
- added Lyrics Provider selection in Settings. Users can choose between multiple providers like LRCLIB and NetEase to ensure they get the best synchronized lyrics.
- added SoundCloud integration! The main search bar now has a dropdown menu to choose between "YouTube" and "SoundCloud", allowing you to search and play directly from the SoundCloud catalog using native yt-dlp capabilities.
- ensured complete translation for the new search and lyrics UI across all 27 supported languages.

## What's new in version 0.9.18

- fixed keyboard accessibility where the Session Autoplay Checkbox (and other playback checkboxes) did not respond properly to Space or Enter keys when focused.
- added "Autoplay Suggested Videos" feature: when enabled and YouTube autoplay next is active, instead of playing the next item from the main search results list, the player dynamically fetches and plays a related or suggested video recommended by YouTube.
- added global setting under playback options to enable/disable "Autoplay Suggested Videos".
- fully translated the new setting into all 27 supported languages.

## What's new in version 0.9.17

- added support for Live Streams under channel options dialog and within the channel context menu (Issue 1).
- added import from / export to OPML for Podcasts and RSS Feeds (Issue 2).
- expanded podcast search catalog coverage to support 49 countries including AR, BE, BR, CH, CL, CO, CZ, DK, EG, FI, GR, HK, HU, ID, IE, IL, IN, JP, KR, MX, NO, NZ, PH, PT, RO, RU, SG, SK, TH, TR, TW, UA, VN, ZA (Issue 2).
- added "Play file" feature to open and play individual local media files symmetrically to "Play folder" (Issue 3).
- fixed and verified internal mpv player environment issues.

## What's new in version 0.9.16

- fixed multiple type annotation errors in `wx_main.py` involving global imports of `http.cookiejar`, `xml.etree.ElementTree as ET`, and `zipfile`.
- improved codebase stability and correctness by resolving static analysis undefined name errors.

## What's new in version 0.9.15

- completed static analysis code review.

## What's new in version 0.9.14

- added a player-session auto-play next checkbox that appears only when the global auto-play next setting is off.
- kept the session auto-play setting alive across new items while the player stays open, then reset it when the player is closed.
- fixed Play playlist navigation so playlist items use a stable now-playing sequence instead of being consumed from the playback queue.
- fixed playlist Previous/Next so it no longer skips several videos or says there is no previous item while a previous playlist item exists.

## What's new in version 0.9.13

- fixed dynamic Next at the end of the currently visible results so loading more results preserves the already-loaded order instead of adopting a reshuffled YouTube response.
- when player Next triggers dynamic loading, ApricotPlayer now anchors the merge to the currently playing URL, not the possibly stale focused row in Results.
- appended newly fetched results after the current/last loaded item first, preventing Next from skipping several videos when YouTube returns the expanded search page in a slightly different order.

## What's new in version 0.9.12

- made session volume the authoritative value between songs, so unstable mpv reads during track changes no longer reset playback to 100, 0, or the saved default volume.
- cancelled pending volume timers when carrying volume into the next item, preventing an old volume key press from landing on the newly started mpv process.
- added player-generation guards to delayed volume and volume-boost workers so stale volume updates cannot modify the next song.
- preserved boosted session volumes across next/previous playback when the current session volume is above 100.

## What's new in version 0.9.11

- made result playback navigation use the result collection that contains the currently playing URL instead of whichever list is longest, preventing next from jumping to unrelated result positions.
- synchronized the embedded Results selection with the item that actually starts playing, so focus no longer stays on an older row while a later video is playing.
- stopped ambiguous dynamic next completion from falling back to shuffle or stale indexes; if the exact current URL cannot be continued, ApricotPlayer now says there is no next item.
- blocked player-only shortcuts such as chapters, lyrics, comments, and details while focus is in Results, while keeping Ctrl+PageUp and Ctrl+PageDown available for player previous and next.
- reset shuffle when a normal result is opened directly, so an earlier Shuffle folder or Shuffle playlist action does not make later search navigation random.

## What's new in version 0.9.10

- fixed result-list letter navigation while the embedded player layout is active: plain keys such as P and V now stay with the list instead of triggering player shortcuts.
- kept Ctrl+PageUp and Ctrl+PageDown available from results for player previous and next, but made unmodified result-list keys belong to the list.
- made player-next after dynamic loading continue from the exact current video URL, so the next item is deterministic instead of falling back to stale index or shuffle state.
- limited player-triggered dynamic next loading to search and trending result contexts, so folders and other finite lists keep their normal no-next behavior.

## What's new in version 0.9.9

- fixed results-list arrow navigation after returning from the player: plain Up/Down/Page/Home/End now stay with the results list instead of being interpreted as player volume shortcuts.
- made player Ctrl+PageDown load the next dynamic result page when it reaches the end of the currently loaded results, then continue playback automatically.
- restored the correct search/channel/playlist dynamic-loading context when returning from the player, preventing stale playlist URLs from causing unrelated "playlist unviewable" errors while browsing results.
- changed dynamic-load failures to non-modal status/screen-reader messages so navigating the result list does not get trapped by an error dialog.

## What's new in version 0.9.8

- restored folder queue creation for the explicit Play folder and Shuffle folder actions.
- kept single-file playback clean: pressing Enter or Play on one local file does not auto-fill the queue with the whole folder.
- preserved manually queued items when Play folder or Shuffle folder creates its folder queue; the new folder queue plays first, then the previous manual queue remains.

## What's new in version 0.9.7

- restored the intended queue priority for Next and end-of-item autoplay: manually queued items are still played before the normal folder/result next item.
- kept the 0.9.6 folder fix by ignoring old auto-generated folder queue entries, so Play folder no longer overwrites or pollutes the real playback queue.
- made next-item stream prefetch ignore legacy auto-folder queue entries and prefetch the first real queued item instead.

## What's new in version 0.9.6

- fixed folder Previous and Next shortcuts by repairing the local-folder relative playback path.
- stopped Play folder from generating and overwriting the real playback queue; folder navigation now uses the folder result list while the queue stays user-controlled.
- fixed end-of-file autoplay so queued items are only consumed automatically when "Automatically play next item" is enabled.
- added a Clear queue button and context-menu action to Playback queue.
- made player shortcuts work across the player screen, including when focus is on the embedded results list, so Up/Down volume and T time are handled consistently.
- clarified the Gapless playback setting label: it only controls mpv's audio transition behavior and does not choose or start the next item.

## What's new in version 0.9.5

- changed the File converter and Folder converter format control so screen readers land on a single "Format to convert to" combo box instead of a separate "Detected format" field.
- made opening a new local file while ApricotPlayer is hidden in the system tray feel immediate again: the existing instance is shown and foregrounded right away, with the new file title applied before playback is rebuilt.
- kept the 0.9.2 screen-reader title fix by setting the new local file title before the window is restored, so the old file title should not be announced first.

## What's new in version 0.9.4

- fixed the Chapters dialog so Enter on a selected chapter immediately jumps there and closes the chapter list, matching the Play button and double-click behavior.
- fixed tray restore after Alt+F4/background playback: the running ApricotPlayer window is now explicitly shown, restored, raised, and focused when reopening from a file, desktop shortcut, or tray action.
- fixed Play folder after YouTube playback so the folder screen keeps its own isolated local-file list instead of accidentally reusing old YouTube results.
- tightened folder queue state so Play folder, individual folder items, Previous, and Next use the same local folder list and folder return metadata.
- added natural local-file sorting so numbered files such as `Telemach(1).mp3`, `Telemach(2).mp3`, and `Telemach(10).mp3` appear in numeric order, with the base file before numbered copies.

## What's new in version 0.9.3

- added podcast chapter support beyond embedded media chapters: ApricotPlayer now reads inline Podlove/PSC chapters from podcast RSS items.
- added support for Podcasting 2.0 `podcast:chapters` JSON chapter URLs, loaded lazily when the Chapters screen or chapter shortcuts are used.
- expanded chapter time parsing so chapter starts such as `00:01:23.500`, `01:23`, numeric seconds, `start`, and `startTime` all work.
- added a manual Check yt-dlp updates now button next to the automatic yt-dlp update setting.
- manual yt-dlp checks now announce when YouTube support is already up to date.

## What's new in version 0.9.2

- fixed opening a new local file from disk while ApricotPlayer is already running: the existing instance now switches to the new file before it brings the window forward, so screen readers do not announce the previous title first.
- added a player-only Copy link at current time action. The default shortcut is Ctrl+Shift+L and the player context menu shows it for YouTube videos.
- moved the default Lyrics shortcut to Ctrl+Shift+Y so it does not conflict with the new timestamp-link shortcut.
- changed local-file player controls from Copy link to Copy path, and hid Copy direct media URL for local files.
- renamed Show video details / Player: video details to Show details / Player: show details.

## What's new in version 0.9.1

- made YouTube comments work without a YouTube Data API key by defaulting to the yt-dlp comments path when no key is configured.
- kept the YouTube Data API path for users who do add a key, with a first-page fallback to yt-dlp if the API request fails.
- added an Obtain YouTube API key button in Settings, Cookies and network, which opens the official Google Cloud credentials page.
- fixed comment loading to use the stable YouTube watch URL instead of direct media stream URLs, and kept loading tied to the video that opened the comments screen.
- fixed small 0.9 race conditions in lyrics and chapter metadata while moving quickly between player items.

## What's new in version 0.9

- added a Chapters button and shortcuts for chapter navigation: open the chapters list, press Enter to seek to a chapter, press Escape to return to the player, and use Alt+Left / Alt+Right in the player to jump between chapters with screen reader announcements.
- added a Lyrics screen with local sidecar lyric support and LRCLIB online lookup when local lyrics are missing.
- added a Comments screen for YouTube videos with 20-at-a-time loading through the YouTube Data API when an API key is configured, plus a yt-dlp fallback for the first comments page when no API key is available.
- added keyboard shortcut settings for chapters, lyrics, comments, previous chapter, and next chapter.
- added low-risk player polish from the 0.9 roadmap: mpv-level gapless audio where supported and optional ReplayGain / loudness normalization.

## What's new in version 0.8.72

- fixed Bass boost changing or losing depth when Volume boost is turned on by replacing the EQ filter atomically: the new Bass boost/EQ chain is added before the old one is removed.
- delayed the EQ refresh until after mpv accepts the Volume boost volume range, reducing filter races during playback.

## What's new in version 0.8.71

- fixed tray/session restore after the 0.8.70 single-instance guard: launching ApricotPlayer again now visibly brings the existing hidden instance back instead of silently exiting.
- made activation restore more direct inside the running app and added a Windows-level restore fallback from the second launch process.

## What's new in version 0.8.70

- hardened startup single-instance handling with an early Windows mutex so rapid repeated launches cannot open multiple ApricotPlayer windows.
- when ApricotPlayer is already open, a second launch now restores the existing tray-enabled session; without the tray option it shows "ApricotPlayer is already open." instead of silently opening another instance.

## What's new in version 0.8.69

- renamed the visible results label to "Results" (and Slovenian "Rezultati"), so screen readers no longer announce "list" twice.

## What's new in version 0.8.68

- made player volume a true session value: volume changes are remembered while the player stays open, including when starting a completely different search, trending, playlist, podcast, queue, or folder item.
- reset the session volume back to the configured default only when the player is actually closed.
- fixed background playback menus so screens opened from the main menu build their own controls and lists first, then append the background Player section; this prevents the player buttons from hiding or taking over the opened screen.

## What's new in version 0.8.67

- fixed equalizer custom profile creation so the profile-name edit box starts blank and blank names become the next Custom profile instead of saving the label text as the name.
- stopped new equalizer profiles and Settings slider edits from writing custom gains into factory presets such as Default / flat.
- added Play playlist and Shuffle playlist to the result context menu for playlist results; Enter still opens the playlist videos list as before.

## What's new in version 0.8.66

- fixed the remaining Settings Shift+Tab duplicate announcement by removing the extra manual reverse-focus handler and relying on wx's native tab traversal from the first setting back to the section list.
- applies an explicit Settings tab order from the section list through the visible controls, matching the older main-menu focus-spam fix by stabilizing controls instead of rebuilding or refocusing them unnecessarily.

## What's new in version 0.8.65

- fixed the remaining duplicate Settings announcement by cancelling the delayed initial section-list focus repair as soon as Settings receives real keyboard navigation.
- handles Tab and Shift+Tab on Settings controls at the key-down stage, so reverse navigation back to General is consumed before wx can also process the same key.

## What's new in version 0.8.64

- fixed Settings keyboard navigation so Shift+Tab from the first setting back to the section list focuses General only once instead of letting wx and the focus repair path announce General twice.

## What's new in version 0.8.63

- reduced launch work by lazy-loading several helper modules that are only needed for networking, updates, cookies, external processes, hashing, sockets, and Windows accessibility helpers.
- moved the saved Podcasts/RSS feed list out of the startup path; it now loads when opening Podcasts/RSS or when a scheduled RSS refresh actually runs.
- kept those lazy-loaded standard-library modules explicit in the PyInstaller build so packaged releases still include everything needed at runtime.

## What's new in version 0.8.62

- fixed Settings keyboard navigation so pressing Tab immediately after opening Settings moves from the section list to the Language combo box instead of being pulled back to General.
- stopped delayed Settings focus repair from stealing focus after the user has already tabbed into the visible settings controls.
- kept the selected audio output device for the whole app session, including when starting another item from Search or Trending while the player is still running.
- made the background-player Full screen control toggle full screen on and off, and debounced Enter on the player full-screen checkbox so it cannot double-toggle or fall through to play/pause.
- renamed embedded/search results to "Results" instead of "Search YouTube", so local folder playback no longer announces YouTube wording.
- narrowed the low and low-mid EQ bands, especially 125 Hz upper bass warmth, to reduce muddy/watery overlap into vocals and neighboring instruments.

## What's new in version 0.8.61

- made first-screen startup lighter by loading cookie, RSS, browser, zip, sound, and certificate helper modules only when those features are actually used.
- deferred NVDA controller DLL loading until the first spoken announcement, so launch does less filesystem work before the main menu appears.
- deferred the tray icon setup on normal launches while still creating it immediately for start-in-tray and close-to-tray flows.
- hardened main-menu and dynamic-results selection handling against stale or destroyed controls.
- improved the background-player Full screen button so keyboard activation also announces that full screen turned on.

## What's new in version 0.8.60

- fixed background-player buttons in the main menu so Space and Enter activate the focused button instead of being intercepted by player shortcuts.
- fixed the background-player Full screen button so keyboard users can start full screen from the main menu while playback continues in the background.

## What's new in version 0.8.59

- narrowed the live EQ band filters so adjacent sliders, especially 62 Hz through 4 kHz, no longer stack so broadly that 12 dB feels like a much larger boost.
- made EQ live changes less jumpy by coalescing slider updates a little longer before replacing the mpv audio filter.
- strengthened the optional clipping protection: it now applies to any positive EQ or Bass boost, adds explicit headroom before the EQ, and keeps the limiter after the EQ to reduce bass clipping and vocal ducking on dense tracks.
- kept the player audio output device as a true session choice until the player is closed, so choosing Bluetooth speakers or another device carries over to the next item.
- fixed Settings opened from the main menu while the background player is visible so focus is forced back to the settings sections instead of the background player controls.
- added a player shortcut, `P`, to preview only the marked clip between the start and end markers; it seeks to the marker start, plays the marked section, then pauses at the marker end.

## What's new in version 0.8.58

- fixed the player Full screen checkbox accessibility path: Space now uses the native checkbox behavior instead of being intercepted by custom key handlers.
- kept Enter support for the Full screen checkbox while allowing Tab and Space to continue through normal control navigation.
- kept focus on the Full screen checkbox after toggling full screen and added explicit Full screen on/off announcements, so screen readers have feedback and Tab can continue to the next player options.

## What's new in version 0.8.57

- added an optional clipping-protection setting for the Volume boost + EQ/Bass boost combination; it uses a soft limiter only and does not reintroduce automatic EQ headroom changes.
- changed channel Popular videos so it no longer sorts only the latest loaded batch. With a YouTube Data API key it loads the channel's most-viewed videos directly; without one it scans the available channel videos, hydrates view counts, sorts by all-time views, and then shows results using the existing dynamic page size.
- kept the dynamic result behavior for Popular videos: dynamic mode reveals 20 at a time, while a fixed results-limit setting shows that fixed number of top videos.

## What's new in version 0.8.56

- stabilized the equalizer after tester reports from 0.8.55: each slider now updates only its own band state and programmatic slider refreshes are ignored by the save/live-preview handlers.
- changed player and local-edit EQ processing back to deterministic Q=1 peaking filters for all ten bands, avoiding the broad octave/shelf behavior that made lows, mids, and highs feel interdependent.
- removed the automatic EQ headroom/limiter layer from the live equalizer path so changing one positive band no longer changes the whole output level or makes other bands sound like they moved.
- reduced EQ event binding back to the primary slider event, preventing duplicate wx scroll events from re-saving stale values during keyboard or screen-reader slider changes.

## What's new in version 0.8.55

- detects currently live YouTube streams from `yt-dlp` `live_status` and `is_live` metadata.
- shows currently live YouTube streams as Live stream instead of Video in search results, result details, player details, playback queue, favorites, history, and user playlists.
- keeps live streams on the normal video playback path, so they still play through mpv like other YouTube videos.
- disables saved resume-position seeking for live streams so a live broadcast starts at the live stream instead of trying to seek to an old timestamp.

## What's new in version 0.8.54

- fixed equalizer slider state so moving one band updates only that band's draft value instead of re-reading and re-saving all ten sliders at once.
- fixed player equalizer live preview to keep an independent in-memory value for every band, preventing one slider event from rewriting neighboring bands.
- changed EQ event binding from the broad wx scroll event to explicit slider and scroll-step events, reducing duplicate or misrouted slider updates from keyboard and screen-reader controls.
- changed all EQ bands back to peaking filters with explicit per-band widths, so 31 Hz through 16 kHz behave like independent graphic-EQ bands instead of broad bass/treble shelves.
- tightened the low, mid, and high band widths to reduce audible bleed between neighboring sliders while keeping low-frequency bands responsive.

## What's new in version 0.8.53

- checked and polished the whole 10-band equalizer path, not just the low bands.
- added explicit filter widths for every equalizer band from 31 Hz through 16 kHz so each slider maps to a stable, intentional audio range.
- changed the 16 kHz band to a high-shelf treble filter, matching the low-shelf handling for sub bass and making the top band more audible.
- bound equalizer sliders to both normal slider and wx scroll events so keyboard, page, arrow, mouse, and assistive-control changes all refresh the same way.
- gave mpv equalizer application a few retries when filter replacement is briefly busy, reducing cases where a slider visually moves but the active audio filter does not update.

## What's new in version 0.8.52

- fixed Previous playback so `Ctrl+PageUp` keeps using the embedded player instead of letting mpv open a separate stream window.
- strengthened the low equalizer bands: 31 Hz and 62 Hz now use wider low-shelf filters, and 125 Hz is wider, so sub bass and low bass sliders make a clearer audible change.
- made equalizer custom profiles deletable from Settings and from the player equalizer dialog.
- coalesced rapid volume-key repeats before sending them to mpv, reducing repeated volume-change crackle on sensitive audio devices.
- changed channel options to Videos, Playlists, and Popular only; Popular now fetches view counts and sorts videos by views.

## What's new in version 0.8.51

- debounced and retried equalizer live preview updates, so rapid slider changes no longer leave stale or missing mpv filters behind.
- widened the 31 Hz, 62 Hz, and 125 Hz equalizer bands so sub bass and low bass changes are audible and consistent.
- added EQ headroom and limiting when Volume boost is combined with positive EQ/Bass boost, reducing crackling and clipping.
- guarded playback startup with request generations, so fast repeated Next/Previous commands ignore stale stream-resolution results instead of starting two items.
- stopped custom equalizer preset rename focus loss from redrawing the Settings page, fixing Tab/Escape navigation after profile creation.
- capped the Default playback volume slider at 100 unless Volume boost on by default is enabled, where the 300 range remains available.
- expanded the Seek seconds setting down to 0.1 seconds and made the small seek shortcuts use that configured value.

## What's new in version 0.8.50

- changed playlist and channel context-menu downloads into an Audio/Video submenu, while keeping `Ctrl+Shift+A` and `Ctrl+Shift+D` as direct audio/video downloads.
- hid unavailable playlist and favorites actions when those lists are empty, so only valid actions are shown.
- fixed the Fullscreen checkbox so Enter toggles it reliably.
- fixed equalizer slider keyboard steps so the full 24 dB range behaves correctly, including 3 dB PageUp/PageDown movement.
- made Bass boost additive and independent from global and player equalizer settings instead of resetting them.
- kept background Next/Previous playback embedded in the existing player panel and preserved focus instead of opening a separate mpv window.
- repaired old custom F5 shortcuts by clearing them back to their default action bindings.

## What's new in version 0.8.49

- fixed dynamic result playback so pressing Enter on a result keeps the exact selected video even if the next 20 results finish loading at the same moment.
- stabilized equalizer sliders so their screen-reader names stay consistent, value updates are quieter, and the player equalizer now has the same 6/12/18/24 dB range selector as the global equalizer.
- fixed the player Fullscreen checkbox so Space toggles the checkbox instead of play/pause, and Enter also toggles fullscreen.
- made player Bass boost session-based, so it stays on for the next item until the user turns it off.
- expanded stream URL cache duration choices up to 7 days and permanent-until-YouTube-expires.
- kept focus in the current list when Next/Previous is triggered from results while playback continues in the background.

## What's new in version 0.8.48

- added a fast YouTube stream URL cache and next-item prefetch option so already resolved videos can start faster without storing full media files.
- added Settings controls for stream URL cache duration, next-item prefetch, and conversion-finished popups.
- added folder conversion progress windows with current file, completed count, and remaining count.
- added safe converter output choices for creating new files/folders or replacing originals only after a successful conversion.
- improved folder conversion to scan subfolders and preserve folder structure in the converted output.
- added progress windows for playlist, channel, and larger batch downloads while keeping small single-video batches lightweight.

## What's new in version 0.8.47

- fixed Player Shift+Tab navigation by using the real screen tab order and only falling back manually when focus is on the Player panel itself.
- fixed background-player tab order from the main menu so Shift+Tab from Player returns to Open, and Tab from Open reaches Player in the expected order.
- limited full player shortcuts to the Player area and player buttons so result lists keep normal arrow-key navigation; Previous and Next remain available while playback continues in the background.

## What's new in version 0.8.46

- fixed Shift+Tab from the Player area so keyboard focus can move backward to results or Back controls instead of getting stuck.
- added wx navigation-key handling for player controls so NVDA and Windows focus traversal use the same player tab order.
- preserved the existing forward Tab order from Player to Previous, Play/Pause, Next, and the rest of the player controls.

## What's new in version 0.8.45

- added Open channel for YouTube videos in result, player, favorites, history, and user playlist context menus when the video has channel metadata.
- added a configurable Open channel shortcut, defaulting to Ctrl+Shift+O.
- Open channel loads the video's channel videos inside ApricotPlayer instead of opening a browser.

## What's new in version 0.8.44

- fixed classic player navigation when background playback is disabled: Tab from Player now goes to Previous, Play/Pause, Next, and the rest of the player controls instead of jumping into the Back buttons.
- made Shift+Tab in the classic player return to Back to results / Back to main menu without trapping focus on Player.
- changed Escape in fullscreen player mode so it exits fullscreen, unchecks the Fullscreen checkbox, keeps playback running, and returns focus to Player.

## What's new in version 0.8.43

- fixed Shift+Tab from the background Player section after returning to the main menu, search results, folder results, or other lists, so focus goes back to the active list instead of forcing users to tab around the whole window.
- made background player reverse navigation choose the current screen's primary control explicitly for steadier NVDA navigation.

## What's new in version 0.8.42

- fixed player shortcuts while background playback is visible in the main menu, so Space, seek, volume, speed, pitch, details, equalizer, and other player keys work when focus is on the background Player section.
- centralized player shortcut handling so the same key behavior is used in the full player and in the background player controls.

## What's new in version 0.8.41

- announced the title of the newly playing video, podcast episode, queue item, or local file when using player Next or Previous.
- preserved quiet automatic playback transitions; the new title announcement is only for manual Next/Previous actions.

## What's new in version 0.8.40

- polished several stability issues before the next larger feature cycle
- reduced screen-reader focus churn in the main menu by updating queue/download labels in place instead of rebuilding the whole menu
- fixed player Escape handling on player buttons by preserving navigation controls in the internal focus map
- made fullscreen launch obey the current fullscreen override state, so leaving fullscreen to results does not get undone by the saved default fullscreen setting
- made settings section rendering smoother by freezing the settings panel while controls are rebuilt
- ignored stale custom player paths when the configured player no longer exists, falling back to the bundled mpv instead
- kept playback queue counts in the main menu synchronized when items are added, removed, played, or consumed automatically
- reduced unnecessary play/pause button relayouts by only updating labels when the state actually changes

## What's new in version 0.8.38

- fixed Back to results from fullscreen so focus returns to the results list instead of landing back on Player
- made fullscreen exit retry the results focus briefly after leaving fullscreen, avoiding Windows focus timing quirks

## What's new in version 0.8.37

- fixed classic player navigation when background playback is disabled: Shift+Tab from Player now reaches the player navigation buttons instead of getting stuck
- added Back to main menu beside Back to results in the classic non-background player
- removed the Close button from the classic non-background player because Back to results and Back to main menu already close the player in that mode
- made the embedded Player panel request Tab keys more explicitly for better screen-reader keyboard navigation

## What's new in version 0.8.36

- removed the redundant Open player screen button from the background player section
- made the background player section start directly with the focusable Player target, followed by Previous, Play/Pause, Next, and the rest of the controls
- kept explicit Tab and Shift+Tab handling so the Player target remains reachable in both directions on Search, main menu, and other screens

## What's new in version 0.8.35

- fixed background player keyboard navigation so Tab and Shift+Tab move consistently through Open player screen, Player, Previous, Play/Pause, Next, and the rest of the controls on Search, main menu, and other screens
- made the background Player target reachable in both directions even when wx reuses the embedded video panel after returning from results or opening Search again

## What's new in version 0.8.34

- fixed the background player tab order in the main menu and other screens so Open player screen is followed by the focusable Player section, then Previous, Play/Pause, Next, and the rest of the controls
- restored the screen-reader named Player target when returning from results to the main menu while background playback is still active
- kept the existing fullscreen player behavior from 0.8.33, with Fullscreen as a checkbox and Back to results used to leave fullscreen

## What's new in version 0.8.33

- fixed fullscreen player tab order so Shift+Tab from Previous now lands on Player before Back to results, while Tab from Player still lands on Previous
- changed Fullscreen in the player from a button to a checkbox that reflects whether the current player screen is fullscreen
- kept fullscreen mode hiding results; Back to results exits fullscreen and restores the normal player-with-results layout
- focused Player earlier while constructing the player screen, reducing the slight delay before screen readers announce Player after starting playback

## What's new in version 0.8.32

- fixed Tab getting trapped on the Player panel by explicitly routing Tab from results to Player and Tab from Player to Previous, Play/Pause, Next, and the rest of the controls
- kept Shift+Tab from Player going back to the visible results list when background playback is enabled
- confirmed fullscreen player mode hides the embedded result list and uses Back to results to return to the normal player-with-results layout

## What's new in version 0.8.31

- made the combined player-plus-results layout conditional on Enable background playback, so disabling background playback restores the classic player with Back to results and no persistent result list
- made the embedded Player panel keyboard-focusable, so Tab from the visible results list lands on Player and Tab from Player lands on Previous, Play/Pause, Next, and the other controls
- kept background playback navigation cleaner: visible results remain available when returning to the player from the menu, while main-menu/background controls do not show a stale results list

## What's new in version 0.8.30

- changed the normal player tab order so Shift+Tab from Player goes to the visible results list, while Tab from Player goes directly to Previous, Play/Pause, Next, and the rest of the player controls
- kept Back to main menu before the results list, so Shift+Tab from Player reaches results first and then the main navigation button

## What's new in version 0.8.29

- refined Escape behavior in the new player-with-results layout: Escape on the Player itself still stops playback and returns to results, while Escape from the visible result list goes to the main menu and keeps playback available in the Player section
- made the player Back to main menu button behave like manual navigation, keeping playback alive and tabbable from the main menu

## What's new in version 0.8.28

- changed the player layout so normal playback keeps the result list visible and tabbable under the player; pressing Enter on another result starts it and returns focus to Player
- removed the Back to results button from normal player mode; it now appears only in fullscreen mode, where results are intentionally hidden
- made the fullscreen player button switch to a true fullscreen player view, with Back to results returning to the normal player-with-results layout
- reduced screen reader spam from result metadata refreshes by deferring updates to the currently focused result row until focus moves away

## What's new in version 0.8.27

- fixed dynamic loading so search, channel, playlist, and trending result lists append new items without moving screen reader focus to the newly loaded rows
- cached Play from folder results in memory for the current session, so returning from a local file no longer rescans huge folders
- made auto-created folder playback queues clear when the folder playback session is stopped with Escape, while manual/background navigation keeps the queue available
- preserved the current playback volume when moving to another result in the same player session, but still resets to the configured default after the player is fully closed
- removed the embedded player tooltip that could cause repeated "Player tool tip" announcements, and added a fullscreen player button
- reopened Settings at the first section after returning from the menu instead of remembering the previous section focus

## What's new in version 0.8.26

- reverted updater download chunks to the previous 512 KB size because the larger 4 MB chunks were slower for some testers

## What's new in version 0.8.25

- hardened settings loading so a corrupted, empty, or partially unreadable settings file can no longer silently fall back to defaults and later overwrite user preferences
- if the main settings file cannot be read but the backup can, ApricotPlayer now restores from the backup instead of resetting settings
- automatic background saves are blocked when settings could not be loaded safely, protecting update interval, equalizer, volume, playback, and download settings after updates

## What's new in version 0.8.24

- improved updater downloads with larger chunks, throttled progress updates, a longer download timeout, and clearer updater log timing
- updater now cleans up failed update downloads before showing the error
- installer desktop shortcut is selected by default again, so ApricotPlayer appears on the Desktop after install/update
- playback queue now disappears from the main menu when the last queued item is removed
- background playback navigation no longer resets the same window title repeatedly, reducing repeated screen reader announcements

## What's new in version 0.8.23

- added Ctrl+Shift+U for unsubscribing from a channel
- added unsubscribe actions to relevant result, channel, subscription, favorite, history, and player context menus
- made channel subscription detection more robust for video results by falling back to channel IDs and normalized channel URLs

## What's new in version 0.8.22

- fixed F7 video details by recognizing both wx and raw Windows F-key codes, including after resetting keyboard shortcuts to defaults
- F7 now opens the full player first when used from background player controls, then focuses video details
- reduced repeated screen reader announcements by avoiding repeated focus resets when ApricotPlayer is already focused on the correct list or control

## What's new in version 0.8.21

- sped up video downloads without changing the already-fast audio download path
- video downloads now use more parallel fragment downloads for DASH/HLS streams and chunked HTTP downloading for large video files
- tightened MP4 format selection so ApricotPlayer prefers already-combined MP4 video+audio files before falling back to slower video/audio merging

## What's new in version 0.8.20

- fixed Play from folder so selected folders show their local audio/video files instead of leaving the list at "No search results"
- added local-file-specific result rows and Space details so folder playback no longer expects YouTube-only metadata like views or upload age
- made folder scanning more resilient by skipping unreadable files or subfolders instead of failing the whole folder

## What's new in version 0.8.19

- fixed Trending return behavior so playback returns to the Trending screen with its country/category controls instead of the normal Search screen
- added Subscribe to channel to the player context menu; Ctrl+Shift+S also works from the player for the currently playing YouTube video
- repaired old shortcut settings where F7 could incorrectly announce volume; defaults are now V for volume and F7 for video details
- kept the faster MP4 video download preference from 0.8.18 so video downloads try a single MP4 stream before falling back to slower merge formats

## What's new in version 0.8.18

- restored Escape in the player to stop playback and return to the previous screen, while manual Back buttons can still keep background playback alive
- added result-list shortcuts for favorites and playlists: Ctrl+F, Ctrl+Shift+F, Ctrl+P, and Ctrl+Shift+P
- Space on a focused search result now announces the result details without opening a dialog
- playlist results now include the video count when YouTube provides it
- made MP4 video downloads prefer a single MP4 file before falling back to separate video/audio streams, which can make many video downloads start and finish faster
- reduced background result metadata hydration work to lower CPU, RAM, and screen reader churn while browsing results

## What's new in version 0.8.17

- moved confusing Cookies and network troubleshooting controls behind a new advanced network/download settings checkbox
- clarified the User-Agent, FFmpeg path, concurrent fragments, and download speed limit labels for screen readers
- strengthened the update relaunch focus path so ApricotPlayer brings itself to the foreground and focuses the main menu after updating

## What's new in version 0.8.16

- fixed background playback so Escape returns one level back without stopping playback when background playback is enabled
- changed the background/player Play button to switch between Play and Pause based on the real player state
- renamed the background player controls to Open player screen and Close, and removed the extra background-playback announcement
- rebuilt Play from folder so it opens a folder as an accessible media list instead of immediately starting the first file
- added Play folder, Shuffle folder, Add folder to queue, and queue Move up/Move down context menu actions
- changed default player shortcuts so V announces the current volume and F7 opens video details

## What's new in version 0.8.15

- fixed the Default playback volume slider accessibility in Settings so screen readers hear the setting name and actual 0-300 volume value instead of a confusing slider/percentage mix
- removed native slider tick labels from that control and reused ApricotPlayer's custom accessible slider value reporting

## What's new in version 0.8.14

- avoided very slow YouTube playback retries when a manually imported cookies file does not contain usable YouTube/Google login cookies
- for YouTube playback, ApricotPlayer now only uses a cookies file as an auth fallback if it can detect real login cookies in that file
- added a faster progressive-format retry before the heavier YouTube JS fallback, so ordinary videos that only miss the first format choice can start faster
- kept age-restricted videos working with valid login cookies while failing quickly and clearly when the selected cookies file cannot authenticate YouTube

## What's new in version 0.8.13

- fixed playback for YouTube videos where valid imported cookies work but the first fast format choice fails with `Requested format is not available`
- ApricotPlayer now keeps the fast playback path first, then retries the slower YouTube JS fallback only after that specific format/auth failure
- verified the reported test video resolves successfully with imported cookies even when the age-restricted fallback setting is off

## What's new in version 0.8.12

- fixed manual browser-extension cookie imports by converting selected Netscape/Mozilla, JSON, and Cookie-header exports into ApricotPlayer's own normalized `cookies.txt`
- existing manually selected cookie files are now normalized automatically the next time playback/downloads need cookies, so older settings are repaired without reselecting the file
- added an optional Browser User-Agent setting for cookies files, matching yt-dlp guidance for sites that require cookies and browser-like headers to come from the same session
- improved manual cookie warnings so ApricotPlayer keeps usable cookies but clearly reports when the imported file has no YouTube/Google login cookies

## What's new in version 0.8.11

- fixed manual `cookies.txt` playback retry so ApricotPlayer uses a selected cookies file after a YouTube login/bot error even when age-restricted fallback support is turned off
- improved Chrome-family cookie export by trying a second DevTools mode when the normal `yt-dlp` browser-cookie path fails with DPAPI decrypt errors
- added diagnostics for manually selected cookies files, including a warning when the file does not contain usable YouTube login cookies
- added Reset all settings in General and Reset settings for the current section in every Settings section
- made settings saves atomic and backed up the previous settings file, reducing the chance that an update or forced close resets playback/equalizer settings
- throttled repeated automatic cookie-refresh speech after a failed cookie refresh, so the same broken browser-cookie state does not keep speaking continuously
- made Repeat take priority over autoplay/queue at end of playback and restart the same item if mpv reaches EOF while repeat is enabled

## What's new in version 0.8.10

- removed ApricotPlayer's remaining app-side cap in dynamic result mode, so YouTube searches, channels, playlists, and local folder lists keep loading 20 more items until the source stops returning results
- kept previously loaded dynamic results in the list while loading more, so arrowing back up still reaches everything already fetched
- added a tabbable Add to playlist button to the full player and background Player section, making it easier to add the currently playing YouTube item, podcast episode, or local file to an Apricot playlist
- added Add to playlist to podcast episode context menus and made player Add to playlist prefer the currently playing item instead of any unrelated multi-selected download queue
- user playlist downloads now skip local files instead of trying to pass already-local media paths through `yt-dlp`
- rechecked folder conversion behavior so failed files are counted and skipped without stopping the whole folder job

## What's new in version 0.8.9

- added a Playback setting to turn the “Playback finished” screen-reader announcement on or off
- kept the end-of-playback state internally even when the announcement is disabled, so Space can still restart from the end correctly
- migrated existing old default equalizer shortcuts from `G` to `F4`, matching the current player shortcut layout
- rechecked the new player hotkeys so repeat (`R`), shuffle (`Shift+S`), bass boost (`F3`), and equalizer (`F4`) are all listed in Keyboard shortcuts and toggle cleanly

## What's new in version 0.8.8

- fixed background playback navigation so global shortcuts and manual navigation keep the active player alive when background playback is enabled, while Escape from the full player still stops playback and returns to the previous results
- added player context-menu actions for download audio/video, add to favorites, add/remove playback queue, add to playlist, open in browser, copy URL, copy stream URL, output devices, equalizer, and close player
- added player hotkeys for `F3` bass boost, `F4` equalizer, `R` repeat, and `Shift+S` shuffle; these are available in Keyboard shortcuts settings
- made whole-folder local playback available from the main menu, with dynamic browsing of folder items and shuffle support across local files
- preserved user-adjusted session volume, volume boost, bass boost, and player equalizer state when the next track starts, instead of resetting to the default volume every time
- raised dynamic YouTube search/channel/playlist loading beyond the old 250-result ceiling, while keeping official Trending from falling back to fake search results
- hardened folder conversion so one failed file no longer stops the whole folder conversion; ApricotPlayer reports converted and failed counts
- improved Chrome-family cookie diagnostics for DPAPI/decode failures and kept the DevTools fallback path available for profiles where normal `yt-dlp` cookie extraction fails
- expanded Trending country choices for testers who enable Trending in Settings

## What's new in version 0.8.7

- added a Library setting to show or hide Trending in the main menu; Trending is off by default for new and existing settings files until enabled
- fixed Trending error handling so an unavailable official feed or missing API key returns to the main menu instead of repeatedly showing the same dialog
- changed Escape from the full player to return to the previous results screen and stop playback, while background playback still continues when browsing from the persistent Player section
- improved Chrome and Chromium cookie export by trying the DevTools fallback when normal extraction finds cookies but no usable YouTube login cookies
- changed the "Open YouTube in selected profile" action to use normal browser profile launching, avoiding the profile-lock error that could appear with Chrome-family browsers
- expanded Windows "Play with ApricotPlayer" registration to more audio and video extensions and per-extension context-menu entries
- tightened global equalizer live preview so Settings sliders affect currently playing background audio immediately and local/player EQ no longer blocks that preview
- saved custom equalizer profile names more consistently so renamed profiles appear by name in the preset combo box

## What's new in version 0.8.6

- replaced the Trending screen's `#trending` search with an official YouTube most-popular feed path using the YouTube Data API when configured
- added a YouTube Data API key setting under Cookies and network; without a key, ApricotPlayer tries public YouTube chart/explore feeds and otherwise explains that official trending is unavailable
- added an `Enable background playback` Playback setting, off by default, so the classic Escape-stops-player behavior remains available
- when background playback is enabled, leaving the player keeps playback alive and exposes an accessible Player section with playback, queue, output-device, equalizer, copy-link, open-player, and close controls
- fixed `Ctrl+Space` after shortcut navigation so it can control the active background player instead of reporting that the player is missing
- improved equalizer state handling so global EQ and player-only EQ do not overwrite each other, and global EQ sliders can live-preview while audio is playing when no player EQ override is active
- added dynamic equalizer profiles and a player equalizer action to save the current local EQ as a global preset
- improved browser cookie export diagnostics for Chrome and other browsers by reporting tried profiles, cookie counts, YouTube-cookie counts, and whether login cookies were found
- added a Settings action to open YouTube in the selected browser/profile before exporting cookies

## What's new in version 0.8.5

- playback can continue in the background when leaving the player with Escape, Back, or Back to results
- the main menu now shows a Player section while something is playing, with Play, Previous, Next, Open player, Copy link, and Close player controls
- added configurable `Ctrl+Space` background play/pause and `Ctrl+L` copy-link support from search results
- renamed the embedded player panel for screen readers so it is announced as Player instead of an unnamed panel
- fixed the default-volume startup path so playback starts at the chosen volume without jumping to 100 first
- stabilized player equalizer behavior so opening the player equalizer no longer overwrites the session EQ, and global EQ changes only apply when no player-only EQ is active
- channel results now open an accessible channel options picker for videos, playlists, home, or popular videos
- added a Trending main-menu item with country and category filters

## What's new in version 0.8.4

- app update checks now trust GitHub's `/releases/latest` endpoint first, so new versions show up even when GitHub's public releases list cache is behind
- the full releases list is still used for cumulative changelogs and now merges the latest release when that list omits it

## What's new in version 0.8.3

- converter default output filenames now keep the original base name and only change the extension, for example `song.mp3` to `song.mp4`
- converter collision handling now uses numbered names such as `song (2).mp3` instead of adding `converted`
- confirmed folder conversion saves directly into the folder selected by the user and does not create an extra nested folder

## What's new in version 0.8.2

- changed the Windows startup setting label to "Start ApricotPlayer at Windows startup"
- Windows startup registration now launches the normal ApricotPlayer window instead of starting hidden in the system tray
- the setting remains off by default for new users
- the separate system tray close behavior is still available through the existing tray setting

## What's new in version 0.8.1

- renamed the Playback setting from "Resume videos where you left off" to "Resume where you left off" because resume now applies to videos, podcasts, and local files
- changed File converter and Folder converter path labels to "File to convert" and "Folder to convert"
- changed the converter's detected format display into a focused read-only field for clearer screen-reader navigation
- renamed the converter output format combo box label to "Convert to"

## What's new in version 0.8

- added Playback settings for default volume and volume boost on by default, so playback can start at a chosen volume and optionally allow volume above 100% without pressing F2 first
- added File converter and Folder converter menu items directly above Settings in the main menu
- added accessible converter dialogs with path entry, Browse buttons, detected input format, output format combo boxes, Save As/folder output prompts, and screen-reader status messages
- added broad FFmpeg-powered audio and video conversion targets, including MP3, M4A/AAC, WAV, FLAC, OGG, Opus, WMA, AIFF, ALAC, AC3, MP2, MP4, MKV, WebM, MOV, AVI, WMV, MPEG/MPG, FLV, 3GP, OGV, TS, M2TS, and ASF
- added audio-to-video conversion choices for either a dark background or a selected image background
- verified the existing Save As download setting still uses a file dialog for single downloads and a folder dialog for playlists, channels, and batch downloads
- confirmed the default keyboard shortcuts include `G` for the player equalizer and `E` for local-file edit mode

## What's new in version 0.7.2

- added a General setting to start ApricotPlayer with Windows directly in the system tray
- added `--start-in-tray` startup handling so tray autostart does not steal focus or open a second process
- made tray startup skip interactive startup prompts, such as media association and missing audio-device dialogs
- startup registration now uses the current executable path and keeps the Windows Run entry in sync when settings are saved
- reviewed the 0.7.1 queue, clip export, local edit mode, update, and startup paths and kept the YouTube fast path unchanged

## What's new in version 0.7.1

- changed marked clip export to use the normal download shortcuts: `Ctrl+Shift+A` exports the marked section as audio and `Ctrl+Shift+D` exports it as video
- clip start and end marker shortcuts now toggle their marker off when pressed again
- added local-file edit mode with `E`, edited-copy saving with `Ctrl+S`, and original-file replacement with `Ctrl+R`
- moved the default player equalizer shortcut from `E` to `G` so `E` can control local-file edit mode
- added a playback queue with player button, global open shortcut, add/remove shortcuts, context-menu actions, Enter-to-play, and automatic next-item playback
- added an optional setting to ask where each download should be saved, using Save As for single downloads and folder selection for channel or playlist downloads
- added first-run language selection for new users
- kept YouTube playback on the fast path first; restricted-video cookies and EJS fallback still run only after a relevant playback failure

## What's new in version 0.7

- added local media file playback support so ApricotPlayer can open audio and video files passed from Windows file associations
- added installer registration for common audio/video file types and a Settings button that opens Windows Default apps
- added Play from folder for opening local audio/video files directly from ApricotPlayer
- added an accessible 10-band equalizer with global Settings controls, genre/sound presets, three custom profiles, and a player-only Equalizer dialog
- expanded the equalizer into an enabled/disabled global Settings section with preset selection and editable preset gains
- updated the player Equalizer dialog with the same accessible preset-and-slider workflow for per-video EQ changes
- added player clip markers with physical `LeftBracket` and `RightBracket` keys, plus `Ctrl+S` to export only the marked section with FFmpeg
- added configurable keyboard shortcuts for Play from folder, clip markers, and clip export
- added a Playback setting to announce Playing or Paused when pressing Space in the player
- added a player Bass boost checkbox that applies the bass boost EQ preset only for the current playback session
- added a first-run repair prompt that registers ApricotPlayer as a Windows media player option if an update did not create the media association registry entries
- improved equalizer slider names so screen readers hear the frequency range and purpose, not only a numeric slider value
- fixed focus after enabling the global equalizer in Settings so the screen reader does not land on a blank panel
- changed the default clip marker shortcuts to physical `LeftBracket` and `RightBracket` keys, so they work across keyboard layouts such as Slovenian where those keys type different characters

## What's new in version 0.6.14.7

- fixed main-menu shortcut handling so pressing modifier keys such as Ctrl+Shift by themselves no longer opens a "Select an item" dialog
- made `Ctrl+Shift+V` open the Notification center from the main menu reliably
- added configurable global shortcuts for main menu navigation, including Search, Direct link, Favorites, Playlists, Subscriptions, Current downloads, History, Podcasts/RSS, Settings, and Main menu
- prevented item-only shortcuts such as download and subscribe from firing in the main menu when no media item is selected

## What's new in version 0.6.14.6

- changed the new age-restricted YouTube fallback setting to off by default for new users and older settings files that do not have this option yet
- kept normal playback on the fast first attempt; cookies and EJS still run only after a relevant playback failure and only when the setting is enabled

## What's new in version 0.6.14.5

- restored fast YouTube playback startup by keeping Node/EJS challenge solving out of the normal first playback attempt
- added a Playback setting for age-restricted YouTube video support, which only uses the slower cookies/EJS fallback when the fast path fails with a sign-in, age, or format challenge
- kept age-restricted videos working while avoiding the extra yt-dlp setup cost for ordinary videos

## What's new in version 0.6.14.4

- bundled EJS challenge solver support and a Node runtime so age-restricted YouTube videos can resolve playable formats instead of failing with `Requested format is not available`
- disabled external/default yt-dlp plugins inside ApricotPlayer so obsolete local plugins such as `yt-dlp-youtube-oauth2` cannot hijack YouTube extraction
- added a playback recovery prompt that asks whether to refresh YouTube cookies when a sign-in, bot-check, or stale-cookie error still reaches the player
- made manual browser cookie export accept valid Google/YouTube login cookies from the DevTools fallback and reject cookie files that do not contain login cookies

## What's new in version 0.6.14.3

- added a Chromium DevTools cookie export fallback for Brave, Chrome, Edge, Chromium, Opera, and Vivaldi when Windows refuses to copy the locked cookie database
- ApricotPlayer now launches the selected Chromium browser headlessly with the selected profile, reads cookies through the browser itself, writes `cookies.txt`, and closes the temporary browser process
- this fixes the repeated `Could not copy Chrome cookie database` failure that blocked age-restricted videos and YouTube bot-check retries when `yt-dlp` could not copy the SQLite cookie file

## What's new in version 0.6.14.2

- fixed browser cookie export for Brave, Chrome, Edge, Firefox, Chromium, Opera, and Vivaldi after yt-dlp started calling the logger `info` method during cookie extraction
- kept the automatic cookie refresh flow compatible with newer yt-dlp cookie extraction logging, so the error no longer stops every browser profile before cookies are checked
- improved YouTube login cookie detection by recognizing additional current Google and YouTube auth cookie names

## What's new in version 0.6.14.1

- fixed `Ctrl+Shift+V` from the main menu by registering the Notification center shortcut in the global accelerator table
- prevented plain single-letter global shortcuts from firing while the user is typing in editable text fields, so a custom `N` shortcut can no longer steal focus from the search box
- kept the Notification center shortcut configurable while avoiding unsafe no-modifier accelerators for editable fields

## What's new in version 0.6.14

- the window title now changes to the currently playing video, podcast, or episode title followed by ApricotPlayer, so Alt+Tab announces the active media
- added an App update check interval setting with 30-minute, 1-hour, 2-hour, 3-hour, 6-hour, 12-hour, and 24-hour choices
- added an optional Windows notification when a background update check finds a new ApricotPlayer release
- background update checks now store the pending release and show an `Update available` item at the top of the main menu
- opening that main-menu item shows the usual changelog and Update now / Skip this version buttons

## What's new in version 0.6.13

- changed `Ctrl+Shift+V` to open the Notification center directly from anywhere in the app
- updated the keyboard shortcut label from "New subscription videos" to "Notification center"
- kept the selected notification stable when the notification list refreshes
- made the shortcut behave the same way inside the subscriptions screen instead of opening only one channel's saved new videos

## What's new in version 0.6.12

- restored fast playback startup by keeping the first yt-dlp playback/download attempt cookie-free
- changed cookie handling to retry with cached `cookies.txt` only after YouTube reports sign-in or bot-confirmation errors
- kept the automatic browser profile export/repair flow as a second fallback only when cached cookies are missing or stale
- removed automatic cookie injection from download options so normal downloads also stay on the fast path

## What's new in version 0.6.11

- rebuilt browser cookie handling so normal playback and downloads use ApricotPlayer's cached `cookies.txt` instead of repeatedly reading locked Brave/Chrome/Edge databases
- added automatic cookie repair: when YouTube asks for sign-in or bot confirmation, ApricotPlayer can refresh cookies from the selected browser, then retry the same playback or download
- added browser profile discovery for cookie export, including Brave, Chrome, Edge, Chromium, Vivaldi, Opera, and Firefox profiles
- added a Browser profile setting with an Auto option that tries discovered profiles and chooses the one with usable YouTube/Google cookies
- made cookie export errors preserve profile-level details so failures no longer collapse into the unhelpful "failed to load cookies" message
- clarified that `Choose cookies.txt file` is for a Netscape-format exported cookies file, not Brave's internal encrypted cookie database

## What's new in version 0.6.10.1

- fixed startup focus so opening ApricotPlayer from a closed state explicitly brings the app window to the foreground and focuses the main menu
- reused the same foreground/focus activation path when restoring an already running app from the system tray or a second launch

## What's new in version 0.6.10

- fixed updater version comparison so future hotfix versions such as 0.6.10.1 are detected as newer releases
- used a three-part 0.6.10 release number so older 0.6.9-era updaters can detect this update
- changed release lookup to choose the highest public version from GitHub releases instead of trusting only GitHub's latest marker
- hardened updater downloads with exact asset-name checks, trusted HTTPS source checks, asset size checks, SHA-256 verification when GitHub or PyPI publishes a digest, and safe ZIP path validation
- made yt-dlp component updates verify PyPI wheel SHA-256 digests and extract wheels with path traversal protection
- optimized Settings section navigation with debounced section rendering so arrowing through sections does not rebuild the right side for every key press
- rebuilt Keyboard shortcuts settings into one action list and one capture field, reducing dozens of edit fields to two focusable controls for smoother screen reader navigation

## What's new in version 0.6.9.1

- added a 30-minute option for automatic subscription checks and podcast/RSS refreshes
- optimized the Settings Playback section so audio output devices are refreshed in the background instead of blocking section navigation
- made second-launch activation checks more frequent so an already running ApricotPlayer window can restore focus more quickly after the launcher process signals it

## What's new in version 0.6.9

- fixed History so pressing Enter on a history item reliably opens playback for videos, podcast episodes, and other saved playable links
- changed second-launch behavior so starting ApricotPlayer again restores and focuses the existing window, including when it is hidden in the system tray, instead of opening an "already open" dialog
- added a Settings entry for whether ApricotPlayer shows a Windows notification when it is sent to the system tray
- added Settings to the tray icon menu and made tray icon activation restore ApricotPlayer directly while keeping the existing Show ApricotPlayer menu item
- added a Repeat checkbox to the player and made Space/Play restart a finished video from the beginning when automatic next playback is off
- changed new-user playback defaults to MPV pitch control and Rubberband speed audio processing as the recommended quality settings
- reduced startup disk writes by only creating the settings file on first run instead of rewriting it on every launch

## What's new in version 0.6.8

- added stale-search protection so older background searches, channel loads, and playlist loads cannot replace newer results after the user has already moved on
- optimized list refreshes across playlists, subscriptions, history, notifications, RSS, favorites, current downloads, and result labels so unchanged lists are not rebuilt
- preserved list selection more carefully during background subscription and RSS updates
- refreshed RSS episode lists in place instead of rebuilding the whole screen when feeds update in the background
- reset result metadata hydration per new search or collection load so failed upload-age lookups can be retried on later searches
- limited UI queue processing per timer tick to keep the interface responsive during bursts of download/status events
- kept the focus-preserving upload-age metadata behavior from 0.6.7 while applying it through the broader list refresh cleanup

## What's new in version 0.6.7

- fixed single video downloads so they always use single-video mode instead of accidentally following playlist URLs when playlist ordering is enabled
- dynamic search metadata hydration now covers newly loaded result pages, so later batches can also update from Uploaded unknown to Uploaded X ago
- optimized result metadata hydration by reusing one yt-dlp session per visible batch
- fixed search result metadata updates so NVDA focus no longer jumps from result to result while upload ages are being filled in
- optimized download progress updates by throttling duplicate UI events while still reporting meaningful progress changes
- subscription checks and RSS refreshes now continue when one channel or feed fails, instead of stopping the whole refresh
- cached the HTTPS certificate context used for GitHub, podcast, RSS, and update requests
- cleaned up static-analysis warnings in the Python sources

## What's new in version 0.6.6

- fixed installer updater relaunch so ApricotPlayer restarts from the newly installed executable immediately after an update
- removed the installer restart-applications flag from the updater flow to prevent Windows/Inno from reopening the old running version before ApricotPlayer can relaunch itself
- the updater now re-checks the installed executable location after setup completes and stops stale ApricotPlayer processes in known install folders before starting the new version
- the installer now also launches the updated app at the end of silent update installs, with a short duplicate-launch suppression window so older updater scripts do not show an extra already-open dialog

## What's new in version 0.6.5

- changed the default audio output device setting from a text field to an accessible combo box populated from mpv's detected Windows audio devices
- saved default audio devices now persist across restarts, while devices chosen from the player are session-only and reset when ApricotPlayer closes
- ApricotPlayer now warns on startup when a saved default audio output device is no longer available and lets the user choose a new default
- removed the visible Player command/path setting from Settings
- added a configurable Direct link Enter action for play, audio download, video download, or direct media URL copy
- rebuilt the player screen so Tab navigation reaches the player buttons, the video panel, and video details controls while player shortcuts still work globally
- added a Show video details button, Copy details button, and improved Escape behavior for temporary versus default-visible details
- added a Show video details by default setting
- added selectable playback-speed audio processing modes, including tuned scaletempo2, mpv default, classic scaletempo, and Rubberband
- search results now hydrate missing YouTube metadata in the background so upload age can update from Uploaded unknown to Uploaded X ago when yt-dlp can resolve it

## What's new in version 0.6.4

- added optional playback cache and resume playback so videos can reopen from the last watched position
- added a player output-device picker on `O`, plus a default audio output device setting for the whole app
- added Previous and Next player buttons and configurable shortcuts
- added user-created playlists with create, open, add, remove, play, and download actions
- added a Direct link screen for playing or downloading any `yt-dlp` supported URL
- added direct media URL copying with `Ctrl+D` for use in VLC or other external players
- added a Notification center for saved subscription and podcast notifications, with Enter playback
- new video and podcast notifications now include the actual video or episode title
- update dialogs now include cumulative release notes for every version newer than the user's installed version
- search rows now keep the `uploaded ... ago` format and show an accessible unknown-upload fallback when YouTube does not provide an upload timestamp
- fixed installer cleanup for old lowercase desktop shortcut names

## What's new in version 0.6.3

- added duplicate shortcut protection in Settings so a newly captured keyboard shortcut cannot silently replace another action's shortcut
- changed new public default settings to use `Downloads\ApricotPlayer` as the download root, with YouTube downloads routed to `music` and podcast episodes routed to `podcasts`
- channel and playlist downloads now create their folders inside the music download area, while podcast feeds and episodes use a podcast-named folder inside the podcasts area
- added download-entire-feed actions for podcast/RSS feeds and episode-list screens
- podcast episodes can now be queued for audio download with the same queue/download flow used by videos
- changed the default New subscription videos shortcut to `N` so public default shortcuts are unique

## What's new in version 0.6.2

- fixed installer updates so ApricotPlayer installs into the exact running installation folder, closes old ApricotPlayer processes from that folder before setup runs, and logs the installed executable that was written
- choosing Update now clears any previously skipped update version, and old skipped-version flags are cleaned up after a newer app version is installed
- added single-instance protection so launching ApricotPlayer again while it is already open or hidden in the system tray shows an Already open message instead of starting a second copy
- Enter now opens podcast/RSS feeds and podcast search results from their lists, and Enter starts playback from episode lists even when the global shortcut handler receives the key first
- numpad Enter now works the same as Enter for configurable open actions

## What's new in version 0.6.1

- added podcast search using the Apple Podcasts directory through the iTunes Search API, while direct RSS and Atom feed URLs remain supported
- added a Podcasts and RSS settings section for provider, country, result limit, maximum episodes per feed, startup refresh, and automatic refresh interval
- added settings to hide Podcasts and RSS feeds or History from the main menu
- removed Choose download folder from the main menu because the same control already exists in Settings
- History can now be disabled so new played/downloaded items are not recorded while it is off

## What's new in version 0.6.0

- added a Podcasts and RSS feeds screen to manage podcast/RSS subscriptions outside YouTube
- feed lists support keyboard and context menu actions for add, refresh, open, copy URL, and remove
- feed item lists support play, audio download, copy URL, and open episode page actions
- RSS and Atom parsing is built in with support for podcast enclosures, Atom enclosure links, publication dates, descriptions, and durations when available
- Escape returns from a playing podcast episode back to the feed item list, matching the accessible navigation model used elsewhere
- added per-user RSS feed storage in `%APPDATA%\ApricotPlayer\rss_feeds.json`

## What's new in version 0.5.3

- added a Notifications settings section with a master Windows notifications switch plus separate toggles for subscription and completed-download notifications
- completed audio, video, batch, playlist, and channel downloads can now send Windows notification center messages when ApricotPlayer is not focused, while in-app popup behavior is preserved when the app is focused
- rebuilt Keyboard shortcuts editing so focusing a shortcut field and pressing a key combination captures it directly; Tab and Shift+Tab still move between fields
- translated the new notification and shortcut-capture text across the supported language packs

## What's new in version 0.5.2

- verified the GitHub repository is public and simplified the updater to use the public ApricotPlayer release endpoints without user-facing owner, repo, or private token settings
- added a Keyboard shortcuts settings section with accessible editable shortcut fields and saved custom shortcuts
- added braille-friendly NVDA announcements by sending both speech and braille messages and raising status bar accessibility events
- changed empty list boxes to expose readable placeholder items such as No results, No favorites, No subscriptions, and No queued downloads instead of leaving screen readers on unknown
- added 15 language choices and translated the new shortcut, empty-list, and important status/announcement strings
- Settings now announces Settings saved when the Save button is pressed

## What's new in version 0.5.1

- fixed the search result context menu so channels and playlists no longer show single-video actions such as Play, Download audio, or Download video
- channel and playlist result context menus now show collection-aware actions such as Open channel videos, Open playlist videos, Download channel, or Download playlist
- added a New videos action in Subscriptions that opens only the videos found since the last subscription check
- pressing Enter on a subscribed channel opens all channel videos, and Escape returns to the Subscriptions list
- tightened library item handling so saved channel and playlist items open as collections instead of being treated as single videos

## What's new in version 0.5.0

- added a History screen for recently played and downloaded items, with keyboard and context menu actions for play, download, favorite, subscribe, copy URL, remove, and clear history
- added a Subscriptions screen for YouTube channels, including Ctrl+Shift+S subscription shortcuts, context menu subscription actions, manual checks, and automatic background checks
- added Windows notifications for new videos from subscribed channels, with settings for automatic checks, check interval, and notifications
- added optional system tray behavior so the close button or Alt+F4 can hide ApricotPlayer to the tray while the Exit command still closes it completely
- renamed the portable release package to `ApricotPlayer.zip` while keeping updater compatibility with the old portable asset name

## What's new in version 0.4.7

- changed the yt-dlp component update check so screen readers only announce "updating components" after a newer yt-dlp package has actually been found and installation is starting
- kept no-update startup checks quiet so ApricotPlayer does not announce component updating when there is nothing to install
- published updated installer and portable release assets for testing updates from 0.4.6

## What's new in version 0.4.6

- fixed update checks on systems where Python/OpenSSL could not find a trusted certificate authority by using the bundled `certifi` certificate store for GitHub and PyPI HTTPS requests
- changed the app updater to exit ApricotPlayer immediately after launching the updater helper instead of relying on a delayed UI timer, preventing the old window from getting stuck as not responding
- updater helper scripts now wait up to 15 seconds for ApricotPlayer to close and then force-close only that ApricotPlayer process if needed, instead of waiting up to 180 seconds
- kept installer and portable ZIP update paths aligned so both release assets use the same certificate and shutdown fixes

## What's new in version 0.4.5

- added a Current downloads screen that shows pending and active downloads together, including batch progress, current item, remaining count, and cancel controls
- queued channels and playlists can now be downloaded together with queued videos, including audio or video mode
- the result context menu now offers `Download all selected items` when more than one item is queued
- new default settings use dynamic result loading, shown as `Dynamic (loads 20 at a time)` instead of a raw `0`
- YouTube search now defaults to `All`, followed by videos, playlists, and channels
- channel and playlist result rows no longer show irrelevant view counts
- pressing Escape from a channel or playlist result list now returns to the previous search results instead of the main menu
- the yt-dlp component updater now checks PyPI in the background, installs newer yt-dlp code into the per-user components folder, and announces updating/done through the screen reader
- added translations for the new download status and component-update messages across all supported UI languages

## What's new in version 0.4.4

- fixed the update dialog buttons by binding `Update now` and `Skip this version` directly to the modal result instead of relying on implicit wx dialog behavior
- `Skip this version` now announces and updates the status immediately, so screen-reader users know the choice was saved
- updater diagnostics now start logging as soon as the update prompt is shown, including button choice, download start, downloaded package size, and script launch path
- verified the update prompt programmatically: `Skip this version` returns skip and `Update now` returns update

## What's new in version 0.4.3

- changed the default video download format to MP4 so normal video downloads no longer default to WebM
- added friendly video download format choices in Settings: MP4 recommended, best available, MP4 single file, and smallest file
- migrated the old `bestvideo+bestaudio/best` setting to the new MP4 recommended mode automatically
- video downloads now set `merge_output_format` to `mp4` for MP4 modes
- translated the new video format labels across all supported UI languages

## What's new in version 0.4.2

- made mpv IPC commands serialized and retry-aware so pitch changes no longer fail when the player pipe is still starting or briefly busy
- made pitch changes retry silently instead of announcing a transient `Pitch control is not available yet` message
- verified the bundled mpv accepts pitch property changes, Rubberband filter insertion, and runtime Rubberband `set-pitch` commands through JSON IPC

## What's new in version 0.4.1

- added ten more UI languages: German, French, Spanish, Portuguese, Italian, Polish, Dutch, Swedish, Croatian, and Serbian
- fixed remaining untranslated Slovenian settings and player strings
- changed the language selector to use the shared language registry instead of a hard-coded Slovenian/English toggle
- localized pitch-control mode labels while keeping the saved settings values stable
- added a README localization policy so every future UI change is translated across all supported languages in the same release

## 0.4

- added `All` to YouTube search so videos, playlists, and channels can appear in one result list
- added context-menu downloads for playlist and channel results; ApricotPlayer creates a dedicated folder under the configured download folder and downloads the videos there
- changed release assets going forward to `ApricotPlayerSetup.exe` plus `ApricotPlayerPortable.zip`; the raw single-file exe is no longer published by default
- added portable ZIP updater support so portable builds can update by downloading and extracting the new ZIP into the current app folder
- restored linked pitch/speed mode as `Linked pitch and speed - pitch keys change both`; pitch keys now adjust pitch and speed together, while speed keys still adjust only speed

## 0.3.8

- renamed the startup app update setting to simply `Check for updates at startup`
- added a manual `Check for updates` button in Settings that checks the latest GitHub release immediately
- manual update checks ignore a previously skipped version so users can retry an update without editing their settings file
- manual update checks announce the result for screen readers

## 0.3.7

- fixed the runtime Rubberband `set-pitch` IPC call so pitch changes update the existing labeled filter instead of recreating it
- kept pitch and playback speed independent while making repeated pitch key presses smoother

## 0.3.6

- rebuilt player pitch control so pitch and playback speed remain independent in every supported pitch mode
- removed the confusing legacy `linked speed` pitch mode; old settings using it now migrate to the recommended independent Rubberband mode
- renamed pitch mode choices to describe what they do directly: best-quality Rubberband independent pitch or basic mpv built-in independent pitch
- Rubberband pitch changes now update the labeled mpv audio filter instead of replacing the whole audio filter chain or changing playback speed

## 0.3.5

- installer builds now use PyInstaller `onedir` layout so installed launches no longer pay the one-file extraction cost every time
- portable `ApricotPlayer.exe` remains available as a single-file build for quick sharing
- installer script now supports packaging either a single executable or a full application directory
- installer updates clean the old `_internal` runtime folder before installing the new one-dir runtime
- release build script now supports `-PackageMode onefile` and `-PackageMode onedir`

## 0.3.4

- optimized the Windows build by removing the broad `--collect-all wx` bundle step
- excluded unused heavyweight modules from the packaged app, including IPython, matplotlib, numpy, PIL, tkinter, and jedi
- reduced the installed executable size and one-file startup unpacking work for faster launches
- deferred loading `yt-dlp` until search, playback, downloads, cookies export, or update checks actually need it, so the main menu can appear sooner
- delayed automatic YouTube/app update checks for a few seconds after startup so they no longer compete with the first menu render

## 0.3.3

- download actions now always speak `Download started.` before the download worker starts, independent of the completion popup setting
- download worker startup is delayed slightly so very fast downloads cannot immediately override the screen-reader start announcement

## 0.3.2

- settings are now split into keyboard-friendly sections: General, Playback, Downloads, Cookies and network, and Updates and advanced
- when a cookies file is configured, ApricotPlayer no longer also tries to read browser cookies, avoiding Chrome cookie database lock errors
- added an export/cache browser cookies action that writes `%APPDATA%\ApricotPlayer\cookies.txt` and then switches browser cookie extraction off
- download hotkeys now leave a short delay after announcing `Downloading audio...` or `Downloading video...` before starting the worker, so screen readers can speak the start message

## 0.3.1

- fixed `Ctrl+Shift+A` and `Ctrl+Shift+D` by registering them as app-wide download accelerators and handling them from the player panel
- made result/player download shortcut detection more reliable for Windows key events
- added a clearer Chrome cookie database error hint, including closing Chrome or using Edge, Firefox, or cookies.txt
- pitch failures now announce that pitch control is unavailable instead of saying timing is unavailable
- installer-based updates now request the desktop shortcut task so installed test builds stay easy to launch

## 0.3

- public README no longer references local development download paths
- added an Inno Setup based Windows installer build that installs ApricotPlayer into the standard Windows Programs folder
- installer adds a Start Menu shortcut and offers an optional desktop shortcut
- updater now prefers `ApricotPlayerSetup.exe` when a release provides it, then falls back to the portable `.exe`
- updater now checks GitHub's `/releases/latest` endpoint instead of assuming the release list is ordered correctly
- release publishing now uploads both the portable executable and installer when both exist

## 0.2.2

- added a `Cookies from browser` setting for YouTube sign-in/bot-check cases
- yt-dlp now receives browser cookies for search, playback, playlist/channel loading, and downloads when this setting is enabled
- YouTube authentication/bot-check errors now include a direct Settings hint

## 0.2.1

- fixed packaged playback failing with `NoneType` stream write errors
- queued downloads now open a dedicated `Queued videos for download` screen instead of a hidden main-menu `Download all` action
- queued download screen supports downloading the selected item, audio/video context menu actions, `Ctrl+Shift+A`, `Ctrl+Shift+D`, and a tab-reachable `Download all` button
- batch downloads now clear the queue immediately so the queued option disappears once there is nothing waiting
- batch downloads announce when they start and when each queued item begins downloading

## 0.2

- updater install flow no longer waits behind a final OK dialog; it logs to `%APPDATA%\ApricotPlayer\updater.log`, exits the app, replaces the `.exe`, and restarts
- video details are created only after pressing `V`; `Escape` closes details back to the player before returning to results
- added `F2` volume boost with screen-reader announcements and automatic clamp back to 100 when disabled
- added cross-search batch download queue with `Shift+A` for audio and `Shift+D` for video
- main menu now shows `Download all` dynamically when the batch queue has items

## 0.1.4

- speed and pitch hotkeys now default to `0.01x` steps
- settings now include playback speed step, pitch step, and pitch control mode
- pitch control defaults to a higher-quality rubberband filter, with optional mpv pitch and linked-speed modes
- `Ctrl+Shift+A` and `Ctrl+Shift+D` are handled more reliably in both results and player mode
- downloads now announce start and can either show a completion popup or only speak completion through the screen reader

## 0.1.3

- updater now shows a visible progress dialog immediately after `Update now`
- updater errors are now shown in an error dialog instead of only the status bar
- updater now prefers the public GitHub `browser_download_url` and falls back to the API asset URL
- playback speed announcements now always use two decimals such as `0.25x`, `0.50x`, and `1.10x`
- speed control now includes `0.25x`

## 0.1.2

- default language for new installs is now English
- settings are always saved to the user's `%APPDATA%\ApricotPlayer\settings.json`
- settings screen now includes a settings file path and a Restore to defaults button
- player announcements now try NVDA speech directly through `nvdaControllerClient`
- `T`, `V`, `L`, speed changes, and pitch changes now use the same announcement path
- video details now use a standard multiline read-only text field for better screen reader reading, arrow navigation, and copying
- speed and pitch controls now have finer steps, including `1.1x`
- reaching default `1.0x` speed or pitch plays a short confirmation sound
- dynamic results mode now fetches the first 20 results first, then fetches more only when needed
- context menu download items now show `Ctrl+Shift+A` and `Ctrl+Shift+D`

## 0.1.1

- updater now uses a PowerShell replacement script that waits for the old app process to exit before copying the new `.exe`
- updater now validates that the downloaded file is a Windows executable before replacing the app
- Escape from player mode now returns focus directly to the results list instead of the search field
- switched versioning from beta suffixes to normal `0.1.x` releases

## 0.1.0-beta.4

- update prompt title is now `Update available`
- update prompt now shows `Version ...`, `What's new?`, and `Do you want to update now?`
- update prompt buttons are now `Update now` and `Skip this version`
- skipped app update versions are remembered so the same release is not offered repeatedly

## 0.1.0-beta.3

- player announcements now raise explicit Windows accessibility events for screen readers
- `T` duration announcements should be spoken by NVDA instead of only changing the status bar
- `S` and `D` now use predictable playback speed steps such as `0.5x`, `0.75x`, and `1.0x`
- this release is built outside the user's Downloads folder to test the auto-updater path

## 0.1.0-beta.2

- updater now shows the release changelog before asking to install the update
- GitHub release publishing now requires release notes
- added pitch control in player mode with `Ctrl+Up` and `Ctrl+Down`
- kept `S` and `D` for playback speed control

## 0.1.0-beta.1

- initial private beta release
- GitHub-based updater for the packaged `.exe`
- dynamic results loading
- in-app playback
- download progress reporting
