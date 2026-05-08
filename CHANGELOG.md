# Changelog

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
