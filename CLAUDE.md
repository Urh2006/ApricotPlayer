# ApricotPlayer — Claude Code navodila

## Projekt

ApricotPlayer je dostopen medijski predvajalnik za Windows (wxPython, mpv, yt-dlp).
Arhitektura: ~20 mixin razredov, združenih v `MainFrame` prek večkratne dednosti v `wx_main.py`.

## Delovni postopek (obvezno upoštevaj vedno)

### Ko končaš spremembe:
1. **Bumpaš verzijo** v vseh treh lokacijah:
   - `apricot/__init__.py` → `__version__`
   - `apricot/constants.py` → `APP_VERSION` in `APP_VERSION_LABEL`
2. **Posodobiš CHANGELOG.md** — nov razdelek na vrhu v formatu `# v{verzija} - Naslov`
3. **Dodaš release notes** → `release-notes/v{verzija}.md`
4. **Commitas in pushaš direktno na `main`** (brez PR-jev)
5. **Tagaš** z `git tag v{verzija}` in pushaš tag: `git push origin v{verzija}`
6. **GitHub Actions** (`build-release.yml`) se samodejno sproži ob tagu in zgradi:
   - `ApricotPlayerSetup.exe` (Inno Setup installer)
   - `ApricotPlayer.zip` (portable zip)
   - GitHub Release z obema artefaktoma

### PR-ji niso potrebni — gre vse direktno na main.

## Komunikacija

- Vsak odgovor začni z `# Claude je odgovoril` (heading level 1) — uporabnik navigira z bralnikom zaslona.

## Struktura projekta

```
apricot/
  __init__.py          # __version__
  constants.py         # APP_VERSION, APP_VERSION_LABEL, vse konstante
  ui/                  # Mixin razredi (cookies, dialogs, equalizer, misc, settings, ...)
  player/              # mpv IPC, playback, volume
  network/             # cookies, youtube
  updater/             # auto-updater (GitHub Releases API)
  download/, library/, media/, search/, system/
wx_main.py             # MainFrame (združuje vse mixine)
vendor/
  nvda/                # nvdaControllerClient64.dll (commitano)
  mpv/                 # mpv.exe + DLL-ji (NI commitano — CI jih prenese)
  ffmpeg/              # ffmpeg.exe (NI commitano — CI jih prenese)
assets/
  default_reached.wav
installer/
  ApricotPlayer.iss    # Inno Setup skript
scripts/
  build_release.ps1    # PyInstaller build (Windows)
  build_installer.ps1  # Inno Setup build
  build_portable_zip.ps1
  publish_release.ps1  # gh CLI release publish
.github/workflows/
  build-release.yml    # Avtomatski CI/CD ob tagu v*
```

## Updater

Updater (`apricot/updater/updater.py`) išče:
- `ApricotPlayerSetup.exe` (installer build)
- `ApricotPlayer.zip` (portable build)

Oba morata biti priložena vsakemu GitHub Releaseu z natančno tema imenoma.

## Vendor binaries (CI jih prenese samodejno)

- **mpv**: shinchiro/mpv-winbuild-cmake releases, `mpv-x86_64-*.7z`
- **ffmpeg**: BtbN/FFmpeg-Builds, `ffmpeg-master-latest-win64-gpl.zip`
- **nvda**: `vendor/nvda/nvdaControllerClient64.dll` je že v repozitoriju
