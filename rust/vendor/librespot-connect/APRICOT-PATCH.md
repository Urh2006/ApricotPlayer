# ApricotPlayer patch of librespot-connect 0.8.0

Source: crates.io `librespot-connect` 0.8.0 (MIT, Copyright (c) 2015 Paul Lietar).
Used through `[patch.crates-io]` in `rust/Cargo.toml` (decision SD-1,
`docs/SPOTIFY_PARITY_MANIFEST.md`). Changes are limited to `src/spirc.rs`
and queue/history functions in `src/state/tracks.rs`:

* `Spirc::player_state()`: a `tokio::sync::watch` receiver of the player state
  this device last sent to Spotify (track, previous and next tracks with `uid`
  and `provider`, `queue_revision`, options). Published after every state
  update and after the device became inactive.
* `Spirc::add_to_queue(uri)`: adds to the manual queue of this device.
* `Spirc::set_queue(next_tracks, expected_revision)`: replaces the next
  tracks only if the queue still has that revision (stock `SetQueue` from a
  remote device has no such check).
* `Spirc::devices()`: a watch receiver of the Connect devices of the account
  from every cluster this device receives (`ConnectDevices`).
* `Spirc::remote_volume()`: the volume a remote device set for this device;
  local changes are not reported, so a player can follow the phone without
  echoing its own volume back.
* `ConnectState::replace_next_tracks` (`src/state/tracks.rs`): used by
  `set_queue`; queued tracks keep their UIDs (the stock remote `SetQueue`
  numbers them anew), so an open queue view still names the same
  occurrences after an edit.
* `ConnectState::next_track` and `prev_track`: smart shuffle recommendations
  participate in playback history, so Previous reaches the recommendation
  between two original playlist tracks and Next retains that occurrence.
* At the beginning of playback history, Previous seeks to zero without Stop
  or a destructive queue reset. `prev_track` leaves the current occurrence
  and next tracks intact when history contains no preceding song.
* Single-track loads retain the song URI as the autoplay seed instead of the
  synthetic web-api URI. Next and natural completion wait for pending context
  pages/recommendations and perform the skip after the authoritative resolver
  has filled the queue. Previous, new loads, transfer and disconnect cancel a
  pending skip. Autoplay Off remains respected.

* Beta 16: `load_with_revision` with a named song records that song's decoder
  request (`selected_request`). An `Unavailable` for that request stops on the
  song (`stop_on_selected_track`) instead of `handle_next`, so the row below
  never starts in place of the selected one. The record ends once the song
  plays.

* Beta 17: `Spirc::play_context_edge(last, revision)` plays the first or last
  song of the current context in the list's own order (an unshuffled copy of
  the context), reading the remaining pages first for the last song. It loads
  through the same path as a named song, so a song that cannot load stops.

* Beta 20: `PlayerEvent::LoadingFailed` (a temporary key, download or decoder
  failure) stops on the current song like a failed chosen song, without
  `handle_unavailable`, so its URI stays in the queue and context.

Upstream: `dev` has `Spirc::add_to_queue` and `clear_queue` and a `SetQueue`
player event without `uid`; a pull request with the confirmed state and the
revision check should replace this copy.

Beta 13 additionally loads the originating context into Player, propagates this
device's Spotify private-session flag and flushes final reports on shutdown.
See ../LISTENING-PATCH.md for the pinned backport and test contract.
- Beta 23: Previous that only returns to the start of the song keeps the
  current decoder request; Connect hands it to the new navigation revision
  with Player::set_request_revision, so Apricot keeps that song's seeks and
  pause changes instead of discarding them as events of an earlier selection.
- 2.1: `ContextResolver::add` measures the time since a failed context
  request the right way round (upstream it was always zero, so a failed
  autoplay request was never retried).
- 2.1: `fill_context_from_page` takes the context type. A further page of
  recommendations is appended to the autoplay context instead of the
  default one (upstream the recommendations started over after the first
  page), without recommendations already in the list, because Spotify is
  told only the last ten songs. Loads from Apricot and the list edges are
  logged at info level for diagnosis.
- 2.1 (review 5): the last song of a list is chosen only when every page
  of the list loaded within its limit, otherwise Player sends
  ListStillLoading (B3); further pages keep their context type, a failed
  recommendation request may be asked again after 30 s, the used request
  leaves the queue before more recommendations are asked for, and pages of
  repeats are asked again at most three times in a row (B4);
  `Spirc::skip_to_upcoming` plays an upcoming recommendation by skipping to
  it, and skipping toward a named song ends when no song is left (B5).
