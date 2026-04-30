# ApricotPlayer

Accessible YouTube player and downloader for Windows, built in Python with `wxPython`.

Current version: `0.5.3` (`0.5.3`)

## Download

Download the latest Windows installer or portable ZIP from the [GitHub Releases page](https://github.com/Urh2006/ApricotPlayer/releases/latest).

The installer adds ApricotPlayer to the Windows Start Menu and can create a desktop shortcut. User settings are stored per user in `%APPDATA%\ApricotPlayer\settings.json`.

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
- Audio and video downloads with progress updates
- Video downloads default to MP4, with selectable video download format options in Settings
- Current downloads screen with active batch/playlist/channel status and cancel controls
- Favorites
- History screen for recently played and downloaded items
- Subscriptions screen for YouTube channels, with manual and automatic checks for new videos
- New videos view for each subscription after a subscription check finds new channel uploads
- Windows notifications for new videos from subscribed channels
- Optional Windows notifications for completed downloads when ApricotPlayer is not focused
- Optional system tray mode when closing the window
- UI languages: English, Slovenian, German, French, Spanish, Portuguese, Italian, Polish, Dutch, Swedish, Croatian, Serbian, Czech, Slovak, Hungarian, Romanian, Turkish, Ukrainian, Russian, Japanese, Korean, Chinese Simplified, Arabic, Hindi, Indonesian, Finnish, and Greek
- Configurable keyboard shortcuts in Settings, captured by pressing the desired key combination
- Braille-friendly NVDA announcements using speech, braille messages, and status bar accessibility events
- Dynamic search mode is the default for new settings, loading results in chunks of 20
- GitHub release updater for installed and packaged builds
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
- `L`: copy current video link
- `Escape`: back to the last search results

## Download shortcuts

- `Ctrl+Shift+A`: download audio
- `Ctrl+Shift+D`: download video
- `Ctrl+Shift+S`: subscribe to the focused item's channel

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
- `scripts/publish_release.ps1 -Tag v0.5.3 -NotesFile release-notes/v0.5.3.md` publishes the installer and portable ZIP to GitHub Releases
