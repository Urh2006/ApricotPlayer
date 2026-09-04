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

- exact registries for 116 settings, 91 actions, 19 customizable main-menu
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
