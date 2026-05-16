# ApricotPlayer

Accessible YouTube player and downloader for Windows, built in Python with `wxPython`.

Current version: `0.8.54` (`0.8.54`)

## Download

Download the latest Windows installer or portable ZIP from the [GitHub Releases page](https://github.com/Urh2006/ApricotPlayer/releases/latest).

The installer adds ApricotPlayer to the Windows Start Menu, can create a desktop shortcut, and can register ApricotPlayer as a Windows media player for common audio/video files. User settings are stored per user in `%APPDATA%\ApricotPlayer\settings.json`.

## Run from source

1. Install Python 3.11 or newer.
2. Install dependencies:

```powershell
py -m pip install -r requirements.txt
```

3. Start the app:

```powershell
py wx_main.py
```

## Features

- NVDA-friendly main menu and search flow
- In-app YouTube playback with `mpv`
- Optional background playback while browsing results, settings, downloads, and the main menu, with a screen-reader named Player section and a Close player control
- When background playback is enabled, normal player mode keeps the current result list visible and tabbable; Tab from results lands on Player, Tab from Player lands on Previous, Play/Pause, Next, and Shift+Tab from Player returns to results
- Fullscreen player mode hides the result list and uses a Fullscreen checkbox plus Back to results to return to the normal player-with-results layout
- When background playback is disabled, the player uses the classic focused layout with Back to results, Back to main menu, and no persistent result list
- In classic player mode, Tab from Player moves through Previous, Play/Pause, Next, and the rest of the player controls, while Shift+Tab reaches the Back controls
- Player background controls on result/library/settings screens, plus context-menu actions from the player for download, queue, favorites, playlists, stream URL, output device, equalizer, and close
- Background player sections start with a focusable screen-reader named Player target, followed by Previous, Play/Pause, Next, with explicit Tab and Shift+Tab navigation for consistency across screens
- Shift+Tab from the background Player section returns directly to the active main menu or result list instead of cycling around the whole window
- Global `Ctrl+Space` background play/pause and `Ctrl+L` copy-link support from search results
- Optional Trending screen, hidden by default, with country and category filters using the official YouTube most-popular API when a YouTube Data API key is configured
- Channel result options for opening channel videos, channel playlists, or popular videos sorted by view count
- YouTube video context menus include Open channel when channel metadata is available, with a configurable `Ctrl+Shift+O` shortcut
- Playlist and channel context menus offer Download as a submenu with separate audio and video choices, while `Ctrl+Shift+A` and `Ctrl+Shift+D` keep working directly.
- Dynamic result loading keeps fetching 20 more search, channel, playlist, or folder items until the source has no more results, without moving screen reader focus while new rows append
- Dynamic result playback preserves the exact selected item even if a new result page finishes loading at the same moment
- Local media file and whole-folder playback for common audio and video files, including file association support on Windows
- First-run repair prompt if Windows media player registration is missing after an update
- Play from folder for choosing local media inside ApricotPlayer, with folder contents shown as local media results and cached in memory for quick return from playback
- Accessible 10-band equalizer with descriptive frequency sliders, global Settings controls, genre/sound presets, dynamic custom profiles, delete support for custom profiles, player-only live controls, and a player action to save the current EQ as a global preset
- Player and global equalizers share selectable 6/12/18/24 dB slider ranges with quieter screen-reader value updates
- Player and global equalizer sliders use stable keyboard steps across the full 24 dB range.
- Player Bass boost checkbox for a session-based EQ boost that stays active for the next item until turned off without resetting equalizer settings.
- Player hotkeys include `F2` volume boost, `F3` bass boost, `F4` equalizer, `R` repeat, and `Shift+S` shuffle
- Optional Playback setting to announce or silence the “Playback finished” screen-reader message
- Player clip markers and FFmpeg export for the marked section of the current video, local file, or podcast
- Marked clips now export through the normal download shortcuts: `Ctrl+Shift+A` for audio clips and `Ctrl+Shift+D` for video clips
- Start and end clip marker shortcuts toggle markers on and off, so accidental markers can be cleared immediately
- Local-file edit mode for changing speed, pitch, or equalizer, then saving an edited copy or replacing the original file
- Playback queue with add, remove, open queue, Enter-to-play, and automatic next-item playback
- Optional Save As behavior for downloads, with file dialogs for single downloads and folder dialogs for channel or playlist downloads
- File converter and folder converter menu items for accessible FFmpeg-powered conversion between common audio and video formats
- Converter output names keep the original file name and only change the extension, with numbered fallback names for collisions
- Converter output controls can create new files/folders or safely replace originals, with recursive folder conversion, progress, and optional completion messages
- Audio-to-video conversion can create a dark-background video or use a chosen image as the video background
- First-run language selection for new users before they start using the app
- Clip marker shortcuts use physical `LeftBracket` and `RightBracket` keys, so they work across keyboard layouts
- Optional screen reader announcement for Playing or Paused when pressing Space in the player
- Configurable default playback volume and an optional volume boost default that lets the player start ready for volume above 100%
- Audio and video downloads with progress updates
- Playlist, channel, and larger batch downloads show a progress window with current item and remaining count
- Optional stream URL cache and next-item prefetch can make repeated playback and Next/Previous transitions start faster without storing whole media files, with cache duration choices up to 7 days or permanent-until-YouTube-expires
- Default downloads go to `Downloads\ApricotPlayer`, with YouTube music/video files under `music` and podcast episodes under `podcasts`
- Video downloads default to MP4, with selectable video download format options in Settings
- Audio download quality settings show clear VBR and kbps labels
- Current downloads screen with active batch/playlist/channel status and cancel controls
- Favorites
- History screen for recently played and downloaded items
- User-created playlists with create, add, remove, play, and download actions
- User playlists can include YouTube videos, podcast episodes, direct links, and local media files
- Subscriptions screen for YouTube channels, with manual and automatic checks for new videos
- Subscription and podcast/RSS automatic refresh intervals include a 30-minute option
- New videos view for each subscription after a subscription check finds new channel uploads
- Notification center for saved subscription and podcast notifications, with Enter playback
- `Ctrl+Shift+V` opens the notification center directly from anywhere in the app
- The player window title includes the currently playing video, podcast, or episode title for Alt+Tab
- App update checks can run periodically in tray mode, show an Update available main-menu item, and optionally send Windows notifications
- Podcasts and RSS feeds screen with Apple Podcasts directory search, direct RSS/Atom feed URLs, refresh, open, play, download audio, download entire feed, queue episodes, copy URL, and browser actions
- Direct link screen for playing or downloading any `yt-dlp` supported URL
- Direct link Enter behavior can be configured in Settings
- Browser cookie export can close the selected browser first, scan Brave/Chrome/Edge/Firefox profiles, use a Chromium DevTools fallback when normal extraction finds no login cookies, report profile diagnostics, export a usable `cookies.txt`, and automatically refresh cookies only after YouTube asks for sign-in or bot confirmation
- Manually chosen `cookies.txt` files are used after a fast playback auth failure even when age-restricted fallback support is off
- Settings can be reset globally from General, or per section from the active Settings section
- Age-restricted YouTube support is optional in Playback settings, off by default, and uses cookies plus the bundled EJS/Node fallback only when the normal fast playback attempt fails
- Settings can hide Trending, History, or Podcasts and RSS from the main menu
- Windows notifications for new videos from subscribed channels
- Optional Windows notifications for completed downloads when ApricotPlayer is not focused
- Optional playback cache and resume playback from the last position
- Default audio output device setting with an accessible combo box, startup validation for missing devices, and a player output-device picker for session-only changes
- Selectable playback-speed audio processing modes, including tuned `scaletempo2`, classic `scaletempo`, mpv default, and Rubberband
- Optional system tray mode when closing the window
- Optional Windows startup mode that launches ApricotPlayer normally when Windows starts
- Tray mode can restore the existing app window from a second launch, open Settings from the tray menu, and optionally show a Windows notification when ApricotPlayer is sent to the tray
- The tray icon keeps keyboard context menu access for Show ApricotPlayer, Settings, subscription check, and Exit
- Player repeat checkbox and finished-video restart behavior when pressing Space or Play
- UI languages: English, Slovenian, German, French, Spanish, Portuguese, Italian, Polish, Dutch, Swedish, Croatian, Serbian, Czech, Slovak, Hungarian, Romanian, Turkish, Ukrainian, Russian, Japanese, Korean, Chinese Simplified, Arabic, Hindi, Indonesian, Finnish, and Greek
- Configurable keyboard shortcuts in Settings, captured by pressing the desired key combination, with duplicate shortcut warnings
- Configurable global navigation shortcuts for Search, Direct link, Favorites, Playlists, Subscriptions, Notification center, Settings, and other main screens
- Legacy shortcut conflicts are repaired on startup when an older settings file assigned the same key to multiple actions
- Keyboard shortcut settings use a lightweight action list and one capture field for faster screen reader navigation
- Recommended pitch and speed processing modes appear first in their Settings combo boxes
- Braille-friendly NVDA announcements using speech, braille messages, and status bar accessibility events
- Focus-preserving search result updates while background metadata is filled in
- Stale search protection prevents older background searches from replacing newer results
- Reusable list refresh handling avoids unnecessary list rebuilds across library, queue, RSS, history, and notification screens
- Dynamic search mode is the default for new settings, loading results in chunks of 20
- Dynamic result metadata hydration covers newly loaded result pages, not only the first page
- GitHub release updater for installed and packaged builds, with installer updates applied to the exact running install folder and restarted from the newly installed executable
- Update downloads are hardened with trusted asset names, HTTPS checks, GitHub/PyPI SHA-256 verification when published, and safe ZIP extraction checks
- Updater HTTPS checks use the bundled `certifi` certificate store for more reliable GitHub access
- yt-dlp component updater that can refresh the Python package into `%APPDATA%\ApricotPlayer\components` and only announces component updating when an update is actually being installed
- Per-user settings in `%APPDATA%\ApricotPlayer\settings.json`

## Player shortcuts

- `Space`: play/pause
- `Left/Right`: seek 5 seconds
- `Ctrl+Left/Right`: seek 1 minute
- `Ctrl+Shift+Left/Right`: seek 10 minutes
- `Up/Down`: volume up/down
- `Ctrl+Up/Down`: pitch up/down
- `S` / `D`: slower / faster playback
- `T`: announce elapsed, remaining, and total time
- `V`: open video details
- `O`: choose audio output device for the current video
- `G`: open the player equalizer
- `E`: toggle local-file edit mode
- `LeftBracket` / `RightBracket`: set clip start and end markers
- `Ctrl+Shift+A`: export marked section as audio when both markers are set
- `Ctrl+Shift+D`: export marked section as video when both markers are set
- `Ctrl+S`: save an edited copy in local-file edit mode
- `Ctrl+R`: replace the original file in local-file edit mode
- `Ctrl+Alt+Q`: open playback queue
- `L`: copy current video link
- `Ctrl+D`: copy the direct media stream URL
- `Ctrl+PageUp/PageDown`: previous or next item
- `Ctrl+Space`: play/pause while playback continues in the background
- `Escape`: leave the player and keep playback running in the background

## Download shortcuts

- `Ctrl+Shift+A`: download audio
- `Ctrl+Shift+D`: download video
- `Shift+A`: select the focused video for download or adding to playlists
- `Ctrl+Shift+S`: subscribe to the focused item's channel
- `Ctrl+L`: copy the focused result link
- `Ctrl+Shift+N`: create playlist
- `Ctrl+Shift+P`: add the focused or queued videos to a playlist
- `Ctrl+Shift+R`: remove an item from a user playlist
- `Ctrl+Shift+Q`: add the focused item to the playback queue
- `Ctrl+Shift+Delete`: remove the focused item from the playback queue
- `Ctrl+Shift+V`: open the notification center
- `Ctrl+Alt+O`: play from folder

## Accessibility

The app uses native Windows controls where possible so it behaves well with NVDA and keyboard navigation. On startup it also checks for `yt-dlp` updates to keep YouTube playback and downloads working.

## Localization

ApricotPlayer keeps UI strings in synchronized language packs. When adding a new feature, setting, dialog, shortcut label, or status message, update every supported language in the same change and keep placeholder names such as `{title}` and `{error}` unchanged.

## Updates

The packaged app checks public GitHub releases automatically unless disabled in Settings. Before installing a newer version, it shows an `Update available` dialog with the version, changelog, and `Update now` / `Skip this version` buttons.

Release notes must start with `# What's new in version X.Y.Z` so the updater dialog includes the target version in the changelog text itself.

Helpful scripts:

- `scripts/build_release.ps1 -PackageMode onedir` builds the fast app folder used by both the installer and portable ZIP
- `scripts/build_installer.ps1` builds `ApricotPlayerSetup.exe` when Inno Setup is installed
- `scripts/build_portable_zip.ps1` builds `ApricotPlayer.zip`
- `scripts/publish_release.ps1 -Tag v0.8.33 -NotesFile release-notes/v0.8.33.md` publishes the installer and portable ZIP to GitHub Releases
