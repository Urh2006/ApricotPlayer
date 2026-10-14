# Spotify listening-history patch (beta 13)

Baseline: official MIT-licensed librespot 0.8.0, still the latest stable release
when checked on 2026-10-14. Existing Connect patches are preserved. Core,
playback and protocol are local copies; other crates stay pinned to 0.8.0.

Source: https://github.com/librespot-org/librespot/pull/1759
Repository: alexng353/librespot
Commit: fbe04b96faf2616aeebcea2d37ca21754cc96cea
Base: a1b66d3c8a14e55a9572a9e17467150dca618c9a
The PR was closed without being merged. This is an experimental downstream
integration, not an upstream-supported history API.

## ApricotPlayer adaptations

- No dev-branch upgrade. The 0.8.0 local-file loader has no reportable Spotify
  identity. Local and passthrough audio are excluded.
- Sink.write_counted counts PCM samples accepted by the output. The Apricot
  bridge returns accepted s16 bytes / 2, including partial packets; missing or
  cancelled generations count zero. The bounded ring holds at most 200 ms.
  Measurement is at the bridge, not the physical speaker.
- Spotify private-session flags for this Connect device suppress the whole
  affected listen. Leaving private mode does not restart a partial record
  until a new load. Privacy depends on the state Spotify actually supplies.
- Final stop_and_flush runs asynchronously with a ten-second deadline before
  shutting down the authenticated session. On normal application exit only,
  shutdown keeps the runtime alive for up to ten seconds after stopping audio.
  Account replacement does not wait on the UI thread. Queue saturation, network failure,
  rejection or abrupt exit can lose history, without blocking audio.
- Reports retain their original session/generation; account changes cannot
  submit old samples under the new account.
- Accepted events log duration without track/account IDs. Receiver success
  does not prove history ingestion or recommendation changes.
- An obsolete deprecated-lint expectation is allowed for Rust 1.98 compatibility.

Transport: Spotify gabo-receiver-service/v3/events, gzip protobuf. Desktop/schema
compatibility identifiers stay separate from ApricotPlayer's version:
1.2.96.518 / 129600518, with the source patch's pinned SDK/core values.
Retries reuse serialized event/sequence identity. Only completed or interrupted
actual audio is reported, never preloads, synthetic listens, seek distance or
pause time. No durable event/token spool is introduced.

## Manual update review

Read LISTENING-PATCH.json, compare official stable releases and PR1759/fork
changes, review the exact diff, then update only after regression checks.
Review schema/file identity, privacy, session ownership, deduplication, retry
bounds and output accounting. Preserve existing Connect modifications.
Run workspace tests, strict Clippy, scripts/test_listening_patch.py, real libmpv
checks on both platforms and one ordinary real listen with timestamped history
readback where allowed. Do not claim On Repeat, Wrapped or royalties from HTTP
success. Never replace registry files or use a floating fork dependency.
