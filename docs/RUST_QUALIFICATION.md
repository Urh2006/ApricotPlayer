# Rust 2.0 Qualification Log

This file records reproducible evidence for rewrite phase gates. Passing a spike
or an intermediate gate does not claim product parity. The complete acceptance
contract remains `docs/RUST_PARITY_MANIFEST.md`.

## 2026-09-02: Phase 1 compatibility and local distribution

Harnesses:

- the full Cargo workspace test and Clippy suites;
- `rust/scripts/qualify_python_settings_compat.ps1`;
- `rust/scripts/qualify_python_data_compat.ps1`;
- `rust/scripts/qualify_local_beta_scripts.ps1`.

Verified:

- exact registries for 117 settings, 91 actions, 19 customizable main-menu
  items, 27 languages, 10 EQ bands, and the frozen preset catalog;
- all 63 screens, 28 routes, and 17 context menus have stable typed IDs and
  machine-checked references;
- all 116 current Python settings load into the typed Rust schema and round-trip
  without changing or losing existing or unknown fields;
- all existing serialized Python profile artifacts round-trip from a private
  temporary copy, with semantic JSON equality and byte-exact text preservation;
- existing Windows JSON with a UTF-8 BOM is imported without weakening malformed
  JSON or wrong-shape rejection;
- each compatibility artifact is bounded to 64 MiB before parsing;
- local release builds carry exact version, commit, dirty-tree, timestamp, Rust,
  data-schema, identity, and update-channel metadata plus a SHA-256 file
  manifest;
- every local beta package and installation includes the exact bundled mpv
  executable, its required D3D compiler, and the NVDA Controller Client;
- a changed package is rejected before installation;
- side-by-side install and reinstall are transactional, remove stale package
  files, and never touch the Python installation;
- default uninstall preserves `%APPDATA%\ApricotPlayer2Beta`, while explicit
  `-RemoveData` removes only that separately validated beta path.

Automated results:

- `PYTHON_SETTINGS_COMPAT=PASS`;
- `PYTHON_DATA_COMPAT=PASS`;
- `LOCAL_BETA_SCRIPTS=PASS`;
- all workspace tests and strict Clippy checks pass.

The Phase 1 machine gate is satisfied. This does not close the manual NVDA and
real-device work still listed under Phase 0, and the current foundation binary
does not yet claim an implemented product UI.

## 2026-09-01: frozen Python launch baseline

Harness: `rust/scripts/measure_python_baseline.ps1`. It starts Python 1.0.21 in
an isolated temporary app-data profile, follows the virtual-environment launcher
to the real GUI child process, and stops timing only after UI Automation can see
the accessible `Main menu` List. It refuses to run while another ApricotPlayer
instance is active, so the product's single-instance dialog cannot contaminate
the sample.

Five consecutive measurements on the development computer:

- launch median: 1007.18 ms;
- launch p95: 1056.81 ms;
- working-set median after one idle second: 65.30 MiB;
- private-memory median: 35.08 MiB;
- idle CPU median: 0.13 percent of total logical CPU capacity.

The five observed launch values were 960.26, 1056.81, 1007.18, 994.56, and
1026.38 ms. This harness includes process-tree and UIA polling overhead, so Rust
must be measured through the same acceptance point rather than compared with a
bare `main()` timer.

Still required:

- cold-cache measurement;
- action, route, playback-start, large-list, active playback, download, and
  long-session resource baselines described in `docs/RUST_REWRITE_PLAN.md`.

## 2026-09-01: native Windows accessibility

Harness:

- `rust/tools/windows-accessibility-spike`
- `rust/scripts/qualify_windows_accessibility_spike.ps1`

Verified with Windows UI Automation against native Win32/Common Controls:

- list and list-item roles;
- checkbox role, state, and Toggle pattern;
- combobox role and items;
- slider role, value 50, and range 0 through 100;
- read-only multi-line Document control;
- Button and ProgressBar roles;
- deterministic Tab traversal, including leaving the read-only field;
- accessible status-name update plus `EVENT_OBJECT_NAMECHANGE`;
- standard modal, context-menu, tray, and child video-host HWND construction in
  the spike.

Automated result: `ACCESSIBILITY_SPIKE=PASS`.

Still required before the Phase 0 gate can close:

- a manual NVDA pass for spoken names, roles, values, states, braille, duplicate
  announcements, modal focus restoration, Applications key, and tray behavior;
- a real embedded mpv video check using the child HWND;
- Narrator smoke; JAWS remains a later product acceptance smoke.

## 2026-09-01: mpv JSON IPC

Harness: `rust/tools/mpv-ipc-spike` using the repository's bundled mpv and a
generated two-second stereo WAV fixture.

Verified:

- bounded Windows named-pipe open, write, peek, read, request-ID matching, and
  timeout path;
- unsolicited event and unrelated-response filtering;
- real mpv startup and asynchronous property readiness;
- volume, speed, pitch, and exact seek commands;
- the Apricot-labelled lavfi EQ chain with clipping headroom and limiter;
- audio-device enumeration;
- clean quit.

The same harness now also exercises the production `MpvProcessEngine` boundary:

- mpv starts idle with the target volume and volume maximum already present in
  its process arguments, before any media can produce audio;
- a bounded asynchronous event connection receives file-loaded, pause,
  position, duration, and end-of-file events without polling from the UI;
- the command connection concurrently accepts load, volume, speed, pitch, and
  exact-seek operations;
- command requests are serialized and shutdown has a bounded graceful-to-kill
  fallback.

Observed mpv: `mpv v0.41.0-744-g304426c39`.

Automated result: `MPV_IPC_SPIKE=PASS`.
Production engine result: `MPV_PROCESS_ENGINE=PASS`.

Still required before the Phase 0 gate can close:

- embedded video, fullscreen, audible output-device switching, gapless behavior,
  all speed modes, held seek, and real shutdown/crash/restart checks;
- latency comparison with the frozen Python implementation.

## 2026-09-04: libmpv backend selection

Harness: `rust/tools/libmpv-spike`, using a dynamically loaded shinchiro
Windows libmpv development build and generated local audio/video fixtures.
`rust/scripts/prepare_libmpv.ps1` pins the 2026-09-03 x86-64 development
archive and verifies both archive and extracted-DLL SHA-256 before use.

Verified through both the raw C ABI and the production `LibMpvEngine` adapter:

- lazy DLL load, client creation, initialization, and clean destruction;
- repeated `loadfile replace` on one client instance;
- exact seek, volume, speed, pitch, and the labelled EQ plus limiter chain;
- native child-HWND video output after `video-reconfig`;
- audio playback does not force eager GPU-window initialization;
- production event projection for file-loaded, pause, position, duration,
  end-of-file, errors, shutdown, and event-queue overflow;
- runtime generation filtering and one engine per genuinely open player
  session, including replacement and close/reopen tests.

Across ten warm debug runs, median observations were:

- libmpv initialization: 18.306 ms;
- libmpv first load: 30.659 ms;
- libmpv second load on the same instance: 29.024 ms;
- external mpv initialization: 44.283 ms;
- external mpv first load: 42.752 ms;
- external mpv second load on the same process: 34.668 ms.

Automated results: `LIBMPV_SINGLE_HANDLE=PASS`,
`LIBMPV_EMBEDDED_HWND=PASS`, and `LIBMPV_PRODUCTION_ENGINE=PASS`.

The default Rust runtime now selects libmpv. The process/JSON-IPC adapter stays
available as a qualification and emergency fallback because in-process native
media code has a larger crash blast radius. Audible device switching, network
streaming, malformed-media recovery, fullscreen transitions, and long soak
tests remain required before release.

## 2026-09-01: media helper processes

Harness: `rust/tools/media-process-spike`. The yt-dlp extraction fixture is
served from an isolated localhost HTTP server, so this gate does not depend on a
live website.

Verified:

- yt-dlp startup, disabled external plugin directories, direct-media extraction,
  bounded output, and JSON parsing;
- Node.js startup;
- ffprobe JSON duration probing;
- FFmpeg decoding to a null sink;
- hidden child-process creation on Windows.

Observed tools:

- yt-dlp `2026.08.19`;
- Node.js `v24.18.0`;
- repository FFmpeg and ffprobe builds.

One debug qualification run measured approximately:

- yt-dlp version startup: 659 ms;
- yt-dlp localhost direct extraction: 1664 ms;
- ffprobe: 299 ms;
- FFmpeg decode: 74 ms.

These are single-run qualification observations, not release performance claims.
Cold/warm repetitions and Python differential measurements remain required.

Automated result: `MEDIA_PROCESS_SPIKE=PASS`.

## 2026-09-04: optional Rust YouTube component

Implementation: `rust/apps/apricot-youtube-helper`, using `rusty_ytdl` pinned
to commit `b1c6eb7c83f0d6189f256ed5df50019a5803c734`.

Verified:

- a versioned newline-delimited JSON protocol with one MiB request and response
  limits, correlated request IDs, explicit errors, and no secret echo;
- one helper process handles handshake, configuration, search, resolve, and
  shutdown commands across a persistent session;
- a bounded background runtime keeps all helper I/O off the UI thread, tags
  responses with generation IDs, replays the latest session configuration only
  after a transport restart, and applies a 45-second network-operation limit;
- anonymous live YouTube search returned three requested results;
- a live resolve of `jNQXAC9IVRw` returned the expected title and 15 available
  formats, including typed track and transport information;
- the local package includes the helper, records its exact backend revision,
  and runs a protocol handshake through the production process client after
  installation;
- GUI-subsystem qualification uses a hidden, waited process and exit codes, so
  it cannot close stdout early or race the following reinstall step;
- the combined updater policy checks yt-dlp and the Rust helper independently,
  while local-only builds still forbid remote installation.

The Rust backend remains experimental and yt-dlp remains the compatibility
default until search, pagination, cookies, playback, downloads, live streams,
and recovery behavior pass the full Phase 5 matrix.

## 2026-09-04: default standalone yt-dlp backend

Implementation: `rust/crates/apricot-platform/src/ytdlp_youtube.rs`, using the
official standalone `yt-dlp.exe` release `2026.08.19`. The preparation script
pins SHA-256
`66674953fe251b89f4d08c5f0e35e0728679bd67ab3d7d05c0562af101dd3e7a`.

Verified:

- the executable is downloaded only from the exact official GitHub release URL,
  hash-verified, staged atomically, and recorded in package metadata;
- every invocation uses structured arguments without a shell, disables user
  configuration and external plugin directories, and starts hidden on Windows;
- stdout and stderr are drained concurrently with independent hard limits, a
  45-second timeout, and forced process termination on timeout;
- search JSON preserves mixed video, live-stream, channel, and playlist types,
  while resolved formats preserve audio/video tracks, transport, dimensions,
  frame rate, bitrate, and preference ordering;
- cookie-file and proxy configuration is validated and error text redacts their
  values; the cookie file is reread by each new invocation;
- a live anonymous search returned results and a live resolve of
  `jNQXAC9IVRw` returned the expected title and playable formats;
- the local package and installed copy contain the pinned executable, and the
  app exercises it through the production adapter during qualification.

Deterministic parser, bounds, backend-selection, and redaction tests pass. The
live test is ignored by the normal workspace suite and is run explicitly with
`APRICOT_YTDLP`, so offline builds remain reproducible.

## 2026-09-08: typed YouTube collection backend

Implementation: protocol version 4 collection commands shared by the yt-dlp
adapter, the optional Rust helper, and `YoutubeSearchService`.

Verified:

- playlist videos, channel videos, channel playlists, channel live streams, and
  channel popular videos have distinct typed requests instead of URL-shape
  guesses in the UI;
- channel tab URLs are normalized without retaining an old tab, query, or
  fragment, while playlist URLs retain their collection identity;
- the yt-dlp backend advertises playlist and channel collection capabilities;
  the optional Rust backend advertises only its implemented playlist capability
  and returns an explicit non-retryable error for unsupported channel requests;
- filtered searches fetch a bounded internal cushion before applying the
  caller's visible limit, preventing a pinned channel from making a one-result
  playlist search appear empty;
- configuration always completes before a collection request, and client tokens
  survive the service's separate internal request generations;
- bounded UI requests and explicit complete-collection requests are separate
  protocol operations, so whole-playlist playback has no invented numeric cap;
- an explicit live test searched YouTube, resolved `jNQXAC9IVRw`, discovered a
  current public playlist from a typed playlist search, and read three playlist
  entries through the new collection command.

The normal workspace suite remains network-independent. The live collection
test is ignored unless run explicitly with `APRICOT_YTDLP`.

## 2026-09-08: nested YouTube collection navigation

Implementation: `YoutubeCollectionController`, collection-owned playback
sequences, and the native Win32 playlist/channel result views.

Verified:

- playlist and channel collections are pushed onto an independent navigation
  stack, so opening a nested playlist does not replace its parent collection or
  the original search session;
- Enter on a playlist opens its videos, while Enter on a channel opens one
  accessible choice dialog ordered Videos, Channel playlists, Live streams,
  and Popular videos;
- the channel dialog initially focuses its four-item list, not the Open or
  Cancel button;
- a packaged live search for `OpenAI` opened a 19-item playlist search, entered
  The OpenAI Podcast, displayed 19 videos, and returned to the original
  playlist at selection zero;
- the same packaged build opened the official OpenAI channel's video tab with
  20 video rows, opened its channel-playlist tab, entered the nested ChatGPT
  Images playlist, and returned one level at a time without losing the parent
  selection;
- closing the player after opening the third nested playlist video restored the
  same collection with selection two and the same selected title;
- selecting the final row of a 20-item channel-playlist page loaded 20 more
  rows cumulatively while retaining selection 19;
- playback sequences bind to the active collection generation, and a Next at
  the loaded boundary requests another cumulative page before selecting the
  exact next playable item;
- selecting the optional Rust backend still routes unsupported channel tabs to
  yt-dlp explicitly, while playlist collections remain available through the
  Rust helper;
- playlist rows expose Play playlist, Shuffle playlist, Open playlist videos,
  the correct favorite toggle, and Copy link in Python order; channel rows
  expose Channel options plus direct Videos, Popular videos, Channel playlists,
  Live streams, favorite, and Copy link actions;
- Play playlist loads the complete collection before creating its exact
  Previous/Next sequence, while Shuffle playlist contains every playable item
  once and only changes the initial order;
- packaged Play playlist advanced from OpenAI Agent Builder Course item 1 to
  item 2 and back exactly, then closing the player restored the original
  19-item result list at selection zero; packaged Shuffle playlist started a
  different valid member;
- YouTube was confirmed to ignore the legacy `sort=p` URL in current yt-dlp.
  Popular videos now scans the complete flat channel once, sorts globally by
  numeric view count, and caches that result for cumulative 20/40/60 display;
- an explicit live OpenAI-channel test verified descending all-time view counts
  and that a ten-item request preserves the exact five-item prefix from the
  preceding request;
- the installed package displayed OpenAI's first four Popular rows at 37, 15,
  14, and 11 million views in descending order; moving from the twentieth row
  to a 40-row cumulative projection used the cached scan in about 79 ms and
  retained selection 19.

Automated results: app 108 tests, core 33 tests, media 9 tests, platform 26
tests plus two ignored live tests, playback 16 tests, storage 47 tests, Windows
UI 17 tests, updater 2 tests, YouTube helper 6 tests, and full workspace Clippy
with warnings denied.

The packaged controls were inspected through Win32 and Windows UI Automation
because native-app computer use was unavailable in this task. Still required:
physical Escape/Enter plus NVDA and Narrator announcement checks, and production
metadata hydration for upload times.

## 2026-09-06: Python-compatible user playlists

Implementation: `UserPlaylistFile`, `UserPlaylistController`, application-owned
playlist state, and native Win32 playlist/list-item views and dialogs.

Verified:

- `playlists.json` is loaded from the isolated Rust beta data directory with a
  read-only fallback to the stable Python file, and the first change writes only
  to the beta directory;
- playlist and item metadata unknown to Rust survive typed load/save, including
  the original Python AudioVault kind aliases;
- malformed current data blocks mutation instead of being silently replaced;
- create-with-current-item, add, remove, and whole-playlist queue operations are
  atomic, while duplicate durable locations are rejected;
- an individually selected playlist item is standalone, matching Python, while
  Play playlist and Shuffle playlist create an exact deterministic sequence;
- asynchronous direct-link resolution preserves a valid playlist sequence, and
  an explicit shuffle choice survives both fresh and continuing player sessions;
- the name edit starts blank and focused, the chooser starts on its list, Enter
  accepts, Escape closes from every control, and focus returns to the owner;
- list controls expose source-appropriate Copy link/Copy path behavior, and local
  items never expose Copy stream URL;
- a typed round trip of a private temporary copy of the real Python data passed
  recursive JSON value comparison.

Automated results: app 83 tests, storage 35 tests, Windows UI 12 tests, full
workspace Clippy with warnings denied, and `PYTHON_DATA_COMPAT=PASS`.

Per-item audio/video and whole-user-playlist video downloads now use the shared
download engine. Whole-playlist download excludes local files, keeps one safe
playlist folder, honors Ask every time, continues after child failures, and is
available from both playlist views and their context menus. Still required for
complete playlist parity: source-specific channel actions and manual
NVDA/Narrator dialog and navigation checks.

## 2026-09-07: Python-compatible playback bookmarks

Implementation: `BookmarkFile`, `BookmarkController`, application-owned
bookmark state, exact initial-position playback, and the native Win32 bookmark
dialog.

Verified:

- `bookmarks.json` loads from the isolated Rust beta data directory with a
  read-only fallback to the stable Python file; migration never writes to the
  stable installation;
- old top-level and current nested Python bookmark shapes normalize into one
  typed model, while unknown fields survive a load/save round trip;
- malformed individual entries are skipped like Python normalization, while a
  malformed current file blocks mutation instead of being silently replaced;
- add, rename, and delete persist atomically by durable bookmark id, and current
  item filtering keeps bookmarks independent by durable media identity;
- all-item and current-item lists match Python ordering and expose native list,
  button, Enter, double-click, Delete, Escape, and context-menu interaction;
- playing a bookmark for the current item uses an exact absolute seek; playing
  another local, direct, or YouTube item passes the initial position in the same
  load operation, including split video/audio streams;
- a persistent libmpv instance receives the bookmark start position only for
  the intended item; its next ordinary replacement explicitly receives no
  position, preventing timestamp leakage between files;
- closing the player opened from the global bookmark list returns to the
  bookmark dialog, and closing that dialog unwinds one further route.

Automated results: app 86 tests, playback 16 tests, storage 38 tests, Windows UI
12 tests, and full workspace Clippy with warnings denied.

Still required for complete bookmark parity: manual NVDA/Narrator dialog,
announcement, focus-return, context-menu, same-item seek, and cross-item resume
checks in the packaged build. General automatic resume and last-session restore
remain separate unfinished parity items.

## 2026-09-07: item-bound automatic playback resume

Implementation: `PlaybackPositionFile`, `PlaybackPositionController`,
application-owned position state, and Win32 replacement/close persistence.

Verified:

- `playback_positions.json` loads from the isolated Rust beta data directory
  with a read-only fallback to the stable Python file, and writes only to the
  beta directory;
- malformed current JSON or a non-object root blocks mutation without replacing
  the user's file, while valid objects preserve unrelated legacy, invalid, and
  future values losslessly when one known position changes;
- numeric JSON values and Python-compatible numeric strings are accepted only
  for the media item that owns the durable path or URL identity;
- positions below five seconds, positions within the final eight seconds, and
  live-stream positions are cleared according to the Python thresholds;
- disabling Resume playback prevents both restoration and persistence;
- the outgoing item's projected position is persisted before replacement and
  on a real player-session close, while navigating elsewhere with the same open
  player session does not reset it;
- automatic resume and explicit bookmark starts are applied in the same libmpv
  load operation, with an explicit bookmark position taking precedence;
- the projected and runtime initial position is attached to exactly one item,
  and the next ordinary local, direct, or YouTube item starts without inheriting
  it.

Automated results: app 92 tests, playback 16 tests, storage 40 tests, Windows UI
12 tests, full workspace Clippy with warnings denied, and
`PYTHON_DATA_COMPAT=PASS`.

Still required: packaged manual checks with NVDA/Narrator for close, replace,
near-end clearing, and stable-data migration. Last-player-session restoration
remains a separate unfinished parity item.

## 2026-09-07: Python-compatible last-player-session restoration

Implementation: `LastPlayerSessionFile`, an application-owned controller with
an ordered background writer, restored result/folder projections, and native
main-menu and Action finder activation.

Verified:

- `last_player_session.json` loads from the isolated Rust beta data directory
  with a read-only fallback to the stable Python snapshot, while all new writes
  target only the beta directory;
- current item, title, timestamp, Python return screen/data, unknown fields,
  and a maximum of 200 slim sequence items survive the typed boundary;
- large transient resolver fields such as formats, captions, thumbnails,
  entries, heatmaps, and comments are omitted from Rust snapshots like Python;
- a Python sequence whose rows omit `id` still binds to the current item by its
  durable URL or path, avoiding silent sequence loss after migration;
- corrupt current data is preserved and blocks replacement rather than being
  overwritten with an empty snapshot;
- writes run off the UI thread, retain program order, coalesce a burst to the
  newest pending snapshot, and drain before application state is destroyed;
- Resume last playback session appears only when a valid snapshot exists and
  the Playback visibility setting permits it; hiding the main-menu row does not
  hide the same command from Action finder;
- restored search results retain their query, search kind, exact selection,
  item order, and deterministic Next/Previous behavior without pretending that
  an unavailable continuation token can fetch more;
- restored local folders retain the folder path, exact selection, and complete
  saved sequence without rescanning the disk;
- real Python app-data compatibility still passes recursive value comparison.

Automated results: app 99 tests, core 33 tests, storage 44 tests, Windows UI 12
tests, full workspace Clippy with warnings denied, and
`PYTHON_DATA_COMPAT=PASS`.

Still required: packaged NVDA/Narrator checks for menu availability, activation,
focus, Escape return, and same-position resume. Return-screen restoration for
RSS, AudioVault, subscriptions, notifications, and trending will become active
with those still-unimplemented Rust screens; their Python identifiers and data
are already preserved in the snapshot.

## 2026-09-08: Python-compatible notification center

Implementation: `NotificationFile`, `NotificationController`, application-owned
notification state, and the native Win32 Notification center view.

Verified:

- `notifications.json` loads from the isolated Rust beta data directory with a
  read-only fallback to the stable Python file, while the first mutation writes
  only to the beta directory;
- Python notification fields, playable media items, informational empty items,
  and unknown future fields survive typed load/save without value loss;
- malformed current data blocks add, remove, and clear operations and remains
  byte-for-byte untouched instead of being silently replaced;
- additions are newest-first and retain Python's 200-entry limit;
- the native list uses Python's accessible field order for title, message,
  media title, channel, and local timestamp;
- Enter and double-click play the selected notification, the remove-selected
  shortcut deletes one row, and the Clear notifications button and context item
  clear the complete list;
- notification playback stores the exact selected index for Escape return and
  clears unrelated player sequences, preventing previous/next from inheriting
  a stale search, folder, or playlist;
- the main menu and global `Ctrl+Shift+V` action open the same screen, and last
  session restoration recognizes the Python `notification_center` identifier;
- a typed round trip of a private temporary copy of real Python notification
  data passed recursive JSON value comparison.

Automated results: app 103 tests, core 33 tests, platform 22 tests plus one
ignored live test, playback 16 tests, storage 47 tests, Windows UI 14 tests,
full workspace Clippy with warnings denied, and `PYTHON_DATA_COMPAT=PASS`.

Still required: packaged manual NVDA/Narrator checks for initial focus, row
announcements, Enter, double-click, context menu, individual removal, clear,
playback, and exact Escape focus return. Automatic production of subscription,
download, and updater notifications remains coupled to those unfinished Rust
features.

## 2026-09-09: non-blocking YouTube result metadata

Implementation: protocol version 4 metadata batches, a dedicated component
runtime, an official YouTube Data API batch client, generation-scoped
application merges, and per-row native list updates.

Verified:

- search, channel, and playlist rows preserve Python's field order: title,
  channel, compact views, upload age or `Uploaded unknown`, duration, and type;
- metadata hydration uses a separate `YoutubeSearchService`, so it cannot make
  initial search display, exact-item activation, or stream resolution wait;
- both yt-dlp and the optional Rust backend accept the typed metadata command,
  while requests are bounded to ten URLs by the protocol and scheduled in
  Python-compatible batches of five by the Windows shell;
- when a YouTube Data API key is configured, the Windows shell instead sends
  up to 50 video IDs to the fixed official `videos.list` endpoint. The client
  has bounded response size, request/connect timeouts, disabled redirects,
  optional proxy support, secret-redacted errors, and preserves result order
  and all durable or resolved locations;
- rejected, malformed, disconnected, or partial API responses disable the API
  optimization only for that result generation and continue through the
  existing five-item component path; no API error blocks search or playback;
- one yt-dlp process handles each batch and tolerates individual unavailable
  videos when at least one batch item succeeds;
- hydrated values merge only into an exact durable identity and cannot replace
  the public URL, resolved stream URL, separate audio URL, local path, selected
  index, result order, or playback-sequence order;
- stale search and nested-collection generations are ignored;
- upload dates from yt-dlp (`YYYYMMDD`) and the Rust backend (`YYYY-MM-DD`) are
  both converted to relative ages;
- native rows update individually. The currently focused result is deferred
  while a screen reader may be speaking it, then refreshed after selection
  moves; all other changed rows update immediately without recreating the list;
- the explicit live yt-dlp test hydrated two public videos with view counts and
  upload dates in the same run that searches, resolves, and reads a playlist;
- the packaged UI qualification searched for videos, observed the focused
  first row remain `Uploaded unknown`, observed another row update in the
  background, moved to row two, then confirmed row one updated and selection
  remained exactly on row two;
- the package manifest, hash verification, tamper rejection, staged install,
  launch, uninstall, and both data-preserving and data-removing paths passed.

Automated results: app 111 tests, core 34 tests, media 10 tests, platform 31
tests plus two ignored live tests, playback 16 tests, storage 47 tests, Windows
UI 19 tests, updater 2 tests, YouTube helper library 5 tests plus 2 process tests,
and full workspace Clippy with warnings denied.

The installed package was requalified with no API key configured: initial
results remained immediate, a nonfocused row hydrated through the five-item
fallback, the focused row remained stable until selection moved, and the
selection stayed on the exact requested row. Deterministic API tests cover the
50-item bound, response ordering, metadata normalization, URL preservation,
duration/date/live conversion, malformed responses, and secret redaction.

Still required: a real NVDA and Narrator listening pass and a live packaged
50-item request with a user-configured YouTube Data API key. No test key is
bundled or written to settings.

## 2026-09-09: native official YouTube Trending

Implementation: a fixed-size Trending search session, the official YouTube
Data API `videos.list` `chart=mostPopular` path, a generation-scoped worker,
and a native Win32 country/category screen matching the Python inventory.

Verified in automated tests:

- all 56 Python country choices and all nine category choices are represented
  in their original order, with category labels read from the active locale;
- the two filters are native named comboboxes, selection changes reload the
  feed, Enter/double-click on a result use the ordinary exact-result playback
  path, and Escape unwinds one route back;
- a zero result-limit setting requests one fixed API page of 50 items, while a
  configured result limit is honored up to the API maximum of 50; Trending can
  never accidentally request a dynamic search continuation;
- `videos.list` requests use the documented `mostPopular` chart, optional ISO
  region and numeric category filters, the configured proxy, bounded response
  reads, fixed timeouts, disabled redirects, and redacted errors;
- each API worker and public component fallback is tied to the active search
  generation, so changing a filter or leaving the screen rejects late data;
- failed or empty API responses may try only a real public YouTube chart or
  Explore destination. They never substitute a `#trending` search;
- YouTube removed its former all-purpose Trending page in July 2025. The live
  packaged yt-dlp check confirmed that `/feed/trending` now redirects home, so
  `All` and categories without a current public destination fail honestly with
  the existing API-key guidance when no Data API key is configured;
- empty public chart responses are errors rather than successful empty feeds;
- result actions, favorites, queue actions, copying, playlists, background
  metadata hydration, player sequence order, and last-session restoration use
  the same application-owned result state as ordinary YouTube search;
- last-session data retains the exact country/category codes, combobox indexes,
  selected result, and deterministic Previous/Next sequence.

Automated results at this checkpoint: app 116 tests, core 34 tests, media 10
tests, platform 34 tests plus two ignored live tests, playback 16 tests, storage
47 tests, Windows UI 20 tests, updater 2 tests, YouTube helper library 5 tests
plus 2 process tests, and full workspace Clippy with warnings denied.

The release package also passed the existing build, manifest-tamper,
libmpv/yt-dlp/helper, install, reinstall, uninstall, and app-data preservation
gates. `qualify_youtube_trending_ui.ps1` ran the installed executable with an
isolated temporary `%APPDATA%` and verified through native Win32 and MSAA that:

- country and category are named comboboxes with 56 and nine choices;
- the results list exposes the explicit accessible name `Trending`;
- the exact forward Tab path is country, category, results, Back, then Load;
- loading remains asynchronous and the Load button remains available like the
  Python UI; and
- a missing API key produces the recovery dialog and returns to the main menu.

This qualification also exposed and fixed a shared native-list issue: changing
a Win32 list window title did not provide an MSAA name. Current result,
collection, folder, history, favorites, notification, playlist, and main-menu
lists now use dynamic MSAA name annotation whenever their semantic screen name
changes.

Still required: a real NVDA and Narrator listening pass and a live packaged API
result request with a user-provided key, including Enter, context-menu,
playback, and successful-result Escape return. No secret is bundled, logged, or
written for testing.

## 2026-09-10: native YouTube subscriptions

Implementation: a Python-compatible typed subscription store, transactional
application controller, a dedicated generation-scoped YouTube worker, and a
native Win32 Subscriptions screen matching the Python navigation and actions.

Verified in automated tests:

- current beta data takes precedence over legacy stable data, while a missing
  beta file imports stable subscriptions without modifying the stable file;
- malformed current data blocks writes instead of replacing recoverable user
  data, and every successful mutation uses the atomic JSON writer;
- channel URLs are canonicalized before add, remove, and refresh operations, so
  equivalent video/channel routes cannot create duplicate subscriptions;
- first refresh establishes a baseline without reporting every existing upload
  as new, later refreshes retain the latest 20 URLs/items, and one failed
  channel does not prevent results from other channels being merged;
- manual and automatic checks use a third YouTube service independent of search
  and metadata hydration, preventing a subscription refresh from taking over an
  active result screen;
- automatic checking honors the configured enabled flag and 0.5-to-168-hour
  interval, while manual checking remains available from the screen and tray;
- saved new-video rows restore an application-owned result sequence for exact
  playback, Previous/Next, queue, favorites, playlist, and context actions;
- category filtering and editing preserve the current selection, and
  subscription events use the durable notification center plus the configured
  announcement and Windows tray-notification preferences;
- the real stable Python `subscriptions.json` was read from a private temporary
  copy through the typed adapter and round-tripped with semantic value equality.

`qualify_youtube_subscriptions_ui.ps1` ran the installed side-by-side beta with
isolated temporary settings and verified through Win32 and MSAA that:

- the list exposes the accessible name `Subscriptions` and Python-compatible
  row field order;
- every control has the expected native role, class, and accessible name;
- the exact forward Tab order is Subscriptions, Back to main menu, Check
  subscriptions now, Open channel videos, New videos, Remove, Filter by
  category, and Set category; and
- Escape returns to the main menu without leaving the beta process or temporary
  data behind.

Network-dependent subscription refreshes are deliberately excluded from this
deterministic UI script. Worker/controller tests cover success, independent
failures, stale generations, and transactional persistence; a real account and
network listening pass remains a pre-release qualification item.

## 2026-09-12: native podcasts and RSS library

Implementation: bounded RSS/Atom and OPML parsers, the Python-compatible
`rss_feeds.json` store, an application-owned feed controller, Apple Podcasts
directory client, generation-scoped workers, refresh scheduling, and native
Win32 feed, episode, search-result, and category screens.

Verified in automated tests:

- current beta data takes precedence over a read-only stable Python fallback;
  unknown feed and episode fields survive atomic writes, while malformed or
  over-64-MiB archives are rejected without replacement;
- RSS 2.0 and Atom feeds normalize titles, durable episode URLs, publication
  times, durations, descriptions, and chapter metadata through bounded XML
  parsing rather than ad hoc text extraction;
- refresh merges retain played state and playback progress by stable episode
  identity, preserve complete archives beyond the visible batch, and report
  only genuinely new episodes after a baseline has been established;
- direct feed add/remove, category filtering and assignment, per-feed speed,
  played state, progress clearing, and OPML import/export use transactional
  application operations;
- an individual refresh failure does not prevent other feeds from updating,
  and startup plus interval refreshes run off the UI thread without stealing an
  active search, result, or player screen;
- opening a feed projects only the configured 25-to-500-item batch, while End
  or Down at the visible boundary appends the next batch without truncating the
  durable archive or losing selection;
- podcast playback creates a feed-owned sequence for exact Previous/Next,
  applies its feed speed preset, persists item-specific resume state, marks an
  ended or near-ended episode played, and restores the same feed, item, and
  sequence from a last-session snapshot;
- Apple directory requests use fixed official endpoints, bounded responses,
  explicit country/provider/limit settings, optional proxy support, and
  secret-free errors; normal workspace tests remain network-independent;
- OPML import parses the whole file before one atomic library mutation, so a
  partial parse or write cannot leave a half-imported collection.

`qualify_podcasts_ui.ps1` ran the installed side-by-side beta with isolated
temporary settings and a deterministic Python-shaped feed archive. Native
Win32 and MSAA inspection verified:

- the main-menu Podcasts and RSS command opens from the selected row;
- feed, episode, and category lists expose the exact semantic accessible names,
  native ListBox role, and Python-compatible row field order;
- every visible command exposes a native Button role and exact accessible name;
- the feed Tab path is list, Back, Search podcasts, Browse categories, Add,
  Refresh, Open, Remove, Filter, Set category, Import OPML, Export OPML, then
  wraps to the list;
- Enter on the selected feed opens its episode list;
- the episode Tab path is list, Back, Refresh, Play, Download episode audio,
  Download entire feed, then wraps to the list;
- the category Tab path is list, Back, Open, then wraps to the list; and
- Escape returns from categories or episodes to feeds, then from feeds to the
  main menu one route at a time.

Automated results at this checkpoint: app 130 tests, core 34 tests, media 19
tests, platform 37 tests plus two ignored live tests, playback 16 tests, storage
54 tests, Windows UI 23 tests, updater 2 tests, YouTube helper library 5 tests
plus 2 process tests, and full workspace Clippy with warnings denied.

Podcast download commands intentionally still report that they are unavailable
in this internal build. Before podcast parity can close, the shared download
engine, podcast chapters/details/transcripts, successful live Apple directory
flows, and real NVDA/Narrator listening checks still need implementation or
qualification. Native-app computer use was unavailable in this task, so the
installed process was exercised through the repository's Win32/MSAA harness;
this is not represented as a screen-reader listening pass.

## 2026-09-13: native downloads and current-downloads route

Implementation: the typed `YtDlpDownloader`, an application-owned transient
download controller, bounded background workers, native Save As/folder dialogs,
and the Win32 Current downloads route.

Verified in automated tests and the offline production-process fixture:

- single audio and video requests use the Python-compatible format, quality,
  height, filename, metadata, subtitle, archive, retry, fragment, timeout,
  proxy, cookie, and FFmpeg settings;
- ordinary downloads start anonymously and report progress immediately;
  configured cookies are retried only for authentication or age-gate failures
  before media transfer starts, avoiding both the normal-path cookie penalty and
  an unsafe restart after a partial download;
- process output is read with a per-line memory bound, cancellation cannot
  deadlock behind a full reader channel, and Windows download children are
  attached to a kill-on-close job so application exit cannot orphan `yt-dlp` or
  FFmpeg;
- podcast episodes always use audio mode, whole RSS feeds and queued batches run
  sequentially, and one failed child is retained in the final summary without
  stopping the remaining items;
- active tasks expose downloading, processing, aggregate position, percent,
  cancel-selected, and cancel-all state; queued tasks can be started, removed,
  or downloaded together as audio or video;
- playlist/channel and large or collection-bearing queued batches open a
  separate modeless progress window with aggregate progress, Hide, and See
  details controls; Tab is routed through that window independently, closing
  only hides it, and See details restores the main window on Current downloads;
- playlist and channel result menus expose Audio and Video under one native
  Download playlist or Download channel submenu at Python's separator position;
- direct links, search/results, YouTube collections, favorites, history, user
  playlist items, podcast episodes, RSS feeds, and the player expose their
  applicable download or queue commands without making local files downloadable;
- single-file Save As and collection/batch folder selection honor Ask every
  time, while generated Windows path components reject invalid characters,
  trailing dot/space cases, and reserved device names;
- the package now contains its static FFmpeg executable and records it in
  `build-info.json`; isolated install qualification executed both bundled
  `yt-dlp --version` and `ffmpeg -version` successfully;
- the localhost fixture completed a real production `yt-dlp` audio download,
  emitted download and processing progress plus a final file event, probed the
  result with ffprobe, and decoded it through FFmpeg.

Automated results at this checkpoint: app 135 tests, core 34 tests, media 19
tests, platform 46 tests plus two ignored live tests, playback 16 tests, storage
54 tests, Windows UI 32 tests, updater 2 tests, YouTube helper library 5 tests
plus 2 process tests, full workspace Clippy with warnings denied,
`MEDIA_PROCESS_SPIKE=PASS`, and `LOCAL_BETA_SCRIPTS=PASS`.

AudioVault download paths, marked-clip export, converters, network throughput
comparison, and a real NVDA/Narrator listening pass remain open. Native-app
computer use was unavailable, so no automated harness result is represented as
a screen-reader listening test.

## Marked clip checkpoint (2026-09-20)

Local in-progress implementation now includes independent start/end marker
toggles, generation-bound preview, and FFmpeg audio/video clip export. Preview
seek, unpause, and watchdog setup run in one worker request; marker changes
cancel the matching worker preview. Export encodes to a same-directory temporary
file and publishes without overwriting an existing destination only after
successful, nonempty output. Failure removes the temporary result.

Evidence: playback unit tests (18 passed), Windows UI cargo check, full workspace
tests and Clippy in the local build before transactional export, platform Clippy
after transactional export, and the opt-in packaged-FFmpeg test for actual WAV
clip output, existing-output preservation, invalid input, and temporary cleanup.

Windows computer use successfully inspected the beta main menu. The user stopped
the next keyboard action with physical Escape. Marker, preview, and export UI
acceptance and NVDA listening remain unverified; the manifest remains unchecked.
Other open issues include export cancellation and source HTTP-header forwarding,
and preview completion projection when UI event consumption is delayed. This is
not a declaration of full marked-clip or overall product parity. No public release
or stable installation change was made.

## Chapter implementation checkpoint (2026-09-20)

The player Chapters action now opens the native list picker with current-chapter
selection, time ranges, Play and Back. Previous/next chapter actions retain the
Python 0.75/1.5-second thresholds. Chapter normalization accepts the Python field
aliases and numeric/clock-string timestamps. Source metadata takes precedence;
libmpv chapter-list observations provide embedded chapters as a fallback. The
external mpv qualification adapter also observes chapter-list.

Evidence: three chapter model tests, 22 playback tests including native node
copy lifetime and invalid-list bounds, workspace compilation and full-workspace
Clippy. The node layout was checked against mpv's public client.h. Real embedded
media, external podcast chapter fetch, default/custom dialog shortcuts, full
UI parity and NVDA/computer-use acceptance remain open. No chapter manifest gate
has been marked complete based solely on these unit tests.

### External and embedded chapter follow-up (2026-09-20)

Podcast chapter URLs now load on demand on a background thread for the list and
previous/next actions. Repeated requests for the same player generation coalesce;
the pending action is discarded when polling detects a replacement item or a
non-player route. Results are cached only into the requesting player generation.
An empty/failed external response allows the embedded chapter fallback. No
chapter request is added to ordinary playback startup.

Evidence: all 142 apricot-app tests pass, including stale chapter response and
closed-session rejection; four rss_client tests pass, including localhost HTTP
success, HTTP 403, invalid JSON and oversized Content-Length rejection. The body
reader additionally remains bounded for missing/inaccurate Content-Length, but
that specific streaming-overflow path is not yet covered by the HTTP fixture.
The packaged libmpv integration test passes with generated chapter-bearing MKA
and replacement WAV, proving chapter observation and clearing on replacement.
Workspace Clippy passes. No NVDA or native chapter dialog acceptance is claimed.
Custom dialog shortcut parity and complete keyboard/computer-use verification
remain outstanding. No public build or stable installation was changed.

### Chapter dialog shortcut wiring (2026-09-20)

The chapter picker now receives the configured open_selected/player_back chords,
while retaining native Enter/Escape behavior. Custom key handling is restricted
to messages targeting the dialog or its children. Other generic pickers keep
their existing default behavior. A focused unit test verifies custom accept/back
mapping and that an unrelated player key is not captured. Full workspace tests
passed before that additional test; the additional test and final workspace
Clippy also passed. Native UI/NVDA verification of custom bindings remains open.

## Lyrics implementation checkpoint (2026-09-21)

The player_lyrics action now opens a read-only native text dialog immediately,
fetching local sidecars or optional LRCLIB text on a worker. Copy lyrics and Back
use localized labels; copy does not put loading/no-result messages on the
clipboard. Sidecar order, 512 KB limit, UTF-8 replacement, title cleanup, artist
fallback, and synchronized/plain response preference follow the Python source.
Online requests are bounded to 20 seconds and 2 MB, with trusted HTTPS redirects.

Evidence: nine platform lyrics tests, three display/timing model tests, and
workspace Clippy pass. Native screen-reader acceptance and a live service request
have not been performed. The initial UI uses the existing text-dialog surface;
timed rich-text highlighting, exact dimensions/accessibility-name parity, and
live playback observation while the modal view is open still need implementation
or verification. This is not complete Lyrics parity and no release was made.

Follow-up: the lyrics dialog now starts at the Python 620x460 size. Read-only
text controls receive an MSAA name annotation without changing their value.
A Windows-native hidden EDIT-control test confirms that annotating it as Lyrics
preserves its original text. This test does not assert NVDA speech or substitute
for full UI acceptance. Timed highlighting and modal playback observation remain
outstanding.

Live lyrics follow-up (2026-09-21): the opt-in
`live_lrclib_returns_displayable_lyrics` test passed against LRCLIB using the
production HTTP client and a public song query. It verifies nonempty retrieved
lyrics and display parsing without logging the lyrics. This supersedes the
earlier note that no live service request had been performed; native dialog,
NVDA, and highlighting acceptance are still open. yt-dlp item conversion now
retains track/artist/creator/album fields, with an extraction-to-query regression
test, without extra extraction requests.

Timed lyrics wiring (2026-09-21): lyrics now use the system Rich Edit control
and retain parsed timed lines. A 200 ms UI timer reads a generation-bound cached
position, without consuming player events or querying mpv. Highlight changes
restore the user's selection; initial implementation does not yet implement
Python's ShowPosition automatic scrolling. Rich Edit character-offset behavior,
highlight colors, selection restoration and NVDA interaction still require
native runtime verification. Workspace Clippy passes; this is implementation
evidence, not a completed accessibility gate.

Rich Edit offset verification (2026-09-21): timed-line Rich Edit ranges are now
precomputed once, rather than rescanning the text during each highlight change.
Automatic scrolling to the active line is wired via EM_SCROLLCARET. A hidden
native RICHEDIT50W test selects both timed lines using these ranges and reads
them back through EM_GETSELTEXT; exact matches pass with a source header,
untimed paragraph, supplementary Unicode character and accented text. This
verifies the CRLF-to-Rich-Edit offset conversion, but does not yet verify visible
scrolling, highlight colors, user selection restoration, or NVDA speech.

Long lyrics verification (2026-09-21): a hidden native read-only RICHEDIT50W
control preserves 136,000 characters set through SetWindowTextW and retrieved
through GetWindowTextW, with exact content equality. No production text-limit
change was necessary. The complete ordinary workspace suite passes (380 tests,
7 opt-in tests ignored), as does workspace/all-targets Clippy with warnings
denied. This does not qualify clipboard behavior or screen-reader interaction.

Transcript parser checkpoint (2026-09-21): added app-level SRT/WebVTT parsing
based on Python media.py, retaining cue source order, optional valid end times,
metadata/comment-block exclusion, inline-tag cleanup and near-duplicate cue
suppression. Three focused tests pass and workspace/all-targets Clippy passes.
This is not a wired transcript feature: local/remote fetching, caching,
language selection, search/copy/seek dialog and NVDA acceptance remain open.
HTML decoding currently uses html-escape 0.2.15; unlike Python html.unescape,
its documented handling excludes legacy semicolonless references and C1
replacement. Resolve that parity gap before accepting this parser as complete.

Transcript track selection checkpoint (2026-09-21): configured CSV languages
are deduplicated case-insensitively with en/sl fallbacks, language-region
matching is supported, requested/manual/automatic source priority is explicit,
and VTT precedes SRT within each language. Selection borrows the entire track
including request headers. Six transcript tests and workspace Clippy pass.
Source JSON object insertion order still needs preservation for fallback
language ties: default serde_json maps sort keys, unlike Python dictionaries.
No network or dialog wiring is implied by these model tests.

Local transcript loading (2026-09-21): worker-callable sidecar loading follows
Python's plain VTT/SRT, captions, transcript, then configured-language order.
Only regular files up to 5,000,000 bytes are accepted; reads are bounded even
if a file grows after its metadata check. Invalid/empty candidates fall through
to the next file. Temporary-directory tests cover precedence, invalid content,
language fallback, oversized files and absent files. Eight transcript tests
and workspace Clippy pass. The native transcript dialog is still not wired.

Transcript source-order correction (2026-09-21): workspace serde_json now
preserves object insertion order. A raw-JSON regression verifies fallback
language order and regional-language ties, including an explicit regional
preference. This resolves the source-order gap above. All 389 ordinary workspace
tests pass; seven opt-in integration tests remain skipped by this command.
No stable installation or user-data files were touched.

Transcript dialog model (2026-09-21): TranscriptView formats Python-style
numbered/time-stamped labels, filters against the full label, maps visible rows
back to original cues and retains all cues for full-text copying regardless of
the filter. Regression coverage verifies timestamp searches, case-insensitive
text searches, original row numbering, empty results and copy-all semantics.
All ten transcript tests and workspace Clippy pass. Native control wiring,
actual clipboard/seek actions and screen-reader acceptance remain outstanding.

Transcript extraction adapter (2026-09-21): yt-dlp now exposes an on-demand
subtitle metadata operation using the existing bounded process runner and
configured proxy/cookie arguments. It accepts HTTP(S) source URLs, rejects
embedded credentials, separates the source using --, requests manual/automatic
subtitle metadata and skips media downloading. A command-construction test
and workspace Clippy pass. This is not a live extraction test: caption fetching,
fallback handling, session cache and dialog wiring remain unfinished. Normal
playback does not invoke this operation.

Transcript HTTP transport (2026-09-21): bounded worker HTTP loading now merges
extractor/track headers, applies the proxy, classifies HTTP 429 separately,
limits responses to 5 MB and follows at most five redirects without HTTPS
downgrades. Origin changes remove Authorization, Cookie and Referer. Loopback
HTTP tests verify text retrieval, 429, oversized Content-Length and removal
of those headers across a redirect to another port. Four platform transcript
tests pass. Actual public-service requests, unknown-length oversized bodies,
yt-dlp fallback, cookie-retry integration and native dialog remain open; this
transport is not yet connected to a user action. Default User-Agent still
needs alignment with the Python/configured browser identity.

Transcript fallback adapter (2026-09-21): yt-dlp subtitle-only fallback writes
under an automatically cleaned private temporary directory. Arguments retain
skip-download and remove metadata-only dump mode; output names use a fixed
caption stem. Bounded reading accepts nonempty VTT/SRT regular files in newest
mtime order and excludes symlinks. A command/file fixture and workspace Clippy
pass. The test does not launch yt-dlp or prove a real 429 recovery; invoking
fallback only after direct-fetch failure still requires orchestration wiring.

Transcript worker orchestration (2026-09-21): a native-UI worker module now
combines local loading, durable source URL selection, configured yt-dlp metadata,
language/track selection, direct HTTP and fallback. Tests prove that direct
success never invokes fallback, fallback success recovers direct failure,
rate-limit classification survives failed recovery, and an expired resolved
stream is not used as the extraction source. Three worker tests and Clippy
pass. The worker is not yet launched by a dialog; generation-bound caching,
cookie-retry behavior and complete UI/NVDA verification remain outstanding.

Transcript session cache (2026-09-21): PlayerSession now stores a typed checked
transcript result, distinguishing not-yet-requested from checked-and-empty.
Replacement and close clear it; generation checks reject responses belonging
to old playback items. An application entry point exposes safe caching to the
dialog. Eleven app transcript/cache tests and workspace Clippy pass. Cache
consumption by the native dialog still needs wiring; this alone does not prove
that reopening the UI avoids network requests.

Native transcript initial wiring (2026-09-22): player_transcript now opens a
native dialog with search, cue list, Play, copy-line, copy-all, timestamp-link
and Back controls. A worker supplies local/online data; the 50 ms UI poll retains
search text without moving focus. Enter from search/list seeks without closing;
Escape closes. Timestamp copying and exact seek callbacks check the requesting
playback generation. Successful results are cached on dialog close and reused
on reopening. All 39 Windows UI crate tests and workspace Clippy pass, but these
tests do not exercise this new dialog. Still pending: configured accept/back
chords, minimum size/localized button sizing, full loading/error/success speech,
exact Python seek announcement text, rate-limit cache handling and actual
computer-use/NVDA acceptance. No local package has been rebuilt for this dialog.
