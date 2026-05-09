# ApricotPlayer

Accessible YouTube player and downloader for Windows, built in Python with `wxPython`.

Current version: `0.7.2` (`0.7.2`)

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
- Local media file playback for common audio and video files, including file association support on Windows
- First-run repair prompt if Windows media player registration is missing after an update
- Play from folder for choosing local media inside ApricotPlayer
- Accessible 10-band equalizer with descriptive frequency sliders, global Settings controls, genre/sound presets, three custom profiles, and player-only live controls
- Player Bass boost checkbox for a temporary per-video EQ boost
- Player clip markers and FFmpeg export for the marked section of the current video, local file, or podcast
- Marked clips now export through the normal download shortcuts: `Ctrl+Shift+A` for audio clips and `Ctrl+Shift+D` for video clips
- Start and end clip marker shortcuts toggle markers on and off, so accidental markers can be cleared immediately
- Local-file edit mode for changing speed, pitch, or equalizer, then saving an edited copy or replacing the original file
- Playback queue with add, remove, open queue, Enter-to-play, and automatic next-item playback
- Optional Save As behavior for downloads, with file dialogs for single downloads and folder dialogs for channel or playlist downloads
- First-run language selection for new users before they start using the app
- Clip marker shortcuts use physical `LeftBracket` and `RightBracket` keys, so they work across keyboard layouts
- Optional screen reader announcement for Playing or Paused when pressing Space in the player
- Audio and video downloads with progress updates
- Default downloads go to `Downloads\ApricotPlayer`, with YouTube music/video files under `music` and podcast episodes under `podcasts`
- Video downloads default to MP4, with selectable video download format options in Settings
- Audio download quality settings show clear VBR and kbps labels
- Current downloads screen with active batch/playlist/channel status and cancel controls
- Favorites
- History screen for recently played and downloaded items
- User-created playlists with create, add, remove, play, and download actions
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
- Browser cookie export can close the selected browser first, scan Brave/Chrome/Edge/Firefox profiles, export a usable `cookies.txt`, and automatically refresh cookies only after YouTube asks for sign-in or bot confirmation
- Age-restricted YouTube support is optional in Playback settings, off by default, and uses cookies plus the bundled EJS/Node fallback only when the normal fast playback attempt fails
- Settings can hide History or Podcasts and RSS from the main menu
- Windows notifications for new videos from subscribed channels
- Optional Windows notifications for completed downloads when ApricotPlayer is not focused
- Optional playback cache and resume playback from the last position
- Default audio output device setting with an accessible combo box, startup validation for missing devices, and a player output-device picker for session-only changes
- Selectable playback-speed audio processing modes, including tuned `scaletempo2`, classic `scaletempo`, mpv default, and Rubberband
- Optional system tray mode when closing the window
- Optional Windows startup mode that launches ApricotPlayer directly into the system tray
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
- `Escape`: back to the last search results

## Download shortcuts

- `Ctrl+Shift+A`: download audio
- `Ctrl+Shift+D`: download video
- `Shift+A`: select the focused video for download or adding to playlists
- `Ctrl+Shift+S`: subscribe to the focused item's channel
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
- `scripts/publish_release.ps1 -Tag v0.6.9 -NotesFile release-notes/v0.6.9.md` publishes the installer and portable ZIP to GitHub Releases
