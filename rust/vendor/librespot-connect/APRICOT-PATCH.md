# ApricotPlayer patch of librespot-connect 0.8.0

Source: crates.io `librespot-connect` 0.8.0 (MIT, Copyright (c) 2015 Paul Lietar).
Used through `[patch.crates-io]` in `rust/Cargo.toml` (decision SD-1,
`docs/SPOTIFY_PARITY_MANIFEST.md`). Only `src/spirc.rs` and one function
in `src/state/tracks.rs` are changed:

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

Upstream: `dev` has `Spirc::add_to_queue` and `clear_queue` and a `SetQueue`
player event without `uid`; a pull request with the confirmed state and the
revision check should replace this copy.
