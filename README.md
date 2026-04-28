# ApricotPlayer

Accessible YouTube player and downloader for Windows, built in Python with `wxPython`.

Current version: `0.3.7` (`0.3.7`)

## Download

Download the latest Windows installer from the [GitHub Releases page](https://github.com/Urh2006/ApricotPlayer/releases/latest).

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
- Favorites
- Slovenian and English UI
- Dynamic search mode with results loading in chunks of 20
- GitHub release updater for installed and packaged builds
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

## Accessibility

The app uses native Windows controls where possible so it behaves well with NVDA and keyboard navigation. On startup it also checks for `yt-dlp` updates to keep YouTube playback and downloads working.

## Updates

The packaged app checks public GitHub releases automatically unless disabled in Settings. Before installing a newer version, it shows an `Update available` dialog with the version, changelog, and `Update now` / `Skip this version` buttons.

Helpful scripts:

- `scripts/build_release.ps1 -PackageMode onefile` builds the portable `ApricotPlayer.exe`
- `scripts/build_release.ps1 -PackageMode onedir` builds a faster installed app folder
- `scripts/build_installer.ps1` builds `ApricotPlayerSetup.exe` when Inno Setup is installed
- `scripts/publish_release.ps1 -Tag v0.3.7 -NotesFile release-notes/v0.3.7.md` publishes release assets to GitHub Releases
