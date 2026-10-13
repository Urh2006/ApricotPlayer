# Beta 9 focused follow-up audit

Baseline: published v2.0.0-beta.8. Review focused on startup device checks,
packaged paths, converter and playback entry points, and the macOS CI package
workflow. This is a targeted follow-up, not a renewed claim of full parity.

## Confirmed functional bug on both platforms

`Shell::poll_saved_audio_device_check` treated a disconnected probe channel as
an empty successful device list. The worker also converted library/probe errors
into that list. This falsely marked a saved output missing. Cancelling the
resulting dialog wrote `auto` to the saved preference.

A regression through the actual shell callback reproduced the saved value
changing from `saved-device` to `auto` before the fix. The worker now preserves
failure as `None`; failure or disconnection completes without changing the
setting or prompting. Tests cover disconnection, explicit failure, and a
successful empty list that must still handle a missing device.

## Packaging gaps

The existing workflow only built the isolated development identity, whose assets
cannot be used for the public application's updater. A manual input now selects
the public beta identity. It remains an artifact workflow and does not publish
partial releases. The public package rejects dirty tracked source and uses
locked Cargo operations.

The helper qualification hardcoded an `.exe` filename, and packaged playback
qualification explicitly rejected macOS. Both now use the platform's packaged
names. CI exercises the archived application and bundled libmpv, not merely the
unpackaged build. The package's actual OS requirement is derived from bundled
Mach-O load commands rather than inferred from the host OS or SDK.

The beta 9 Actions run stayed queued without a runner and was cancelled after
the user explicitly chose a local Mac build instead. The new workflow steps
have not completed on Actions; final package qualification runs locally on the
Mac. The workflow capability is prepared, not certified by this release.

## Limits

No real profile or installation is changed by the CI qualification. This review
does not establish complete screen-reader speech, physical global-key delivery,
Spotify Smart Shuffle, or the beta 7 account-dependent Liked Songs report.
An online GitHub runner also cannot reproduce the user's actual Spotify account
and local audio-device environment.

The reported VoiceOver progress tones were traced to the app-update window's
native `NSProgressIndicator` in `ConversionProgress`. Updates set its numeric
value and message; this path has no sound playback or `NSBeep` call and does not
force per-percent speech. The sole `NSBeep` fallback in the Mac UI belongs to
the unrelated player default-value feedback. No obvious cause of ignoring the
user's VoiceOver speech preference was established, so no speculative change
was made. Actual VoiceOver reproduction remains necessary.
