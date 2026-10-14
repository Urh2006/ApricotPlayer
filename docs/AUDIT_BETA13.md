# Beta 13: genuine Spotify listening reporting

The user authorized integration and a joint Windows/macOS beta release on
2026-10-14. No periodic update automation was requested: later update reviews
are initiated manually by the user.

## Implementation and provenance

Official librespot 0.8.0 remains the pinned stable baseline (latest official
release checked on 2026-10-14). The exact PR1759 backport, original crate
checksums and manual update contract are in rust/vendor/LISTENING-PATCH.json
and LISTENING-PATCH.md. The closed/unmerged PR is experimental downstream code.
Existing ApricotPlayer Connect, queue, autoplay and beta 11/12 fixes remain.

Reporting is optional and default OFF. Both Spotify settings paths require
explicit Yes after an experimental/account-risk warning; No/Cancel leaves it
off. A live watch channel cancels in-flight work and rejects queued consent
epochs on revocation. Enabling starts with zero samples; no past audio is sent.

Actual accepted PCM supplies ms_played, with the selected Spotify file/audio
identity and originating context. No preload, seek distance or pause time is
added. Missing/cancelled bridge generations count zero; partial packets count
only accepted bytes. The bridge has a bounded 200 ms buffer; this is output
accounting, not measurement at physical speakers. Local/passthrough audio is
excluded. Known private-session state suppresses the whole affected listen.

A bounded asynchronous queue owns the original authenticated session. Failed
reports do not stop audio or later reports. Receiver-level transient retries
reuse the serialized payload and sequence identity. On normal application exit
only, audio is stopped and the runtime is kept alive for at most ten seconds
for a final flush. Account replacement remains asynchronous. Abrupt exit,
queue saturation or rejection can lose history. No cleartext credential cache,
event spool, profile migration, artificial listens or playback loops are added.

## Automated and Mac runtime evidence

- Targeted shared Spotify, application and shell tests: 466 passed, 0 failed.
- Vendored report tests: 14 passed, 0 failed, including disabled/stale consent
  epochs, cancellation of in-flight work, rendered duration,
  pause/seek, private-session suppression, old/new playback statistics,
  same-payload retry, permanent rejection, full-queue flush and recovery after
  transport failure. rust/scripts/test_listening_patch.py reproduces these in
  an isolated workspace without account/event submissions.
- History readback example: two tests reject old receipts and wrong tracks.
- Strict workspace/all-targets Clippy and git diff --check pass.
- A new real Mac mpv test passed rapid PCM replacements with nondefault pitch,
  then ordinary YouTube/SoundCloud/local provider paths using offline media.
  Existing real libmpv tests cover PCM transitions, pause/seek, speed,
  pitch/EQ, replacement failures, natural completion and source shutdown.
- Isolated native AppKit warning: No is default; Yes and Cancel return the
  correct responses without settings/account changes.
- Isolated real AppKit audit: Escape, 13 accessible player buttons, pause,
  focus preservation and Close passed. This is not a full VoiceOver speech
  or NVDA certification.

Release packaging and Windows qualification use this same final source, with
normal installer/portable/DMG/ZIP gates, signatures, build-info and asset hashes.
They must pass before publication; development test results alone do not
qualify a release package.

## Selection and rapid navigation fixes

An old EOF could arrive after Connect selected a new context row but before
the decoder acknowledged its new request ID. IDs are now reserved before
queueing; Connect fences old acknowledgements and callbacks immediately.
Thirteen Connect transport regressions pass, including reselecting the same URI
and rejecting old EOF/Unavailable, with current EOF still advancing once.
Manual intent revisions are captured before commands enter the asynchronous
queue and registered against decoder request IDs before the decoder can emit
callbacks. Both the UI listener and PCM bridge use that association, so even
the first delayed old Loading/Playing callback cannot reopen an old stream or
replace the selected row. The bounded lookup captures a Weak<Player>, avoiding
a player/event-listener ownership cycle. Manual navigation also invalidates
queued or late old Playing/Unavailable notices. Automatic advancement retains the genuine failure warning; filtering
only by current URI would incorrectly suppress that case. Affected code is
Spotify-only; the offline real-mpv provider transition test checks the common
engine without making website requests or generating Spotify listen events.

## What is and is not proven about Spotify

The Windows read-only baseline attempt for official timestamped recently-played
returned HTTP 429. No repeated polling or fake events were used. History must
be confirmed by a new matching track URI and played_at timestamp after an
ordinary genuine listen, not an old receipt or a reordered context list.

A successful event receiver response only proves transport acceptance. Spotify
controls ingestion, taste-profile exclusions, recommendations, On Repeat,
Wrapped and royalties. None of those is guaranteed by this implementation or
by the upstream author's separate Linux test. Public notes describe the
feature as experimental and make no immediate recommendation/statistics promise.
