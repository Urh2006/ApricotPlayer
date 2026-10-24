# Beta 11 verification

Scope: the Spotify profile submenu, account-local main-menu pins, typed pinned
context menus, artist follow-state reads and native Windows integration.

## Shared contract

Main-menu pins are stored per logged-in account in a versioned atomic document.
Reading pins does not require a network session. Unknown document fields are
preserved; malformed input is reported rather than replaced with empty data.
Optional catalog metadata preserves playlist owner and capabilities. Canonical
pin kind/URI must match metadata, and saved/follow state is cleared before reuse.
Categories are refreshed from their parent page/section when reopened. Profile
and Top sections are not eligible for category pins.

## Windows paths

My profile has four local rows. Find friends uses an empty Profiles search with
the profile frame retained underneath, including after submitting a query.
Playlist and artist pins keep their existing context commands; categories only
offer the commands relevant to browsing. Unpinning changes local shortcuts,
never the original Spotify catalog or library membership.

Context-state reads are asynchronous and separate from collection/bulk reads.
They capture account epoch, current view, item URI, frame depth, selected row,
focused control and player generation. A changed target drops the result without
opening a late menu. Unknown artist state clears the old value and disables
follow operations instead of guessing. Pinned playlists refresh edit permissions
before their menu is shown. Playing-song artist commands use captured media
metadata rather than the list behind the player.

The opt-in native Win32 regression uses actual hidden controls, synthetic rows
and an isolated account store. It exercises hub order, offline pins, playlist
and category activation/menu differences, profile child routes, Profiles search
and one-step Back, track artist metadata, one follow command on artist rows,
unknown follow state, stale replies, section refresh requests and unpinning.

## Acceptance limits

Native control tests do not certify complete NVDA/VoiceOver speech. Live account
read probes and final platform results are recorded separately during release
qualification. No real account mutations or installation changes are implied
by synthetic fixtures. Playlist publication on a public profile is a separate
operation from playlist access permissions; it must not be implemented by
aliasing the existing Public/Private control.

## Public-profile publication

Primary web-player evidence: `web-player.1eb7daa6.js` library
`setPublishedState` builds `UPDATE_ITEM_ATTRIBUTES` on the rootlist, targeting
the exact item URI and setting `newAttributes.values.public`. This is distinct
from `/playlist-permission/v1/.../permission/base`. Read-only account probes
confirmed that explicit rootlist `public:true` URIs match both the published
rootlist and the actual profile playlist pages. The profile endpoint's reported
total was inconsistent across pages; actual URI membership was compared.

The new operation reads the current revision and explicit target state, rejects
missing/ambiguous/incomplete targets, checks access permissions before publishing,
posts the verified attribute operation and requires matching readback. It does
not silently save a missing playlist or change private access to public. Making
a known published playlist private unpublishes it first, matching the web client.
Unknown states and unsupported targets remain disabled. Operation-shape and
unknown-state regression tests pass; no live publication mutation is claimed.

Publication and artist-follow menus send explicit desired states, so concurrent
changes on another device cannot reverse the visible action. Publication readback
checks the rootlist flag directly, including removal after access became private.
Native Windows and shell regressions confirm artist follow results preserve
marked songs and use follow announcements instead of Liked Songs messages.
The read-only Windows account probe returned VIEWER from the base-permission
endpoint for owned and foreign playlists with either publication flag. It made
no account changes and did not start a Connect player; private BLOCKED access
was not represented in those four sampled categories.
