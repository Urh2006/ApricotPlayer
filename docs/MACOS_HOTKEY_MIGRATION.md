# macOS background shortcut normalization

The shared action catalog defaults background play/pause to `Ctrl+Shift+Space`
on macOS (`Command+Shift+Space` in the UI). Windows keeps `Ctrl+Space`.
Settings defaults, missing-binding fallbacks, the editor, labels and key routing
all read the same platform default. Native registration uses the stored value
without adding modifiers behind the editor's back.

Beta6/7 serialized the complete Windows default map into macOS settings.
The exact canonical `Ctrl+Space` value for `background_play_pause` is normalized
to `Ctrl+Shift+Space` on macOS load. Those versions did not record whether that
identical value was an explicit override or the old default; they cannot be
distinguished. This one legacy canonical value therefore migrates in either
case. Other custom values, including explicit empty/unbound entries, survive
normalization. The normalized value is persisted on the next settings save.

Character global shortcuts resolve virtual keys through the active TIS Unicode
keyboard layout and UCKeyTranslate, with an ASCII-capable layout fallback for
input methods that have no layout data. Input-source notifications re-register
the configured chord. Existing native special-key equivalences remain intact;
there is no US-letter-position table. Native hotkey registration does not add
modifier restrictions that the common editor lacks. OS conflicts remain subject
to exclusive Carbon registration.

The handler subscribes to Pressed and Released and performs one activation per
press edge. An internal Carbon dispatch regression checks repeated Pressed events,
release, and a new press. That proves the installed handler's NoRepeat contract;
it does not prove physical WindowServer delivery. CUA app-targeted key input did
not establish a physical global keyboard test, and no VoiceOver speech pass is
claimed from accessibility-tree observations.
