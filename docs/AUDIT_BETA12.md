# Beta 12 verification

Playlist membership is read from the complete Spotify rootlist. It is independent of an item's public-profile publication attribute. Non-playlist entities retain their existing strict saved-state lookup. Missing or truncated rootlist data is rejected rather than guessed; requested order and repeated playlist URIs are preserved. Unknown playlist membership disables the library operation.

All four My profile rows are navigation links with no fetched count. Windows native LB_GETTEXT checks verify exact labels. The shared shell verifies the same names and Profiles search/Back behavior.

Playback unavailable notices track the active request and URI from Loading, Playing and Paused events. Librespot also sends Unavailable for a future preload with the current request ID; this must not announce failure of the playing song. Event-channel regression checks future-preload failures, replaced/stopped requests, duplicate failures and one real active-load failure. No automatic retry or changes to genuine Spotify skip behavior are introduced.

Historical sanitized diagnostic aggregates contain load errors and HTTP throttling, but do not establish the cause of the user's individual skipped song. No real account, running installation or existing playlists were modified for testing.

Listening-history investigation: the pinned librespot 0.8 implementation publishes Spotify Connect state but does not implement listening-event reporting. Primary upstream discussion https://github.com/librespot-org/librespot/discussions/626 and the maintainer presentation https://fosdem.org/2026/events/attachments/RNBQ8U-reverse-engineering-spotify/slides/267362/reverse_e_xy4vd0r.pdf explain the distinction. Closed experimental PR https://github.com/librespot-org/librespot/pull/1759 reports a Linux history-ingestion test but does not establish On Repeat, recommendation or Wrapped behavior. The patch depends on a separate transport, desktop event compatibility identifiers and changes across core, playback and protocol crates. It is not imported in this release. No artificial listening events are submitted and no personalization improvement is claimed.

Final platform test counts and package identity/digests are recorded during qualification. Synthetic/native tests do not certify complete NVDA or VoiceOver speech. Public release notes describe user-visible changes and the listening-history limitation.
