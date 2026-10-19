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

- Reporting is optional and default OFF in PlayerConfig and SpotifySettings.
  Both settings paths require explicit Yes to an experimental/account-risk
  warning before enabling. No/Cancel leaves the feature off. Live changes do
  not reconnect or interrupt playback. New consent starts with zero samples.
- A watch channel with consent epochs rejects disabled/stale queued reports
  and cancels in-flight reporting futures on revocation. Already submitted
  records cannot be recalled. An unchanged setting does not restart counters.
- Load request IDs are reserved synchronously, before commands reach the
  decoder thread. Connect sets its expected ID immediately and ignores old
  acknowledgements, EOF and Unavailable events; reselecting the same URI is
  covered. Manual intent tags are captured before async commands and recorded
  before decoder callbacks. The UI listener and PCM bridge reject stale tags,
  including the first delayed old callback; automatic advancement inherits its
  tag and keeps genuine skip warnings. The bounded lookup holds only a weak
  player reference, with no event-listener/player ownership cycle.

- Beta 16: a remote track whose load was interrupted (network, audio key,
  stalled download or a decoder failure caused by those) is tried once more
  after 750 ms before Player reports it Unavailable. Metadata saying the track
  cannot play here is not retried. A Connect load from Apricot that names its
  song remembers that song's decoder request; if exactly that request is
  Unavailable, Connect stops on the song instead of skipping to the next row,
  and Apricot reports "This Spotify item cannot be played". Songs Spotify
  chose (whole albums, Next, natural advancement) still skip as before.
- Beta 20: a load that is still interrupted after its retry keeps its reason.
  Player sends the new LoadingFailed event instead of Unavailable, leaves the
  Loading state, and a failed preload no longer marks the next song
  unavailable. Connect stops on the song without blacklisting its URI for the
  session, so Previous and Next can return to it; Apricot reports "Spotify
  playback was interrupted". Dropping a replaced loader cancels its pending
  retry. The audio key manager keeps up to 256 successful keys of this session
  in memory per track and file, so returning to a song does not request its
  key again, and canceled key requests no longer stay pending.
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
