# ApricotPlayer 2.0 Rust rewrite plan

Status: approved direction, planning complete enough to start implementation.

Baseline: the exact `main` commit and Python release that exist when the user
says `zacnimo`. At the time this plan was written, that baseline is 1.0.21.

Scope: a complete rewrite of ApricotPlayer-owned application code in Rust with
no intentional feature loss. Spotify is explicitly outside this plan. macOS
implementation starts only after the Windows Rust edition reaches the parity
gate described below.

The detailed product contract is in `docs/RUST_PARITY_MANIFEST.md`. That file is
part of this plan, not optional supporting material.

## 1. Product contract

ApricotPlayer 2.0 is not a redesign and not a reduced new player. It must keep:

1. every current screen, dialog, source, operation, setting, shortcut, context
   menu, error state, and persisted data operation;
2. the current navigation model, including one-step Escape returns, focus
   restoration, background playback, and player-session semantics;
3. all NVDA-accessible names, roles, values, states, announcements, braille
   usefulness, and keyboard-only paths;
4. current fast paths: no new network request, cookie attempt, metadata wait,
   extraction pass, or artificial delay on a successful playback path;
5. the current mpv audio behavior, including stereo output, volume sessions,
   EQ, clipping protection, boost interaction, speed, pitch, ReplayGain, BPM,
   seek, cache, and edit-mode export;
6. compatibility with current settings and user data;
7. stable and beta update behavior once 2.0 is ready for public distribution.

The rewrite may improve internals, performance, diagnostics, security, and
testability. It may not silently change product behavior merely because a Rust
library offers a different default.

## 2. What "rewritten in Rust" means

All ApricotPlayer-owned runtime code will be Rust. The production executable
will not import Apricot's Python modules or ship wxPython.

The following remain bundled third-party engines rather than being rewritten:

- mpv for demuxing, decoding, playback, filters, video output, and audio output;
- yt-dlp for website extraction and downloads;
- FFmpeg/ffprobe for conversion, exports, probing, and selected download work;
- Node.js and yt-dlp-ejs for YouTube JavaScript challenge support;
- Rubber Band support where the selected mpv/FFmpeg build uses it.

This is the same boundary as using an operating-system API or codec library. We
will call these tools through bounded, structured process or IPC adapters. We
will not rewrite a media codec stack or YouTube extractor and pretend it has
feature parity.

yt-dlp will be invoked as a pinned, bundled executable and consumed through its
JSON output. It is not application Python code, but its implementation may
contain its own embedded Python runtime. The normal extraction call count and
latency will be benchmarked against the in-process Python baseline. If process
startup causes a measurable regression, the response is caching, prefetching,
or a narrowly scoped long-lived extraction helper decision backed by timings,
not extra speculative extraction calls.

## 3. Source-control and local-beta policy

Development happens in the existing repository on a local `rust-2.0` branch.
There are no GitHub pushes, tags, releases, or updater announcements until the
user explicitly approves the final 2.0 publication.

Local Git is still mandatory:

- tag the frozen Python baseline locally;
- make small, tested local commits after each vertical slice;
- create local alpha/beta tags for rollback and bisecting;
- keep the remote untouched;
- maintain an offline or private backup of the repository because unpushed Git
  history on one computer is not a backup.

Rust test builds install side by side as `ApricotPlayer 2 Beta` and must not
replace the stable Python installation. They use separate identity and paths:

```text
Executable name: ApricotPlayer2Beta.exe
Application ID: ApricotPlayer.RustBeta
Program data: %APPDATA%\ApricotPlayer2Beta
Install root: %LOCALAPPDATA%\Programs\ApricotPlayer2Beta
```

The local beta updater is disabled. A local build/install script replaces the
beta installation atomically and preserves beta data. The public 1.x updater
must never discover or offer these local builds.

## 4. Architecture decision

### 4.1 Shared core, native platform surfaces

The application uses a shared Rust core and platform adapters. Windows 2.0 uses
standard Win32/Common Controls through Microsoft's `windows` crate. The later
macOS port uses AppKit through a Rust Objective-C bridge such as `objc2`, while
sharing the same domain, application, source, playback, storage, and action
code.

This choice intentionally favors native accessibility and predictable focus
over a custom-drawn cross-platform GUI. The official `windows` crate exposes
Windows APIs directly. AccessKit and egui remain useful reference/fallback
options, but AccessKit currently does not cover every rich-text/hypertext case
and Apricot's accessibility contract is too central to accept that risk without
real NVDA testing.

The UI boundary is not a generic home-grown widget toolkit. Shared code owns
routes, actions, view data, validation, and state transitions. Each platform UI
uses its native controls to render those states and sends typed actions back.

### 4.2 Retain external mpv for the first Rust release

The first Rust release keeps the proven mpv process plus JSON IPC design:

- Windows named pipe transport;
- structured JSON commands, never command-string concatenation;
- bounded response size and timeout;
- observed properties and explicit request IDs;
- child-process lifetime ownership and crash recovery;
- standard child HWND embedding for video and a separate audio-only path;
- the exact current cache, HLS, seek, audio-filter, and startup policy.

This preserves codec coverage, filter behavior, and crash isolation. `libmpv`
can be evaluated later behind the same `PlaybackEngine` interface, but it is not
allowed to become an unproven dependency of the parity rewrite.

### 4.3 Cargo workspace

The rewrite lives beside the Python baseline until parity is complete:

```text
rust/
  Cargo.toml
  Cargo.lock
  rust-toolchain.toml
  apps/
    apricot-player/
  crates/
    apricot-core/
    apricot-app/
    apricot-media/
    apricot-playback/
    apricot-storage/
    apricot-platform/
    apricot-ui-windows/
    apricot-updater/
    apricot-test-support/
  resources/
  tests/
  tools/
```

This is deliberately a small set of deep crates, not one crate per Python file.

### 4.4 Module responsibilities

`apricot-core`:

- domain types such as `MediaItem`, `MediaId`, `MediaSource`, `Collection`,
  `QueueEntry`, `PlaybackBookmark`, `Chapter`, `TranscriptLine`, `Comment`,
  `DownloadRequest`, and `PlaybackPosition`;
- typed `ActionId`, `Route`, `FocusId`, `Shortcut`, `SettingId`, and errors;
- pure queue, navigation, sorting, validation, EQ, and state-transition logic;
- no UI, network, filesystem, process, registry, or platform calls.

`apricot-app`:

- application use cases and the single authoritative `AppState`;
- navigation stack, player session, background player, task lifecycle, and
  cancellation generations;
- action dispatch and scope enforcement;
- view models and user-facing announcement events;
- orchestration through interfaces supplied by the other crates.

`apricot-media`:

- source adapters for YouTube, SoundCloud, direct URLs, local files/folders,
  podcasts/RSS, Apple Podcasts directory, and AudioVault;
- search, trending, channels, playlists, Shorts, subscriptions, metadata
  hydration, lyrics, transcripts, comments, and chapters;
- yt-dlp JSON adapter, HTTP client, cookies, stream selection, fallback policy,
  and remote response limits;
- download planning and FFmpeg/yt-dlp command construction.

`apricot-playback`:

- `PlaybackEngine` interface and mpv process/IPC implementation;
- player state machine and event translation;
- seek/scrub scheduler, queue progression, gapless/prefetch policy, resume, and
  end detection;
- volume, output device, EQ, bass boost, clipping protection, ReplayGain,
  speed, pitch, BPM, markers, and edit/export filter plans;
- exact distinction between keyframe seeking and exact jumps.

`apricot-storage`:

- typed settings schema, defaults, migrations, unknown-field preservation, and
  validation;
- atomic JSON repositories for all current user data;
- stream cache and bounded metadata caches;
- local-beta import and final 1.x-to-2.0 migration with backup and rollback.

`apricot-platform`:

- path discovery, binary discovery, trusted process launch, file dialogs, URL
  opening, clipboard, notifications, secure credentials, logging, and clocks;
- single-instance lock, activation, file-open forwarding, startup registration,
  file associations, output devices, tray, fullscreen, and screen-reader
  announcement interfaces;
- Windows implementation now and macOS implementation later.

`apricot-ui-windows`:

- native window, controls, dialogs, menus, player video host, tab order, focus,
  keyboard events, UI Automation behavior, and high-DPI layout;
- rendering of application view models and dispatch of typed actions;
- no source extraction, persistence, queue policy, or audio logic.

`apricot-updater`:

- app and yt-dlp update checks, channels, skip-version behavior, verified
  metadata, hashes, trusted redirects, package validation, install transaction,
  restart, and rollback;
- local beta mode refuses network publication/update operations.

`apricot-test-support`:

- fake clock, filesystem, HTTP, source, mpv, process, notification, credential,
  and accessibility adapters;
- recorded sanitized fixtures and deterministic task scheduler helpers.

## 5. Canonical registries

Three registries prevent the drift that has historically caused shortcut,
context, label, and settings bugs.

### 5.1 Action registry

Every user action has one record containing:

- stable `ActionId` matching the current Python ID where one exists;
- localization label key;
- default Windows shortcut and future macOS mapping;
- valid scopes: global, list, player, settings, or modal;
- menu/context/Action Finder visibility;
- enablement predicate and unavailable announcement;
- whether held repetition is allowed;
- accessibility result announcement policy.

Main-menu labels, player labels, context menus, Action Finder, Settings shortcut
editor, conflict detection, and runtime routing all consume this registry. Tests
fail if any of the 91 current actions lacks a handler, label, shortcut mapping,
scope, or parity entry.

### 5.2 Settings schema

Every setting has one typed descriptor containing:

- stable JSON key;
- Rust type, default, constraints, and normalization;
- settings section and control type;
- label/help localization keys;
- reset-section and reset-all behavior;
- migration aliases and platform applicability;
- secret/redaction policy.

Tests compare this registry with the 116-field baseline. UI controls, defaults,
serialization, validation, diagnostics, and resets derive from the same schema.

### 5.3 Main-menu registry

The 19 customizable actions have one ordered registry. Update Available,
Settings, and Exit are explicitly permanent. Hiding an action only changes its
main-menu visibility; its global shortcut remains active.

## 6. State and concurrency model

The native UI thread owns controls and never performs network, process, parsing,
filesystem, hashing, conversion, or media-analysis work.

A single Tokio runtime hosts background work. The UI sends typed commands to an
application coordinator and receives bounded events. Important rules:

- no application-wide `Arc<Mutex<AppState>>` shared by every subsystem;
- no lock is held across `.await`;
- one owner mutates application state;
- bounded channels apply backpressure;
- time-position and progress updates are coalesced;
- every search, collection load, metadata hydration, playback resolution, and
  settings render has a generation/cancellation token;
- stale completions cannot change focus, lists, player title, or current item;
- shutdown cancels and joins owned tasks with explicit timeouts.

Held seek, volume, speed, and pitch use a repeat-state machine, not the raw
operating-system key-repeat queue. It performs one immediate step, waits the
configured hold delay where applicable, repeats at the configured interval,
coalesces announcements, and stops on key-up, focus loss, route change, or
player close. It can never leave a backlog of delayed actions.

## 7. Navigation and focus model

Navigation becomes explicit instead of being inferred from visible controls:

```text
NavigationStack
  RouteFrame { route, parameters, focus_snapshot, list_snapshot }

PlayerSession
  current_item, source_context, sequence, queue, audio_session,
  playback_state, visible_player_mode
```

The player session is independent of the visible route. This permits background
playback without turning result-list keys into player keys.

Rules:

- Enter pushes exactly one route or starts exactly one action;
- Escape pops exactly one route, except player Escape which closes the player
  view/session according to the existing contract and returns to its captured
  source route;
- closing a channel returns to its parent result list, then to the previous
  search/main route one step at a time;
- every route captures stable focus identity, not only a numeric row index;
- asynchronous list updates preserve focus by item ID;
- global shortcuts are handled before route-local shortcuts, while player-only
  shortcuts are rejected outside the player scope;
- Tab order is declared per view and tested in both directions;
- modal dialogs restore focus to the invoking control.

## 8. Accessibility architecture

Windows uses native controls wherever a standard control exists: buttons,
checkboxes, edit fields, read-only multiline text, list/list-view controls,
comboboxes, sliders, progress bars, menus, and dialogs. That gives NVDA standard
UI Automation roles, names, values, and states.

Additional rules:

- every control has a stable accessible name and meaningful state/value;
- list rows expose useful linear text and selectable state;
- EQ sliders expose band name and signed dB value independently;
- progress updates are throttled and never steal focus;
- background metadata updates do not recreate the focused control;
- announcements are typed events with priority, deduplication key, and expiry;
- use one announcement path at a time: NVDA Controller Client when available,
  otherwise a UI Automation notification/live-region adapter;
- no action is mouse-only;
- Applications key and Shift+F10 open the same context menu;
- Enter and Space semantics are tested for every actionable control;
- letter navigation remains native in lists and is never intercepted by player
  shortcuts;
- diagnostics include current route, focus ID, action scope, task generation,
  and player session state without secrets.

Automated tests inspect the UI Automation tree for roles, names, states, focus,
and invoke actions. Manual gates use NVDA on every screen. Narrator and JAWS get
smoke coverage, but NVDA remains the primary acceptance reader.

## 9. Data compatibility and migration

The Rust beta never writes to `%APPDATA%\ApricotPlayer`. On first request it can
copy the Python data set into its beta directory, record source hashes, and run
migrations on the copy.

The final migration supports:

- `settings.json` with all current keys and unknown-key preservation;
- `favorites.json`, `bookmarks.json`, `history.json`, `subscriptions.json`,
  `rss_feeds.json`, `playlists.json`, `notifications.json`;
- `playback_positions.json`, `playback_queue.json`,
  `last_player_session.json`, and `stream_url_cache.json`;
- `download-archive.txt`, cache metadata, cookies file selection/signature, and
  component state where safe;
- legacy `UrhasaurusYouTubePlayer` settings/favorites migration already
  supported by Python.

Every durable file gets an explicit schema version in Rust-managed output.
Existing unversioned Python files are version 0 inputs. Writes use a temp file in
the same directory, flush, atomic replace, and a generation guard. Corrupt input
is preserved for diagnostics and never overwritten with an empty default.

AudioVault credentials retain Windows DPAPI compatibility during migration and
are reprotected using the Windows credential adapter. Machine-bound encrypted
data is not copied blindly to macOS later.

Before final cutover:

1. close both apps;
2. back up the complete Python app-data directory;
3. migrate into a new 2.0 directory transactionally;
4. validate counts, required fields, and references;
5. keep the backup and a migration report;
6. roll back automatically if startup validation fails.

## 10. Online sources and HTTP policy

All HTTP uses one bounded client layer with:

- HTTPS and trusted-redirect validation where required;
- explicit connect/read/total timeouts;
- response-size limits matching or tightening the Python constants;
- cancellation;
- redacted structured logging;
- retries only for idempotent and approved operations;
- no cookie use on unrestricted fast paths until a relevant failure proves it
  is needed;
- no remote URL allowed to become a local file path or arbitrary process
  argument without validation.

YouTube and SoundCloud extraction use sanitized yt-dlp JSON. Normal, requested-
format, web-safari, cookie, direct-media, HLS, truncated-stream, and EJS paths
remain ordered exactly by the proven recovery policy. Current stream-format
preference and cache-key semantics are preserved.

RSS/Atom, OPML, chapters, transcript, lyrics, comments, AudioVault pages and
manifests all retain hard byte limits. XML parsing does not resolve DTDs,
entities, or external references.

## 11. Playback and audio invariants

The current Python behavior is captured as golden tests before implementation.
The Rust player must preserve:

- player-session volume, output device, EQ, bass boost, volume boost, autoplay,
  repeat, shuffle, speed, and pitch lifetime;
- reset only when the player session actually closes;
- no startup burst before target volume/filter state is applied;
- stereo channel layout unless the source/output explicitly differs;
- one filter graph update per logical audio change;
- independent EQ bands and stable profile values;
- clipping protection that accounts for positive EQ gain and boost interaction;
- output-device-linked EQ presets and missing-device fallback;
- exact current seek modes, buffer/cache options, and HLS selection;
- queue precedence and deterministic previous/next/related behavior;
- saved resume position bound to the correct item identity;
- BPM re-analysis every time `B` is invoked;
- live and edit/export filter-chain equivalence.

Audio changes are represented by an immutable `AudioGraph` value. A planner
compares the desired graph with the applied graph and sends the smallest ordered
mpv command batch. Slider UI changes never mutate another band's value.

## 12. Downloads, conversion, and jobs

Downloads and conversions use a persistent in-memory job manager with bounded
concurrency and explicit states:

```text
Queued -> Resolving -> Downloading -> Processing -> Completed
                                     -> Failed
                                     -> Cancelled
```

Batch parents aggregate child progress. One failed child is announced and
recorded but does not stop remaining children. Progress windows are independent
of the main app and closing/hiding one never closes the player.

Every process launch uses an argument vector, trusted executable resolution,
bounded output capture, cancellation, and process-tree termination. Output path
validation preserves Windows reserved-name, collision, traversal, and extension
rules. Archive extraction rejects links, traversal, alternate data streams,
reserved devices, duplicates, encryption, special files, excessive expansion,
and compression bombs.

## 13. Localization

All 27 current languages remain available. The Rust app initially consumes the
existing `locales_json/*.json` files so translation keys and community workflow
do not change during the rewrite.

The loader provides English fallback and formatted-parameter validation. CI
checks that every used key exists in English, every locale is valid JSON, and
placeholder names match English. New Rust-only text must be added to all locale
files or intentionally fall back to English until translated; it may never be a
hard-coded inaccessible control label.

Moving to Fluent or gettext is a separate post-parity decision. Combining a
localization migration with the language rewrite would add risk without helping
feature parity.

## 14. Security baseline

The 1.0 security work remains mandatory:

- no shell command construction from user or remote text;
- pinned dependencies and committed `Cargo.lock`;
- `cargo audit`, `cargo deny`, license/source policy, Clippy, formatting, and
  minimal unsafe-code review;
- trusted binary discovery and hashes for bundled runtimes;
- disabled external yt-dlp plugins;
- secret-safe logs and diagnostic reports;
- path, URL, archive, update, response-size, and IPC bounds;
- updater hash/signature verification, trusted hosts, transactional replacement,
  and rollback;
- secrets through platform protection, never plaintext diagnostics;
- explicit review of every `unsafe` block with a safety invariant comment.

## 15. Performance contract

Phase 0 records reproducible Python measurements on the user's computer:

- cold and warm launch to focused main menu;
- Enter on a local result and a YouTube result to player announcement/audio;
- Escape route transition latency;
- key-to-action latency for seek, volume, speed, and pitch;
- dynamic list append and metadata hydration latency;
- memory at main menu, active audio, video, large folder, and long session;
- CPU while paused and during normal playback;
- yt-dlp extraction and download throughput.

Hard gates:

- no normal-path task on the UI thread;
- no added extraction or cookie call on a successful path;
- no user-input event backlog;
- Rust p95 action latency must be no worse than the Python baseline;
- Rust warm startup and idle memory must be no worse than the Python baseline;
- result-to-player audio start must stay within the measured noise band unless a
  source/network difference is proven;
- long-session memory must remain bounded.

We target a meaningful improvement, but do not claim one until measurements show
it. Rust's language alone does not make mpv, network extraction, or decoding
faster.

## 16. Test strategy

### 16.1 Automated tests

- pure unit and property tests for sorting, queue, URLs, settings, actions,
  shortcut parsing, EQ, navigation, and migration;
- golden compatibility tests using sanitized copies of every Python data file;
- source contract tests with recorded yt-dlp/HTTP fixtures;
- fake and real mpv IPC tests, including timeout, partial JSON, crash, restart,
  seek, filters, volume, and end events;
- download/process tests with fake executables and real opt-in tools;
- UI Automation tests for every screen, control role/name/state, focus path,
  Enter, Space, Escape, context menu, Tab, Shift+Tab, arrows, Home/End, and
  letter navigation;
- parity tests that count and validate all 116 settings, 91 shortcuts, 19
  customizable menu actions, 27 languages, 10 EQ bands, and current presets;
- security/property tests for malformed JSON/XML, archive names, URLs, paths,
  redirects, huge responses, update packages, and diagnostics redaction;
- concurrency tests with reordered completions and cancellation races;
- memory/task leak soak tests.

### 16.2 Real-device acceptance

Each local beta has a focused smoke script. A parity-complete beta runs the full
manifest with NVDA and keyboard only, then mouse smoke, Narrator/JAWS smoke, and
visual/video/fullscreen checks.

External live tests cover YouTube, SoundCloud, direct media, local media,
podcasts, AudioVault, downloads, cookies, updates, and network interruption.
They are never the only coverage: sanitized deterministic fixtures protect the
same logic from website drift.

### 16.3 Differential testing

Where practical, the same fixture/action script runs against Python and Rust and
compares:

- ordered result IDs and labels;
- route and focus transitions;
- queue/session state;
- mpv arguments and filter commands;
- download plans;
- serialized data;
- announcements and errors.

Differences require an explicit approved compatibility note.

## 17. Implementation sequence

### Phase 0: freeze, inventory, and prove the risky boundaries

- record baseline commit/version and create a local baseline tag;
- run Python tests and capture performance/resource measurements;
- generate machine-readable action, setting, menu, locale, screen, context-menu,
  and persistent-data manifests;
- build a native Win32 accessibility spike with list, checkbox, combobox,
  slider, read-only text, modal, context menu, progress, focus restoration,
  announcements, tray, and video-host HWND;
- prove mpv JSON IPC, embedded video, fullscreen, output devices, EQ, speed,
  pitch, seek hold, and shutdown from Rust;
- prove yt-dlp JSON extraction/download and FFmpeg invocation latency.

Gate: NVDA behavior and normal-path latency are acceptable before broad coding.

### Phase 1: foundation and compatibility kernel

- create Cargo workspace, CI-quality local checks, logging, crash reporting, and
  platform paths;
- implement core IDs/types, action registry, settings schema, localization,
  navigation stack, announcement model, and typed errors;
- implement atomic storage and import a copy of real Python data;
- implement local beta build/install/uninstall scripts.

Gate: all registries match the Python manifest and imported data round-trips.

### Phase 2: accessible shell and settings

- main window, single instance, activation, file forwarding, title, main menu,
  customizable menu, Action Finder, Settings, reset section/all, shortcut
  capture/conflicts, dialogs, tray/background shell, and notifications;
- native focus model and full keyboard routing.

Gate: every shell/settings action works with NVDA and no media feature is yet
claimed complete.

### Phase 3: playback vertical slice

- local single-file playback, mpv lifecycle, embedded video, audio-only,
  fullscreen, background mode, play/pause, time, seek/scrub, volume, output,
  speed, pitch, EQ, boost, clipping, ReplayGain, BPM, resume, markers, edit mode,
  and session close/reset behavior;
- player controls, labels, context menu, details, and format announcement.

Gate: the complete player/audio matrix matches Python on local fixtures.

### Phase 4: local collections and navigation

- folders with natural sorting and dynamic batches;
- favorites, history, bookmarks, user playlists, queue, last session, and
  playback positions;
- deterministic previous/next/shuffle/autoplay and exact return routes.

Gate: no skipped/random items, shortcut leakage, stale focus, or data loss.

### Phase 5: YouTube, SoundCloud, and direct links

- search, trending, channels, tabs, playlists, Shorts, popular sorting, dynamic
  loading, metadata hydration, subscriptions, notification center, and browser
  actions;
- stream resolution, HLS/DASH/progressive policy, direct-media fallback, cache,
  prefetch, cookies, EJS, and all proven recovery paths.

Gate: startup and seeking are no slower than baseline and all source/list
navigation scenarios pass.

### Phase 6: reading and media intelligence

- details, upload time, live state, chapters, transcripts/captions, lyrics,
  comments/replies, timestamp links, bookmarks, sidecars, and copy/export/search
  operations.

Gate: every read-only field is navigable, copyable where applicable, and returns
focus correctly.

### Phase 7: podcasts/RSS and AudioVault

- Apple directory search, direct RSS/Atom, OPML, categories, refresh,
  notifications, full archives, dynamic episode batches, played/progress state,
  per-feed speed, chapters, queue, and downloads;
- AudioVault credentials, login/logout/register, recent movies/shows, search,
  episode navigation, stream, history, queue, individual/whole-show download,
  expiry recovery, archive extraction, and cancellation.

Gate: real-account AudioVault and large-feed podcast acceptance passes.

### Phase 8: downloads, converters, updater, and OS integration

- every single/batch download, queue/progress window, Ask every time, archive,
  metadata/subtitle option, cancellation, and partial failure;
- file/folder conversion, clip export, edit export, collisions, replacement, and
  progress;
- associations, Open With, startup, notifications, diagnostics, app updater,
  yt-dlp updater, install transaction, and rollback.

Gate: all jobs and platform operations pass security and interruption tests.

### Phase 9: parity-complete local betas

- no new feature work until every manifest item is implemented;
- run differential, accessibility, performance, security, migration, soak, and
  clean-machine tests;
- fix beta findings through local builds only;
- preserve the Python installation as immediate fallback.

Gate: zero unapproved parity gaps and no known severity-1/2 defects.

### Phase 10: 2.0 release preparation

- freeze features and migrate final assets/versioning/documentation;
- build a clean Windows installer and portable ZIP from the same commit;
- test clean install, 1.x migration, update, rollback, uninstall, second launch,
  file associations, offline behavior, and corrupted packages;
- remove Python application code/runtime from production packaging;
- write the complete 1.x-to-2.0 changelog and migration notes;
- only after explicit user approval, push the branch, merge, tag, and publish
  GitHub 2.0.

Gate: 2.0 is the first public Rust build. Daily alphas/betas remain local.

### Phase 11: macOS after Windows Rust parity

Only after Phase 10's Windows candidate is stable do we begin macOS. Shared Rust
crates remain unchanged wherever behavior is platform-neutral. We add AppKit,
Keychain, Unix-socket mpv IPC, Finder open/reopen, menu-bar background controls,
VoiceOver announcements, login item, notifications, DMG packaging, Gatekeeper
documentation, and later notarization.

`docs/MACOS_PORT_PLAN.md` remains the full macOS acceptance contract, but its old
Python/wxPython implementation sequence is superseded by this Rust-first order.
Every feature in `docs/RUST_PARITY_MANIFEST.md` must then pass on both platforms.

## 18. Local version sequence

Version names describe confidence, not GitHub availability:

- `2.0.0-dev.N`: internal compile/test checkpoints;
- `2.0.0-alpha.N`: installable local slices with known missing areas;
- `2.0.0-beta.N`: feature-complete local builds with no intentional parity gaps;
- `2.0.0-rc.N`: migration, packaging, performance, and regression candidates;
- `2.0.0`: first approved public GitHub Rust release.

Every installed build exposes its exact version, commit ID, build timestamp,
Rust version, bundled component versions, data schema version, and beta/stable
identity in diagnostics.

## 19. Definition of done

Windows Rust 2.0 is ready only when:

1. every item in `docs/RUST_PARITY_MANIFEST.md` is checked by implementation and
   evidence, with no silent omissions;
2. all 116 settings, 91 shortcuts, 19 customizable main-menu items, 27
   languages, 10 EQ bands, and current presets are accounted for automatically;
3. real Python user data migrates transactionally and rollback is proven;
4. the full NVDA and keyboard acceptance run passes;
5. playback/audio/download behavior and normal-path timing are no worse than the
   frozen Python baseline;
6. security, updater, clean-install, update, rollback, and soak tests pass;
7. the Python application is not required by the Rust production runtime;
8. the user approves publication.

Saying `zacnimo` begins Phase 0, not a blind bulk translation. The parity
manifest remains the checklist throughout the work, and no phase is marked done
from code presence alone.

## Technical references checked for this decision

- Microsoft Rust for Windows: <https://learn.microsoft.com/en-us/windows/dev-environment/rust/rust-for-windows>
- Cargo workspaces: <https://doc.rust-lang.org/cargo/reference/workspaces.html>
- mpv manual and JSON IPC: <https://mpv.io/manual/master/#json-ipc>
- AccessKit platform accessibility model: <https://github.com/AccessKit/accesskit>
- egui accessibility and AccessKit test support, retained as a fallback
  reference rather than the selected Windows UI: <https://github.com/emilk/egui/blob/main/docs/accessibility.md>
