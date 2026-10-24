# ApricotPlayer

ApricotPlayer is an accessible media player and downloader for Windows and macOS. Search YouTube and SoundCloud, listen to Spotify, follow podcasts, watch videos, play your own files, and keep everything within reach of the keyboard.

The current stable release is **2.0.0**, written in Rust with native Windows and Mac interfaces. It brings Spotify and macOS support alongside the familiar ApricotPlayer features. Existing Windows users can update directly from the Python-based 1.0.21 release through the Stable update channel.

## Contents

- [Download and install](#download-and-install)
- [Getting around](#getting-around)
- [The main menu](#the-main-menu)
- [YouTube, SoundCloud, and direct links](#youtube-soundcloud-and-direct-links)
- [Spotify](#spotify)
- [Local files and folders](#local-files-and-folders)
- [Podcasts and RSS](#podcasts-and-rss)
- [AudioVault](#audiovault)
- [Favorites, playlists, history, and bookmarks](#favorites-playlists-history-and-bookmarks)
- [Playback and audio controls](#playback-and-audio-controls)
- [Downloads, clips, and conversion](#downloads-clips-and-conversion)
- [Settings and languages](#settings-and-languages)
- [Updates and your data](#updates-and-your-data)
- [Keyboard shortcuts](#keyboard-shortcuts)
- [Help and bug reports](#help-and-bug-reports)
- [Source code and development](#source-code-and-development)

## Download and install

Open the [Releases page](https://github.com/Urh2006/ApricotPlayer/releases), choose a release, and expand **Assets**. Choose **2.0.0 or a newer stable release** for everyday use. Releases marked **Pre-release** are optional test versions, offered through the Beta update channel. Read the release notes before installing.

The files under **Source code** are for development. To run ApricotPlayer, download one of the packages below. The packages include the media tools they need; you do not need to install Python, Rust, mpv, or FFmpeg separately.

### Windows

- **ApricotPlayerSetup.exe:** download and run the installer. It can add Start menu and desktop shortcuts and register ApricotPlayer for common audio and video files.
- **ApricotPlayer.zip:** download the portable package, extract the whole ZIP, and run **ApricotPlayer.exe** from the extracted folder. Keep its other files and folders together. The portable package still saves settings in your Windows user profile.

If you are upgrading a portable Rust 2.0 beta, download and extract the 2.0 ZIP manually for this transition. The older Rust portable updater cannot read its layout. Installed copies and Python 1.0.21 portable copies can use the normal updater; saved data remains in your user profile.

Windows releases contain the 64-bit build. Settings also provides media-player registration options if you want to change file associations later. Windows may ask you to finish choosing the default app in its own settings.

### macOS

The current Mac package requires an **Apple silicon Mac running macOS 27 or later**. There is no Intel Mac package in the current releases.

1. Download **ApricotPlayer-macOS-arm64.dmg**.
2. Open the disk image and copy **ApricotPlayer** to **Applications**.
3. Open ApricotPlayer from Applications, then eject the disk image.

The Mac app is not notarized by Apple. If macOS blocks its first launch, try opening the app once, then open **System Settings > Privacy & Security**, choose **Open Anyway** for ApricotPlayer, and confirm. Do this only for the package you downloaded from this project's Releases page.

A release may also contain **ApricotPlayer-macos-arm64.zip**. This is used by the in-app updater; the DMG is the normal download for a first installation.

## Getting around

On the first run, choose your language. Open **Settings** whenever you want to change the language, playback behavior, download folder, or shortcuts.

Use the arrow keys to move through menus and lists, **Enter** on Windows or **Return** on Mac to open the selected item, and **Tab** and **Shift+Tab** to move between controls. Menus support first-letter navigation. Use **Escape** to go back one screen, keeping your place in the list you came from.

Most lists and the player have a context menu with additional actions. Open it with **Applications** or **Shift+F10** on Windows, or **Shift+F10** on Mac. VoiceOver's **VO+Shift+M** menu command also opens the selected row's menu, and **VO+Space** activates a row.

**Action Finder** lets you search for an action by name and run it without remembering its shortcut: **Ctrl+Shift+J** on Windows or **Command+Shift+J** on Mac. Its available actions follow the current screen.

ApricotPlayer uses native controls with screen-reader support, including NVDA on Windows and VoiceOver on Mac. Menu labels can include your configured shortcuts. Playback messages, result details, progress, and errors are available without looking at the video.

### Browsing while something plays

Turn on **background playback** in Settings to keep listening as you browse. Escape then returns to the previous screen without closing the player. You can return to the player area with Tab; player shortcuts work when focus is on the player itself or one of its buttons, even when another screen is visible alongside it.

When focus is in a menu or list, that screen keeps its normal navigation and shortcuts. For example, typing **T** in a menu uses first-letter navigation; it announces the time when focus is in the player. The background play/pause shortcut is **Ctrl+Space** on Windows and **Command+Shift+Space** on Mac, including when ApricotPlayer is not the active application.

Use the player's **Close** button to end the player session. Shuffle, speed, pitch, and other session controls are retained while that player remains open, including when you choose another item. Escape also leaves full screen before returning to browsing. With background playback disabled, leaving the player stops playback.

## The main menu

These are the main-menu entries. Some appear only when there is something to show. The shortcuts below are the defaults; Windows is listed first, followed by Mac.

- **Current downloads:** inspect active downloads, progress, and cancellation options. Appears while downloads are active. **Ctrl+Alt+D / Command+Option+J**.
- **Playback queue:** open ApricotPlayer's queue for local files and supported non-Spotify sources. Play, remove, or clear queued items. Appears when the queue contains items. **Ctrl+Alt+Q / Command+Option+Q**.
- **Search YouTube / SoundCloud:** search for videos, tracks, playlists, channels, or artists. **Ctrl+Alt+Y / Command+Option+Y**.
- **Resume last playback session:** return to a saved session when one is available and resume is enabled. There is no dedicated default shortcut.
- **Trending:** browse YouTube trending results by country and category. This optional feature uses your YouTube Data API key. There is no dedicated default shortcut.
- **AudioVault:** search audio-described movies and TV shows or browse recent additions. **Ctrl+Alt+A / Command+Option+A**.
- **Spotify:** open Spotify search, your library, mixes, playlists, accounts, and more. **Ctrl+Alt+C / Command+Option+C**.
- **Play from folder:** browse a folder's media and play it as a collection. **Ctrl+Alt+O / Command+Option+O**.
- **Play file:** choose a local audio or video file. **Ctrl+Alt+I / Command+Option+I**.
- **Direct link:** paste a supported media, playlist, channel, stream, podcast, or Spotify link. Settings controls what Enter does here. **Ctrl+Alt+L / Command+Option+L**.
- **Favorites:** open items saved in ApricotPlayer's favorites. **Ctrl+Alt+F / Command+Option+F**.
- **Bookmarks:** return to named positions you saved within media. **Ctrl+Alt+K / Command+Option+K**.
- **Playlists:** create and use ApricotPlayer playlists, separate from Spotify playlists. **Ctrl+Alt+P / Command+Option+P**.
- **Subscriptions:** manage subscribed YouTube channels and check for new videos. **Ctrl+Alt+B / Command+Option+B**.
- **Notification center:** read saved notices about new subscription videos and podcast episodes. **Ctrl+Shift+V / Command+Shift+V**.
- **History:** revisit played and downloaded items when history is enabled. **Ctrl+Alt+H / Command+Option+H**.
- **Podcasts / RSS:** search for podcasts, add feeds, and browse episodes. **Ctrl+Alt+R / Command+Option+R**.
- **File converter:** convert one local audio or video file. There is no dedicated default shortcut.
- **Folder converter:** convert supported files in a folder, optionally including subfolders. There is no dedicated default shortcut.
- **Copy diagnostic report:** copy information useful in a bug report. **Ctrl+Alt+Shift+D / Command+Option+Shift+D**.
- **Update available:** review and install an offered application update. There is no dedicated default shortcut.
- **Settings:** change how ApricotPlayer works. **Ctrl+Alt+S / Command+Option+S**.
- **Exit:** quit ApricotPlayer, including background playback. There is no dedicated default shortcut.

Use **Settings > Customize main menu** to hide entries you do not use, including Spotify. Hiding an entry does not disable its shortcut. Settings, Exit, and an available update stay visible. **Ctrl+Alt+M / Command+Option+M** returns to the main menu.

## YouTube, SoundCloud, and direct links

Search YouTube for all results, videos, playlists, or channels. Search SoundCloud for tracks, playlists, or artists. Results include details such as duration, channel or artist, and playlist size where the service supplies them. Result-detail shortcuts let you read individual columns without opening an item.

Open a YouTube channel to browse its videos, popular uploads, playlists, and live streams. Open a playlist to choose a particular video, play it in order, or shuffle it. More results load as you reach the end of supported lists, without sending you back to the first result. You can also open items in your browser, copy their page links, or copy a resolved stream URL where available.

Direct links use yt-dlp for YouTube, SoundCloud, and other supported sites, as well as direct playable streams. Whether a particular link can play or download depends on the site's current support, permissions, and media format. Spotify links use the Spotify integration instead.

YouTube playback prefers the original audio language rather than automatically choosing a dubbed track. Playback can continue through a list or use related videos, according to Settings. There is also a command to play the next related YouTube video. You can choose browser playback instead of the internal player where offered.

### Subscriptions and notifications

Subscribe to a YouTube channel from a result or its menu. Organize subscriptions into your own categories and filter by category. Open a channel's videos or just its new videos, remove a subscription, and check manually or automatically at your chosen interval.

New items are kept in the Notification center so you can return to them later. Remove individual notices or clear the list when you have finished with them. Optional system notifications can announce new videos and completed downloads; their settings are separate from the saved notification list.

### Chapters, transcripts, lyrics, and comments

- **Chapters:** browse chapter titles, jump to one, or move to the previous or next chapter from the player.
- **Transcripts and subtitles:** open available captions or transcripts, search the text, jump to a timestamp, copy text, or copy a timestamped link. Local SRT and VTT subtitles are supported too.
- **Lyrics:** open local text or LRC lyrics, or look them up online, and copy the text. Timed lyrics highlight the current line when timing is available. Online lookup can be disabled in Settings.
- **YouTube comments:** browse comments and replies, change the supported sort order, search loaded comments, copy text, and open author links. Comments can use the YouTube Data API when a key is configured or the yt-dlp path otherwise.

These tools depend on what the source provides. Not every video has chapters or captions, and not every song has lyrics.

### Cookies and network settings

Most public content does not need browser cookies. For content that needs your signed-in session, Settings can export cookies from supported Brave, Chrome, Edge, Firefox, Chromium, Opera, or Vivaldi profiles, or import a cookies file. Browser and profile availability depends on your system. Open the selected browser's YouTube login profile from Settings if you need to sign in first. The exporter can offer to close the browser when needed. Treat cookies files as private account data.

Age-restricted YouTube support is a separate option, off by default. Advanced network options include a proxy, user-agent override, rate limit, retries, socket timeout, concurrent download fragments, a custom FFmpeg location, and a YouTube Data API key. Settings includes a link to obtain an API key. A key is needed for Trending; ordinary search and playback do not require one. Bundled JavaScript support is used when YouTube extraction requires it.

## Spotify

Spotify is built into ApricotPlayer through **librespot**. You sign in with your own Spotify account in your browser; ApricotPlayer does not ask you to type your Spotify password into the player. **Spotify Premium is required for full local music playback.** The official Spotify application does not need to be installed.

Open **Spotify > Log in**, finish the browser sign-in, and return to ApricotPlayer. The login screen lets you copy the link, reopen the browser, or retry. You can add several accounts, choose the active one, log out, log in again, or remove a saved account. Libraries and pinned Spotify-menu items follow the selected account.

### The Spotify menu

- **Search:** search all content or narrow it to tracks, artists, albums, playlists, shows, episodes, profiles, or audiobooks. **Ctrl+Alt+Shift+Y / Command+Option+Shift+Y**.
- **My library:** browse saved content, including library folders, with filters and a configurable sort order. **Ctrl+Alt+Shift+L / Command+Option+Shift+L**.
- **Liked songs:** browse and play your Spotify Liked Songs. **Ctrl+Alt+Shift+F / Command+Option+Shift+F**.
- **Playlists:** open playlists in your library or create and manage your own. **Ctrl+Alt+Shift+P / Command+Option+Shift+P**.
- **Followed artists:** browse artists you follow and open their music.
- **Home:** browse personalized sections returned for your account.
- **Daily mixes:** open your available mixes. **Ctrl+Alt+Shift+M / Command+Option+Shift+M**.
- **Recently played:** revisit items from Spotify's recent listening history.
- **Top tracks and artists:** browse lists for the last four weeks, last six months, or all time.
- **Browse all:** explore categories, pages, and their sections.
- **Queue:** view the separate Spotify queue. **Ctrl+Alt+Shift+Q / Command+Option+Shift+U**.
- **Devices:** choose an available Spotify Connect device and transfer playback. **Ctrl+Alt+Shift+O / Command+Option+Shift+O**.
- **My profile:** open Following, Followers, Find friends, and My public playlists. You can also open other profiles and playlist owners.
- **Settings:** change Spotify playback and library preferences.
- **Accounts:** manage sign-ins. **Ctrl+Alt+Shift+C / Command+Option+Shift+C**.

Entries without a shortcut above have no dedicated default shortcut. Login and account options depend on your sign-in state. Pin artists, playlists, categories, and browse sections to this Spotify menu from their context menus. Removing a pin leaves the original content and your Spotify library alone.

### Playing, queuing, and discovering music

Open a playlist or album, choose any track, and play from that point. Playback retains the collection so the following track comes from the same context, or follows shuffle when enabled. Opening another item starts it playing even if the previous one was paused. Previous can restart the current track after it has played for a while; use it again near the start to return to the preceding track.

The first-track and last-track commands jump to the edges of the current Spotify list in its original order, even with shuffle on. Repeat cycles between off, repeating the collection, and repeating one track. Shuffle cycles through off, shuffle, and Smart Shuffle for supported playlists. Smart Shuffle and recommendations depend on what Spotify makes available for that context.

Start a radio from a supported track, artist, album, or playlist to discover related music. Autoplay can follow your Spotify account setting, be forced on, or be turned off. Where Spotify supplies them, ApricotPlayer displays lyrics and episode transcripts and can play an episode's short preview. For a Spotify episode, the Lyrics command opens its available transcript. Podcasts and audiobooks follow the account's access and regional availability.

Spotify has its own queue, separate from ApricotPlayer's general playback queue. It distinguishes manually added songs, upcoming collection tracks, autoplay, and Smart Shuffle recommendations. When ApricotPlayer is the active playback device, its queue window lets you play an item, move manually queued songs up or down, remove items, or clear the manual queue while keeping the playing collection. Devices transfers playback to an available Connect device. Starting another source in ApricotPlayer releases Spotify so it does not take over the player again.

### Likes, follows, and playlist editing

**Like means a real Spotify like.** Save a song with **Ctrl+Shift+I** on Windows or **Command+Shift+I** on Mac, and it is added to your Spotify Liked Songs. Use the same command to remove a saved song. It works on the selected Spotify row or on the playing song when focus is in the player area. Saving albums or playlists and following artists or profiles use the matching Spotify library action; ApricotPlayer favorites remain separate.

Use **Shift+A** to mark or unmark individual songs, or **Ctrl+A / Command+A** to select a whole Spotify collection, including songs not yet loaded into the visible list. Individual songs can be excluded. Save a selection to Liked Songs or add it to a playlist. If some songs are already present, choose whether to add only new ones or add all. Large changes are processed in batches, with the result reported when they finish.

For playlists you can edit, context menus provide creation, renaming, description editing, adding and removing tracks, moving tracks, and changing Public/Private visibility. Adding or removing a public playlist from your profile is separate from its visibility. Follow or unfollow an artist from its row, a song's menu, or the playing song.

**Hide song**, also called dislike in shortcut settings, differs from removing a like. It hides or unhides a song in supported personalized contexts. Spotify does not provide a universal dislike action for every track, so the option appears only where it applies.

### Spotify settings and limits

Choose Normal, High, or Very High streaming quality, volume normalization, autoplay behavior, and library sorting by recents, recently added, alphabetical order, creator, or custom order. Connection-related changes reconnect the active account.

**Listening-history reporting is experimental and off by default.** Enabling it asks for confirmation and explains the additional account risk. Spotify controls whether submitted records affect recommendations, On Repeat, or statistics. Browsing Spotify history does not require enabling ApricotPlayer's experimental reporting.

**Remember keys of played Spotify songs** is also off by default. It keeps song keys encrypted for your user account so previously played songs can start again without requesting a new key after restarting the app. Turning it off deletes the saved keys. This is not offline music downloading.

Spotify can temporarily limit how quickly an account starts new songs, especially while skipping rapidly. ApricotPlayer announces the limit, waits, and starts the selected song when allowed. Catalog results, mixes, previews, lyrics, Smart Shuffle, and audiobook access depend on Spotify, your account, and your region.

Spotify content cannot be downloaded, converted, or exported as clips. The librespot engine is updated with ApricotPlayer; it is not a separately replaceable component like yt-dlp. ApricotPlayer is an independent project and is not an official Spotify client.

## Local files and folders

Open a single file or a folder of audio and video. Choose a track and move through the collection with Previous and Next. Local items can use the general playback queue and the same speed, pitch, output-device, equalizer, bookmark, and clip tools where the format permits them.

On Windows, the installer and Settings register supported file types. On Mac, use the default-player option and macOS file-opening controls. Opening an associated media file brings it into ApricotPlayer. Opening ApricotPlayer again restores the existing window rather than starting another independent session.

## Podcasts and RSS

Search the Apple podcast directory with a configurable country and result limit, browse its podcast categories, or add an RSS/Atom feed directly. Browse episodes, play one, download individually or as a feed batch, queue episodes, add them to playlists, copy links, or open them in your browser.

Organize saved feeds into your own categories and filter by category. Import subscriptions from an **OPML** file or export them for another podcast application. Feeds can also be removed individually.

Refresh feeds manually, at startup, or on an automatic interval. Choose how many episodes to show and load older episodes from the available feed archive. ApricotPlayer remembers progress and played state. Mark an episode played or unplayed, clear its progress, and choose or save a speed preset for a particular podcast. New-episode notices are kept in the Notification center.

RSS podcasts are separate from Spotify shows and episodes. Their progress, downloads, and saved feeds belong to ApricotPlayer.

## AudioVault

Sign in with your AudioVault account to search audio-described movies and TV shows or browse recent additions. Open a TV show to browse its available episodes, listen in the player, and download available audio. Download a show package without opening each episode separately. Episodes may need to be fetched or extracted into the cache before playing. Downloads are organized within the AudioVault part of your download folder.

Manage AudioVault credentials in Settings, sign out, or open the registration page if you need an account. Available titles, access, and links come from AudioVault rather than a catalog bundled with ApricotPlayer.

## Favorites, playlists, history, and bookmarks

**Favorites** keep items you want to find again. Add or remove them from supported item menus or the player. **ApricotPlayer playlists** organize supported local media, online links, YouTube items, and podcast episodes. Create a playlist, add or remove items, play it, or download supported entries. These local collections do not edit YouTube or Spotify playlists.

**History** keeps played and downloaded items, with a configurable size limit. Remove individual entries, clear history, or disable it.

**Bookmarks** save positions within an item. Add one while playing, give it a name, return to it later, rename or delete it, and copy a timestamped link where supported. The main-menu screen collects saved bookmarks; the player's bookmark command opens those for the current item.

The **general playback queue** holds what you want to hear next, rather than a permanently saved playlist. Add supported items from lists or the player, start an entry, remove it, or clear the queue. Automatic next-item playback follows playback settings. Spotify uses its own queue instead.

## Playback and audio controls

- Play or pause, seek by the configured small step, by one minute, or by ten minutes, and jump to the start or end. The small step defaults to five seconds.
- Move to the previous or next item, repeat playback, shuffle a collection, or continue with related YouTube videos.
- Adjust volume, its step and starting level, and toggle volume boost or bass boost.
- Change speed and pitch independently, choose the audio-processing mode, configure adjustment steps and held-key behavior, and reset both together.
- Check time, volume, format, or full media details without leaving the player.
- Check **BPM with B**, including Spotify. It analyzes audio around the current position, accounts for speed, and works while paused without moving playback. Analysis is an estimate and takes a moment.
- Choose an output device, remember it in Settings, and return to automatic output when a device is unavailable.
- Use full-screen video or keep video within the regular player. F11 toggles full screen; Escape leaves it, and Tab brings controls into reach.
- Choose whether playback starts paused, details are shown by default, and Playing/Paused and Playback finished messages are announced.
- Resume saved positions, enable gapless playback for supported media, and configure buffering, cache size/location, stream-format preference, and next-stream prefetching.

The stream-URL cache saves resolved addresses for quicker starts. It does not save whole videos for offline viewing, and expiring links still need refreshing. Buffering and downloads are separate features.

The window title follows the current media, including the artist for Spotify songs. The Details window lets you read and copy the information available for the item.

### Equalizer and ReplayGain

The ten-band equalizer covers 31 Hz, 62 Hz, 125 Hz, 250 Hz, 500 Hz, 1 kHz, 2 kHz, 4 kHz, 8 kHz, and 16 kHz. It includes factory presets and editable custom presets. Enable it globally or adjust it for the player session, name presets, import or export profiles, compare with A/B, and associate presets with output devices. Choose a 6, 12, 18, or 24 dB adjustment range and optional clipping protection.

ReplayGain uses track or album gain when the media provides the required tags. Spotify has its own normalization setting. Volume boost, bass boost, equalizer, and speed controls are available from the player.

## Downloads, clips, and conversion

### Downloading media

Download audio or video from supported online sources, individual results, playlists, channels, ApricotPlayer playlists, and podcast feeds. Mark results with **Shift+A**, then use batch download actions. This selection is separate from the playback queue.

Choose formats, audio quality or bitrate, maximum video height, output folder, and filename template. Audio quality includes best quality, fixed bitrates, and VBR choices. MP3 and MP4 are the default formats. Preserve playlist order, ask for a location each time, confirm downloads, avoid previously archived downloads, or keep downloads quiet.

Optional output includes thumbnails, descriptions, information JSON, subtitles, automatic subtitles, selected subtitle languages, embedded metadata, and cover art. Configure filename restrictions, rate, parallel fragments, and retries. Downloads default to **Downloads/ApricotPlayer**, with media-specific subfolders where applicable.

**Current downloads** shows progress and cancellation, including stopping remaining batch items. Completion can show a message, send a notification, or open the folder, according to Settings. Downloads also appear in History when enabled.

### Marked clips and local editing

Press **[** in the player to set a clip start and **]** to set its end. Press the same marker key again to remove it. **P** previews the range. Audio and video download commands export the marked section when the source supports clip export.

For local files, **E** enables edit mode. Apply speed, pitch, and equalizer changes to a new copy or explicitly replace the original. Saving a copy keeps the source file unchanged. Use marked-clip export when you want only a section. Spotify streams cannot be exported or edited into files.

### File and folder converter

Convert one file or a folder, optionally including subfolders. Choose an output location and whether to replace originals. When keeping originals, colliding output names get a number rather than overwriting another file. Conversion reports progress and offers cancellation. Audio-to-video conversion can use a dark background or a selected image.

Audio targets: **MP3, M4A, AAC, WAV, FLAC, OGG, Opus, WMA, AIFF, ALAC, AC3, and MP2**.

Video targets: **MP4, MKV, WebM, MOV, AVI, WMV, M4V, MPG, MPEG, FLV, 3GP, OGV, TS, M2TS, and ASF**.

The source and selected codec must be supported by the included FFmpeg build. Conversion works with local files; it does not provide Spotify downloads.

## Settings and languages

Settings covers **General, Customize main menu, Playback, Equalizer, Downloads, Library, Podcasts, Notifications, Cookies and network, AudioVault, Spotify, and Keyboard shortcuts**. Reset a section or restore settings defaults. Shortcut editing checks conflicts, and changed shortcuts appear in labels when those labels are enabled.

General settings include language, result limits, direct-link behavior, update checks/channel, and system integration. On Windows, close to the system tray or start with Windows. Mac provides menu-bar and start-at-login behavior. The tray/menu-bar menu restores the window and provides settings, subscription checks, and Exit. Closing to the tray/menu bar differs from quitting.

The interface is available in **English, Slovenian, German, French, Spanish, Portuguese, Italian, Polish, Dutch, Swedish, Croatian, Serbian, Czech, Slovak, Hungarian, Romanian, Turkish, Ukrainian, Russian, Japanese, Korean, Simplified Chinese, Arabic, Hindi, Indonesian, Finnish, and Greek**. Missing translations fall back to English.

## Updates and your data

Check for application updates automatically at a configurable interval or manually from Settings. Choose **Stable** or **Beta**; stable users are not automatically moved onto prereleases. Review an update's version and notes, install it, or skip it. The updater checks packages and keeps recovery information so a failed replacement can be rolled back. Updating the application keeps your saved user data.

**yt-dlp** has its own automatic startup check and manual update action. Application and yt-dlp updates are separate. The Spotify engine and other bundled components are delivered with application packages.

Published packages use these per-user data folders:

- **Windows:** `%APPDATA%\ApricotPlayer`.
- **Mac:** `~/Library/Application Support/ApricotPlayer`.

Settings, favorites, playlists, history, bookmarks, subscriptions, feeds, and positions live outside application files. Mac caches and logs use **Library/Caches** and **Library/Logs**. Separate development builds named **ApricotPlayer 2 Beta** use **ApricotPlayer2Beta** folders; a public prerelease is not necessarily a separate-profile build.

The portable Windows ZIP uses the user-data folder too. Back up that folder before moving computers, not just the app. Local paths, devices, and credentials are tied to the original machine.

On Mac, **Import Windows data** in Settings accepts a Windows data folder or ZIP of it. It backs up current Mac data, imports supported saved data, resets machine-specific paths/devices, and quits so the import is read next time. Windows-protected Spotify and AudioVault credentials do not transfer; sign in again. Copying playlists does not copy their media files.

## Keyboard shortcuts

**These are the default shortcuts. You can change the shortcuts for ApricotPlayer actions in Settings > Keyboard shortcuts, including clearing a shortcut or choosing another combination.** Standard text-editing, focus-navigation, operating-system, and screen-reader keys follow their own controls and settings. Menu labels and Action Finder show your current bindings.

Actions apply where available. List commands use the selection; player commands use the current media when focus is in the player area. Text fields keep normal typing. Downloads, chapters, lyrics, transcripts, and similar tools depend on the source. One-letter player shortcuts do not replace first-letter navigation in menus.

### Windows shortcuts

#### Windows navigation and background control

- **Ctrl+Alt+M**: Open the main menu.
- **Ctrl+Alt+Y**: Open YouTube / SoundCloud search.
- **Ctrl+Alt+A**: Open AudioVault.
- **Ctrl+Alt+O**: Choose a folder to play.
- **Ctrl+Alt+I**: Choose a file to play.
- **Ctrl+Alt+L**: Open Direct link.
- **Ctrl+Alt+F**: Open Favorites.
- **Ctrl+Alt+K**: Open all saved bookmarks.
- **Ctrl+Alt+P**: Open ApricotPlayer playlists.
- **Ctrl+Alt+B**: Open YouTube subscriptions.
- **Ctrl+Alt+D**: Open Current downloads.
- **Ctrl+Alt+H**: Open History.
- **Ctrl+Alt+R**: Open Podcasts / RSS.
- **Ctrl+Alt+S**: Open Settings.
- **Ctrl+Shift+J**: Open Action Finder.
- **Ctrl+Space**: Play or pause the background player, also when ApricotPlayer is not the active app.
- **Ctrl+Alt+Shift+D**: Copy a diagnostic report.
- **Ctrl+Alt+Q**: Open the general playback queue.

#### Windows lists and item actions

- **Ctrl+Shift+A**: Download audio, or export the marked clip as audio when supported.
- **Ctrl+Shift+D**: Download video, or export the marked clip as video when supported.
- **Ctrl+Shift+S**: Subscribe to the selected YouTube channel.
- **Ctrl+Shift+U**: Unsubscribe from the selected YouTube channel.
- **Ctrl+Shift+O**: Open the YouTube channel of the selected or playing item.
- **Shift+A**: Mark or unmark a result for batch downloads, or a Spotify song for bulk actions.
- **Ctrl+Alt+Left**: Read the previous result-detail column.
- **Ctrl+Alt+Right**: Read the next result-detail column.
- **Ctrl+Shift+Q**: Add the selected or playing item to the appropriate playback queue.
- **Ctrl+Shift+Delete**: Remove an item from the playback queue.
- **Ctrl+Shift+N**: Create a playlist in the relevant playlist screen.
- **Ctrl+F**: Add the selected or playing item to ApricotPlayer Favorites.
- **Ctrl+Shift+F**: Remove the item from ApricotPlayer Favorites.
- **Ctrl+P**: Add the selected or playing item to a playlist.
- **Ctrl+Shift+P**: Remove the selected item from its playlist.
- **Ctrl+L**: Copy the selected item link, or its path for local media.
- **Ctrl+D**: Copy the resolved stream URL, where available.
- **Applications**: Open the context menu.
- **Enter**: Open or play the selected item, or activate the focused control.
- **Ctrl+Shift+V**: Open the Notification center.
- **Delete**: Remove the selected item where removal is available.

#### Windows podcast actions

- **Ctrl+Shift+X**: Mark a podcast episode played or unplayed.
- **Ctrl+Shift+R**: Clear the saved progress for a podcast episode.
- **Ctrl+Shift+E**: Save the current speed as a preset for this podcast.

#### Windows player

- **L**: Copy the playing item link or local file path.
- **Ctrl+Shift+L**: Copy a link at the current playback time, where supported.
- **Space**: Play or pause.
- **T**: Announce elapsed time and duration.
- **B**: Analyze and announce BPM around the current position, including Spotify.
- **S**: Decrease playback speed.
- **D**: Increase playback speed.
- **Ctrl+0**: Reset speed and pitch together.
- **Ctrl+Up**: Raise pitch.
- **Ctrl+Down**: Lower pitch.
- **V**: Announce the current volume.
- **F**: Announce the current media format.
- **F7**: Open media details.
- **O**: Choose an audio output device.
- **F4**: Open the equalizer.
- **F11**: Toggle full-screen video.
- **Ctrl+Shift+G**: Choose the ReplayGain mode.
- **Ctrl+Shift+B**: Add a bookmark at the current position.
- **Ctrl+Shift+K**: Open bookmarks for the playing item.
- **Ctrl+Shift+C**: Open chapters.
- **Ctrl+Shift+T**: Open the transcript or available subtitles.
- **Ctrl+Shift+Y**: Open lyrics.
- **Ctrl+Shift+M**: Open YouTube comments.
- **Alt+Left**: Go to the previous chapter.
- **Alt+Right**: Go to the next chapter.
- **E**: Toggle local-file edit mode.
- **Ctrl+S**: Save edited local media as a new copy.
- **Ctrl+R**: Replace the original local file with the edited version.
- **[**: Set or remove the clip start marker.
- **]**: Set or remove the clip end marker.
- **P**: Preview the marked clip.
- **Ctrl+PageUp**: Play the previous item; on Spotify, restart the current track first when appropriate.
- **Ctrl+PageDown**: Play the next item.
- **Ctrl+Shift+PageDown**: Play the next related YouTube video.
- **Escape**: Leave full screen, or go back from the player; background playback follows Settings.
- **F2**: Toggle volume boost.
- **F3**: Toggle bass boost.
- **R**: Toggle repeat; on Spotify, cycle off, repeat collection, and repeat track.
- **Shift+S**: Toggle shuffle; on supported Spotify playlists, cycle off, shuffle, and Smart Shuffle.
- **Left**: Seek backward by the configured small step, five seconds by default.
- **Right**: Seek forward by the configured small step, five seconds by default.
- **Ctrl+Left**: Seek backward one minute.
- **Ctrl+Right**: Seek forward one minute.
- **Ctrl+Shift+Left**: Seek backward ten minutes.
- **Ctrl+Shift+Right**: Seek forward ten minutes.
- **Ctrl+Home**: Seek to the start.
- **Ctrl+End**: Seek to the end.
- **Up**: Increase volume.
- **Down**: Decrease volume.

#### Windows Spotify

- **Ctrl+Alt+C**: Open the Spotify menu.
- **Ctrl+Alt+Shift+C**: Open Spotify Accounts.
- **Ctrl+Alt+Shift+Y**: Open Spotify Search.
- **Ctrl+Alt+Shift+L**: Open Spotify My library.
- **Ctrl+Alt+Shift+F**: Open Spotify Liked songs.
- **Ctrl+Alt+Shift+P**: Open Spotify Playlists.
- **Ctrl+Alt+Shift+Q**: Open the Spotify queue.
- **Ctrl+Alt+Shift+O**: Open Spotify Connect devices.
- **Ctrl+Shift+I**: Like or unlike a Spotify song, save or remove supported content, or follow or unfollow an artist/profile.
- **Ctrl+Shift+H**: Hide or unhide a Spotify song in a supported personalized context.
- **Ctrl+Alt+Shift+M**: Open Spotify Daily mixes.
- **Ctrl+Alt+Shift+R**: Start Spotify radio from a supported item.
- **Ctrl+Shift+PageUp**: Play the first track of the current Spotify list in its original order.
- **Ctrl+Shift+PageDown**: Play the last track of the current Spotify list in its original order.

#### Windows navigation and selection

- **Tab**: Move to the next control, including the player area during background playback.
- **Shift+Tab**: Move to the previous control.
- **Up / Down in a list**: Select the previous or next row.
- **Home / End in a list**: Move to the beginning or end; supported paginated lists can load further items.
- **PageUp / PageDown in a list**: Move through a page of rows.
- **A letter in a menu**: Move to an item beginning with that letter.
- **Ctrl+A in a Spotify collection**: Select all songs, including unloaded pages, for bulk actions.
- **Escape in a screen or dialog**: Go back or close that screen or dialog.
- **Shift+F10**: Open the context menu, as an alternative to Applications.

### Mac shortcuts

**Command** is the Mac primary modifier; **Option** is also called Alt. The exceptions to Windows combinations are listed explicitly, including background playback, Current downloads, Add to playback queue, and Spotify Queue. On compact keyboards, PageUp, PageDown, Home, and End may need Fn with an arrow key. Function keys may need Fn too, depending on your keyboard settings. VO means your configured VoiceOver modifier.

#### Mac navigation and background control

- **Command+Option+M**: Open the main menu.
- **Command+Option+Y**: Open YouTube / SoundCloud search.
- **Command+Option+A**: Open AudioVault.
- **Command+Option+O**: Choose a folder to play.
- **Command+Option+I**: Choose a file to play.
- **Command+Option+L**: Open Direct link.
- **Command+Option+F**: Open Favorites.
- **Command+Option+K**: Open all saved bookmarks.
- **Command+Option+P**: Open ApricotPlayer playlists.
- **Command+Option+B**: Open YouTube subscriptions.
- **Command+Option+J**: Open Current downloads.
- **Command+Option+H**: Open History.
- **Command+Option+R**: Open Podcasts / RSS.
- **Command+Option+S**: Open Settings.
- **Command+Shift+J**: Open Action Finder.
- **Command+Shift+Space**: Play or pause the background player, also when ApricotPlayer is not the active app.
- **Command+Option+Shift+D**: Copy a diagnostic report.
- **Command+Option+Q**: Open the general playback queue.

#### Mac lists and item actions

- **Command+Shift+A**: Download audio, or export the marked clip as audio when supported.
- **Command+Shift+D**: Download video, or export the marked clip as video when supported.
- **Command+Shift+S**: Subscribe to the selected YouTube channel.
- **Command+Shift+U**: Unsubscribe from the selected YouTube channel.
- **Command+Shift+O**: Open the YouTube channel of the selected or playing item.
- **Shift+A**: Mark or unmark a result for batch downloads, or a Spotify song for bulk actions.
- **Command+Option+Left**: Read the previous result-detail column.
- **Command+Option+Right**: Read the next result-detail column.
- **Command+Shift+Return**: Add the selected or playing item to the appropriate playback queue.
- **Command+Shift+Delete**: Remove an item from the playback queue.
- **Command+Shift+N**: Create a playlist in the relevant playlist screen.
- **Command+F**: Add the selected or playing item to ApricotPlayer Favorites.
- **Command+Shift+F**: Remove the item from ApricotPlayer Favorites.
- **Command+P**: Add the selected or playing item to a playlist.
- **Command+Shift+P**: Remove the selected item from its playlist.
- **Command+L**: Copy the selected item link, or its path for local media.
- **Command+D**: Copy the resolved stream URL, where available.
- **Shift+F10**: Open the context menu.
- **Return**: Open or play the selected item, or activate the focused control.
- **Command+Shift+V**: Open the Notification center.
- **Delete**: Remove the selected item where removal is available.

#### Mac podcast actions

- **Command+Shift+X**: Mark a podcast episode played or unplayed.
- **Command+Shift+R**: Clear the saved progress for a podcast episode.
- **Command+Shift+E**: Save the current speed as a preset for this podcast.

#### Mac player

- **L**: Copy the playing item link or local file path.
- **Command+Shift+L**: Copy a link at the current playback time, where supported.
- **Space**: Play or pause.
- **T**: Announce elapsed time and duration.
- **B**: Analyze and announce BPM around the current position, including Spotify.
- **S**: Decrease playback speed.
- **D**: Increase playback speed.
- **Command+0**: Reset speed and pitch together.
- **Command+Up**: Raise pitch.
- **Command+Down**: Lower pitch.
- **V**: Announce the current volume.
- **F**: Announce the current media format.
- **F7**: Open media details.
- **O**: Choose an audio output device.
- **F4**: Open the equalizer.
- **F11**: Toggle full-screen video.
- **Command+Shift+G**: Choose the ReplayGain mode.
- **Command+Shift+B**: Add a bookmark at the current position.
- **Command+Shift+K**: Open bookmarks for the playing item.
- **Command+Shift+C**: Open chapters.
- **Command+Shift+T**: Open the transcript or available subtitles.
- **Command+Shift+Y**: Open lyrics.
- **Command+Shift+M**: Open YouTube comments.
- **Option+Left**: Go to the previous chapter.
- **Option+Right**: Go to the next chapter.
- **E**: Toggle local-file edit mode.
- **Command+S**: Save edited local media as a new copy.
- **Command+R**: Replace the original local file with the edited version.
- **[**: Set or remove the clip start marker.
- **]**: Set or remove the clip end marker.
- **P**: Preview the marked clip.
- **Command+PageUp**: Play the previous item; on Spotify, restart the current track first when appropriate.
- **Command+PageDown**: Play the next item.
- **Command+Shift+PageDown**: Play the next related YouTube video.
- **Escape**: Leave full screen, or go back from the player; background playback follows Settings.
- **F2**: Toggle volume boost.
- **F3**: Toggle bass boost.
- **R**: Toggle repeat; on Spotify, cycle off, repeat collection, and repeat track.
- **Shift+S**: Toggle shuffle; on supported Spotify playlists, cycle off, shuffle, and Smart Shuffle.
- **Left**: Seek backward by the configured small step, five seconds by default.
- **Right**: Seek forward by the configured small step, five seconds by default.
- **Command+Left**: Seek backward one minute.
- **Command+Right**: Seek forward one minute.
- **Command+Shift+Left**: Seek backward ten minutes.
- **Command+Shift+Right**: Seek forward ten minutes.
- **Command+Home**: Seek to the start.
- **Command+End**: Seek to the end.
- **Up**: Increase volume.
- **Down**: Decrease volume.

#### Mac Spotify

- **Command+Option+C**: Open the Spotify menu.
- **Command+Option+Shift+C**: Open Spotify Accounts.
- **Command+Option+Shift+Y**: Open Spotify Search.
- **Command+Option+Shift+L**: Open Spotify My library.
- **Command+Option+Shift+F**: Open Spotify Liked songs.
- **Command+Option+Shift+P**: Open Spotify Playlists.
- **Command+Option+Shift+U**: Open the Spotify queue.
- **Command+Option+Shift+O**: Open Spotify Connect devices.
- **Command+Shift+I**: Like or unlike a Spotify song, save or remove supported content, or follow or unfollow an artist/profile.
- **Command+Shift+H**: Hide or unhide a Spotify song in a supported personalized context.
- **Command+Option+Shift+M**: Open Spotify Daily mixes.
- **Command+Option+Shift+R**: Start Spotify radio from a supported item.
- **Command+Shift+PageUp**: Play the first track of the current Spotify list in its original order.
- **Command+Shift+PageDown**: Play the last track of the current Spotify list in its original order.

#### Mac navigation and selection

- **Tab**: Move to the next control, including the player area during background playback.
- **Shift+Tab**: Move to the previous control.
- **Up / Down in a list**: Select the previous or next row.
- **Home / End in a list**: Move to the beginning or end; supported paginated lists can load further items.
- **PageUp / PageDown in a list**: Move through a page of rows.
- **A letter in a menu**: Move to an item beginning with that letter.
- **Command+A in a Spotify collection**: Select all songs, including unloaded pages, for bulk actions.
- **Escape in a screen or dialog**: Go back or close that screen or dialog.
- **VO+Space**: Activate the row under the VoiceOver cursor.
- **VO+Shift+M**: Open that row's context menu.

## Help and bug reports

Read the [changelog](https://github.com/Urh2006/ApricotPlayer/blob/main/CHANGELOG.md) and [release notes](https://github.com/Urh2006/ApricotPlayer/releases) for changes and release-specific information.

Report bugs or suggest improvements in [GitHub Issues](https://github.com/Urh2006/ApricotPlayer/issues). Include the app version, operating system, source involved, steps, and expected result. For accessibility issues, include your screen reader/version, focus location, and what was announced. A diagnostic report can help; review it before sharing and remove private information. Do not include passwords, cookies, or account tokens.

For security problems, use the private reporting route in [SECURITY.md](SECURITY.md).

## Source code and development

The stable source is on **main**, with the Rust workspace in **rust/**. Future prereleases are prepared on **beta**. Version 2.0.0 is the stable release of the rewrite completed through beta 26.

For Rust development, use main in your existing clone and use the [pinned toolchain](https://github.com/Urh2006/ApricotPlayer/blob/main/rust/rust-toolchain.toml). The workspace separates shared application logic, playback, storage, Spotify, updater, and native interfaces. Packaging includes external libraries and helpers; a plain Cargo executable is not the complete distribution.

From **rust/**, the regular checks are:

```text
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked --no-fail-fast
```

Build prerequisites and options are documented in the [Windows packaging script](https://github.com/Urh2006/ApricotPlayer/blob/main/rust/scripts/build_stable_release.ps1) and [Mac packaging script](https://github.com/Urh2006/ApricotPlayer/blob/main/rust/scripts/build_macos_app.sh).

The [Rust rewrite plan](https://github.com/Urh2006/ApricotPlayer/blob/main/docs/RUST_REWRITE_PLAN.md), [parity manifest](https://github.com/Urh2006/ApricotPlayer/blob/main/docs/RUST_PARITY_MANIFEST.md), and [Mac port plan](https://github.com/Urh2006/ApricotPlayer/blob/main/docs/MACOS_PORT_PLAN.md) record architecture and acceptance requirements. Some sections retain planning history; current source and release notes show what is shipped.

The earlier Python source remains for reference; 1.0.21 was the last Python release. Its entry point is **wx_main.py**, using Python 3.11+, wxPython, and **requirements.txt**. It does not run the Rust 2.0 interface.

Translations live in **apricot/locales/**, with Rust-specific text in **rust/crates/apricot-app/locales/rust_strings.json** on main. Preserve placeholders such as `{title}` and keep control names, messages, and shortcut labels consistent when contributing translations.
