# ApricotPlayer beta 8: functional review

Review baseline: `v2.0.0-beta.5` (`c5c09bd`) through Claude's beta 7
(`a1b5918`). The Windows adapter, new shared shell, macOS adapter, playback,
Spotify and updater paths were compared with the Rust and macOS parity plans.
This report separates the Standards and Spec reviews. Cosmetic refactoring was
excluded; fixes require a concrete failing path or an identified missing route.

## Standards

1. **Text-entry shortcuts.** Spotify's inline search was treated as a results
   list. Enter opened the selected result, Delete was consumed, and Shift+A
   ran a list action instead of editing the input. `FocusKind::TextEntry` now
   preserves native editing and submission while keeping global navigation.
2. **Import validation.** Syntactically valid JSON of the wrong shape, including
   invalid media entries, could replace valid collections and then load empty.
   Import now checks the data manifest and the actual collection readers before
   any replacement. Oversized source files produce an error rather than a
   silently incomplete import; Windows-protected credentials are excluded.
3. **Partial import rollback.** A later write failure left earlier files replaced.
   Import now stages all files, makes an exclusive backup and restores originals
   (or removes newly created files) on a failed commit.
4. **Writes after import.** Player shutdown could overwrite imported playback
   positions using the old in-memory state. Playback is stopped and persisted
   before committing; after a successful import, polling, actions and shutdown
   persistence are suppressed until the application exits.
5. **Disabled shortcuts re-enabled at reload.** Normalization discarded empty
   bindings and substituted defaults. A saved disabled background shortcut
   loaded as Ctrl+Space in a failing Windows regression. Explicitly unbound
   actions now remain unbound on both platforms.

The highest-risk Standards findings concern loss of imported user data. Tests
inject wrong shapes and a failure after an earlier replacement, and verify the
original bytes remain intact.

## Spec

1. **Resume last session.** A visible macOS menu entry and Action Finder action
   lacked a handler. Resume now restores the saved item, sequence and return
   context, including playlist/podcast selection.
2. **Diagnostic report.** Its menu entry and shortcut fell through to
   "unavailable". Reports are now generated asynchronously, copied to the
   clipboard and redacted; they include safe Spotify state without account IDs.
3. **Copy link.** The list shortcut lacked a handler. It now shares the existing
   active-item copying path.
4. **Result columns.** Previous/next column shortcuts lacked handlers. They now
   use the shared result-column cursor and announcement model.
5. **Metadata hydration.** Flat YouTube results never received missing dates and
   view counts. Hydration now has generation guards, bounded batches, selected
   row priority, API fallback and deferred updates to a focused row.
6. **Next-stream prefetch.** The setting was displayed but unused on macOS.
   Prefetch now respects both settings, avoids duplicate work and populates the
   same bounded stream cache without blocking playback.
7. **Background player.** Escape kept the session alive but the native adapter
   hid all player controls. The macOS adapter now presents the shared background
   player model alongside the previous screen, retaining list focus.
8. **Global play/pause.** The macOS action existed without native global-key
   registration. The adapter now registers the configured background shortcut.
   Its default is consistently Command+Shift+Space in settings and routing.
   Canonical beta 6/7 Ctrl+Space values migrate to the new macOS default;
   those releases did not distinguish a saved default from an identical custom
   value. Other custom bindings and explicitly unbound actions stay intact.
   Character hotkeys resolve through the active keyboard layout rather than
   hardcoded ANSI positions, and registration refreshes when that layout changes.
9. **Player-focused targets and context menus.** Actions could act on a row
   behind the background player. Playing-item priority now applies to generic
   actions, Spotify Like/Unlike, radio, eligible Hide operations and Shift+F10.
   List focus retains selected-row behavior; a local player does not mutate a
   Spotify row behind it.
10. **Spotify lyrics/transcripts.** The macOS path used only generic lyrics
    providers. Spotify's own timed lyrics and episode transcript route now runs
    first, with the existing generic fallback retained.

The highest-risk Spec findings concern inaccessible playback controls and
actions affecting the wrong item. Regression tests use the production target
selectors without making account mutations.

## Additional verified playback and updater findings

- After Spotify → Escape → YouTube, the first video showed Playing but its
  position stayed at zero. The new item's pause reset was routed to the old
  PCM source, leaving mpv's independent pause flag set. A regular-media load
  replacing PCM now applies the requested pause state to mpv. The real beta 7
  account reproduced the stall; a real-libmpv regression failed with
  `Paused(true), Started` before the fix and played through after it.
  The candidate Windows GUI repeated the same account/search flow and reached
  elapsed 0:26 on its first load. Shuffle enabled in Spotify survived the source
  change: toggling it from the YouTube background controls announced Off.
  A disconnected PCM source also left the old mode set during the replacement
  load, incorrectly failing the new regular item with "the PCM source is gone".
  Mode cleanup now occurs even when the source has disappeared; a separate
  real-libmpv regression reproduced that failure.
- A new item reset shuffle even though other session state was retained.
  Shuffle now retains both manual On and Off until the player area is closed.
  Escape and item replacement preserve it. An explicit Shuffle playlist command
  can enable it; stale saved-item metadata cannot override the current choice.
- Closing a Spotify player destroyed the mpv client without stopping the PCM
  source. Its decoder could keep writing into a bounded stream without a
  consumer. Engine teardown now stops its owned source before destroying mpv;
  explicit Stop and transitions to ordinary media do not stop it twice. A
  real-libmpv teardown regression observed zero stop calls before the fix.
- Single-track Spotify load requests discarded shuffle. Requests now carry the
  retained mode and preserve confirmed repeat during an open PCM session;
  remote attachment follows the confirmed Connect state.
- Startup and manual yt-dlp checks could concurrently replace the same folder.
  The shared shell now permits only one component update worker at a time.
- A macOS yt-dlp file with a Mach-O header but an unusable body was accepted.
  The updater now runs a bounded version check before replacing the working
  component and removes failed staging data.
- The macOS app updater removed its backup before proving the new application
  could launch. It now performs the packaged qualification check before
  discarding the backup and restores the previous bundle if it fails.
- Diagnostic redaction omitted macOS `HOME` and `TMPDIR`. Both roots are now
  redacted using an injectable environment reader covered by a failing test.

## LibreSpot updates

ApricotPlayer links the LibreSpot Rust crates into the application and carries
a local Connect patch. It does not load a separately replaceable LibreSpot
executable or plugin. Replacing an official standalone executable would neither
update the embedded engine nor preserve that patch. Therefore an independent
yt-dlp-style updater or checkbox would be misleading; LibreSpot updates must
ship as tested ApricotPlayer releases. The official latest release checked on
2026-10-13 is [v0.8.0](https://github.com/librespot-org/librespot/releases/tag/v0.8.0),
matching the pinned dependency.

## Verification limits

Final package inspection found 51 bundled Mach-O files with actual
`LC_BUILD_VERSION minos 27.0`, including mpv, libmpv and FFmpeg. The main Rust
executable has minos 11.0; SDK version alone is not used as the requirement.
The packaging script now derives the declared OS minimum from bundled binaries
and signs in private temporary storage to avoid Finder metadata invalidating
the signature. This beta requires macOS 27.0; older-system dependency builds
remain outstanding.

Tests were run red before fixes at production seams, including actual macOS
update scripts, real libmpv and native background controls. A passing unit suite
does not certify complete NVDA/VoiceOver speech, braille or real-account behavior.
Release validation records the final platform-specific results separately.

Native Carbon dispatch reached the correct registered hotkey ID and paused the
isolated macOS player. Computer Use's application-targeted key injection did not
establish a foreground Finder/WindowServer test, so physical global-hotkey input
and complete VoiceOver speech remain separate runtime checks.

The beta 7 account-dependent Liked Songs reopening report did not reproduce in
the actual offline LibreSpot task: repeated loads after playing, paused and
stopped states restarted the first song and preserved the next song. Its test
is retained, but the original account/server case is not claimed fixed.
Spotify Smart Shuffle is also not claimed verified by ordinary boolean-shuffle
tests; ordinary session shuffle and recommendation handling are separate paths.
