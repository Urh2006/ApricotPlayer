# Changelog

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
