# ApricotPlayer

Accessible YouTube player and downloader for Windows, built in Python with `wxPython`.

Current version: `0.1.2` (`0.1.2`)

The current standalone build is written to:

```text
C:\Users\urhst\Downloads\ApricotPlayer.exe
```

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
- GitHub release updater for the packaged `.exe`
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

The packaged app can check GitHub releases automatically. For a private repository, testers need either:

- GitHub CLI installed and logged in, or
- a GitHub token entered in the app settings

Helpful scripts:

- `scripts/build_release.ps1` builds `ApricotPlayer.exe`
- `scripts/publish_release.ps1 -Tag v0.1.2 -NotesFile release-notes/v0.1.2.md` publishes the built `.exe` to GitHub Releases

Before the app installs a newer version, it shows an `Update available` dialog with the version, changelog, and `Update now` / `Skip this version` buttons.
