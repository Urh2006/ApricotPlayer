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

Still required for complete playlist parity: per-item and whole-playlist
downloads, source-specific channel actions, and manual NVDA/Narrator dialog and
navigation checks.

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
