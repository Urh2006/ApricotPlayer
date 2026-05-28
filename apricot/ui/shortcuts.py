from apricot.constants import *
import wx
import os
from pathlib import Path

# ---------------------------------------------------------------------------
# Hot-path constants — built once at import time.
#
# shortcut_matches() is called 30+ times per keypress in on_char_hook.
# Each call chains down to shortcut_key_code(), which previously rebuilt a
# 30-entry alias dict and compiled two regex patterns on every invocation.
# Moving them here reduces per-keypress regex overhead to near zero.
# ---------------------------------------------------------------------------

_RE_SHORTCUT_SPACES = re.compile(r"\s+")
_RE_SHORTCUT_FKEY   = re.compile(r"f(\d{1,2})")
_RE_SHORTCUT_ALT    = re.compile(r"\s*\|\s*")

_SHORTCUT_KEY_ALIASES: dict[str, int] = {
    "enter":                  wx.WXK_RETURN,
    "return":                 wx.WXK_RETURN,
    "space":                  wx.WXK_SPACE,
    "spacebar":               wx.WXK_SPACE,
    "escape":                 wx.WXK_ESCAPE,
    "esc":                    wx.WXK_ESCAPE,
    "delete":                 wx.WXK_DELETE,
    "del":                    wx.WXK_DELETE,
    "backspace":              wx.WXK_BACK,
    "back":                   wx.WXK_BACK,
    "insert":                 wx.WXK_INSERT,
    "ins":                    wx.WXK_INSERT,
    "home":                   wx.WXK_HOME,
    "end":                    wx.WXK_END,
    "pageup":                 wx.WXK_PAGEUP,
    "page up":                wx.WXK_PAGEUP,
    "pagedown":               wx.WXK_PAGEDOWN,
    "page down":              wx.WXK_PAGEDOWN,
    "left":                   wx.WXK_LEFT,
    "left arrow":             wx.WXK_LEFT,
    "right":                  wx.WXK_RIGHT,
    "right arrow":            wx.WXK_RIGHT,
    "up":                     wx.WXK_UP,
    "up arrow":               wx.WXK_UP,
    "down":                   wx.WXK_DOWN,
    "down arrow":             wx.WXK_DOWN,
    "applications":           getattr(wx, "WXK_WINDOWS_MENU", getattr(wx, "WXK_MENU", getattr(wx, "WXK_APPS", -1))),
    "application":            getattr(wx, "WXK_WINDOWS_MENU", getattr(wx, "WXK_MENU", getattr(wx, "WXK_APPS", -1))),
    "apps":                   getattr(wx, "WXK_WINDOWS_MENU", getattr(wx, "WXK_MENU", getattr(wx, "WXK_APPS", -1))),
    "menu":                   getattr(wx, "WXK_WINDOWS_MENU", getattr(wx, "WXK_MENU", getattr(wx, "WXK_APPS", -1))),
    "context menu":           getattr(wx, "WXK_WINDOWS_MENU", getattr(wx, "WXK_MENU", getattr(wx, "WXK_APPS", -1))),
    "[":                      VK_OEM_4_LEFT_BRACKET,
    "leftbracket":            VK_OEM_4_LEFT_BRACKET,
    "left bracket":           VK_OEM_4_LEFT_BRACKET,
    "openbracket":            VK_OEM_4_LEFT_BRACKET,
    "open bracket":           VK_OEM_4_LEFT_BRACKET,
    "physical left bracket":  VK_OEM_4_LEFT_BRACKET,
    "]":                      VK_OEM_6_RIGHT_BRACKET,
    "rightbracket":           VK_OEM_6_RIGHT_BRACKET,
    "right bracket":          VK_OEM_6_RIGHT_BRACKET,
    "closebracket":           VK_OEM_6_RIGHT_BRACKET,
    "close bracket":          VK_OEM_6_RIGHT_BRACKET,
    "physical right bracket": VK_OEM_6_RIGHT_BRACKET,
}


class ShortcutsUI:
    def shortcut_for(self, action: str) -> str:
        shortcuts = getattr(self.settings, "keyboard_shortcuts", {}) or {}
        value = str(shortcuts.get(action) or DEFAULT_KEYBOARD_SHORTCUTS.get(action) or "").strip()
        return value or DEFAULT_KEYBOARD_SHORTCUTS.get(action, "")

    def menu_label_with_shortcut(self, label_key: str, action: str) -> str:
        shortcut = self.shortcut_for(action)
        return f"{self.t(label_key)}\t{shortcut}" if shortcut else self.t(label_key)

    def shortcut_to_accelerator(self, shortcut: str) -> tuple[int, int] | None:
        parsed = self.parse_shortcut(shortcut)
        if not parsed:
            return None
        ctrl, shift, alt, key_name = parsed
        key_code = self.shortcut_key_code(key_name)
        if key_code is None or key_code < 0:
            return None
        flags = 0
        if ctrl:
            flags |= wx.ACCEL_CTRL
        if shift:
            flags |= wx.ACCEL_SHIFT
        if alt:
            flags |= wx.ACCEL_ALT
        return flags, key_code

    @staticmethod
    def parse_shortcut(shortcut: str) -> tuple[bool, bool, bool, str] | None:
        text = str(shortcut or "").strip()
        if not text:
            return None
        text = text.split("|", 1)[0].strip()
        parts = [part.strip() for part in text.replace("-", "+").split("+") if part.strip()]
        if not parts:
            return None
        ctrl = shift = alt = False
        key_parts: list[str] = []
        for part in parts:
            normalized = part.lower().replace(" ", "")
            if normalized in {"ctrl", "control", "strg"}:
                ctrl = True
            elif normalized in {"shift", "shft"}:
                shift = True
            elif normalized in {"alt", "option"}:
                alt = True
            else:
                key_parts.append(part)
        if not key_parts:
            return None
        return ctrl, shift, alt, " ".join(key_parts).strip()

    @staticmethod
    def shortcut_key_code(key_name: str) -> int | None:
        normalized = key_name.strip().lower().replace("_", " ").replace("-", " ")
        normalized = _RE_SHORTCUT_SPACES.sub(" ", normalized)
        if normalized in _SHORTCUT_KEY_ALIASES:
            return _SHORTCUT_KEY_ALIASES[normalized]
        match = _RE_SHORTCUT_FKEY.fullmatch(normalized)
        if match:
            number = int(match.group(1))
            if 1 <= number <= 24:
                return wx.WXK_F1 + number - 1
        if len(normalized) == 1:
            return ord(normalized.upper())
        return None

    @staticmethod
    def shortcut_name_for_key_code(key_code: int, unicode_key: int = 0) -> str:
        names = {
            wx.WXK_RETURN: "Enter",
            wx.WXK_NUMPAD_ENTER: "Enter",
            wx.WXK_SPACE: "Space",
            wx.WXK_ESCAPE: "Escape",
            wx.WXK_DELETE: "Delete",
            wx.WXK_BACK: "Backspace",
            wx.WXK_INSERT: "Insert",
            wx.WXK_HOME: "Home",
            wx.WXK_END: "End",
            wx.WXK_PAGEUP: "PageUp",
            wx.WXK_PAGEDOWN: "PageDown",
            wx.WXK_LEFT: "Left",
            wx.WXK_RIGHT: "Right",
            wx.WXK_UP: "Up",
            wx.WXK_DOWN: "Down",
            getattr(wx, "WXK_APPS", -1): "Applications",
            getattr(wx, "WXK_MENU", -1): "Applications",
            getattr(wx, "WXK_WINDOWS_MENU", -1): "Applications",
            VK_OEM_4_LEFT_BRACKET: "LeftBracket",
            VK_OEM_6_RIGHT_BRACKET: "RightBracket",
        }
        if key_code in names:
            return names[key_code]
        if wx.WXK_F1 <= key_code <= wx.WXK_F24:
            return f"F{key_code - wx.WXK_F1 + 1}"
        if ord("A") <= key_code <= ord("Z"):
            return chr(key_code)
        if ord("0") <= key_code <= ord("9"):
            return chr(key_code)
        if unicode_key and 32 <= unicode_key < 127:
            return chr(unicode_key).upper()
        return ""

    def shortcut_from_key_event(self, event: wx.KeyEvent) -> str:
        key_code = event.GetKeyCode()
        modifier_keys = {wx.WXK_TAB, getattr(wx, "WXK_CONTROL", -1), getattr(wx, "WXK_SHIFT", -1), getattr(wx, "WXK_ALT", -1)}
        if key_code in modifier_keys:
            return ""
        raw_key_code = 0
        try:
            raw_key_code = int(event.GetRawKeyCode())
        except Exception:
            raw_key_code = 0
        if raw_key_code in (VK_OEM_4_LEFT_BRACKET, VK_OEM_6_RIGHT_BRACKET):
            key_name = self.shortcut_name_for_key_code(raw_key_code, event.GetUnicodeKey())
        else:
            key_name = self.shortcut_name_for_key_code(key_code, event.GetUnicodeKey())
        if not key_name:
            return ""
        parts: list[str] = []
        if event.ControlDown():
            parts.append("Ctrl")
        if event.ShiftDown():
            parts.append("Shift")
        if event.AltDown():
            parts.append("Alt")
        parts.append(key_name)
        return "+".join(parts)

    def on_shortcut_capture_key(self, event: wx.KeyEvent, control: wx.TextCtrl) -> None:
        if event.GetKeyCode() == wx.WXK_TAB:
            event.Skip()
            return
        action = str(getattr(control, "_apricot_shortcut_action", "") or "")
        if (
            event.GetKeyCode() == getattr(wx, "WXK_SPACE", ord(" "))
            and not event.ControlDown()
            and not event.ShiftDown()
            and not event.AltDown()
            and action != "player_play_pause"
        ):
            return
        shortcut = self.shortcut_from_key_event(event)
        if not shortcut:
            event.Skip()
            return
        conflict = self.shortcut_conflict(shortcut, action)
        if conflict:
            message = self.t("shortcut_in_use", shortcut=shortcut, action=self.t(conflict[1]))
            wx.MessageBox(message, self.t("shortcut_in_use_title"), wx.OK | wx.ICON_WARNING)
            self.speak_text(message)
            control.SetFocus()
            return
        control.ChangeValue(shortcut)
        control.SetInsertionPointEnd()
        control.SetFocus()
        if action:
            self.shortcut_editor_values[action] = shortcut
            self.update_shortcut_action_label(action)
        self.speak_text(self.t("shortcut_captured", shortcut=shortcut))

    def shortcut_label_key(self, wanted_action: str) -> str:
        for action, label_key in SHORTCUT_DEFINITIONS:
            if action == wanted_action:
                return label_key
        return wanted_action

    def shortcut_display_label(self, action: str, shortcut: str) -> str:
        label = self.t(self.shortcut_label_key(action))
        return f"{label}: {shortcut or DEFAULT_KEYBOARD_SHORTCUTS.get(action, '')}"

    def sync_shortcut_editor_value(self) -> None:
        if not hasattr(self, "controls"):
            return
        control = self.controls.get("shortcut_active_value")
        if not isinstance(control, wx.TextCtrl):
            return
        action = str(getattr(control, "_apricot_shortcut_action", "") or self.shortcut_editor_current_action)
        if action:
            self.shortcut_editor_current_action = action
            self.shortcut_editor_values[action] = control.GetValue().strip() or DEFAULT_KEYBOARD_SHORTCUTS.get(action, "")

    def update_shortcut_action_label(self, action: str) -> None:
        control = self.controls.get("shortcut_action_list") if hasattr(self, "controls") else None
        if not isinstance(control, wx.ListBox) or action not in self.shortcut_editor_actions:
            return
        index = self.shortcut_editor_actions.index(action)
        try:
            control.SetString(index, self.shortcut_display_label(action, self.shortcut_editor_values.get(action, "")))
        except Exception:
            pass

    def on_shortcut_action_selected(self, _event) -> None:
        self.sync_shortcut_editor_value()
        list_control = self.controls.get("shortcut_action_list") if hasattr(self, "controls") else None
        value_control = self.controls.get("shortcut_active_value") if hasattr(self, "controls") else None
        if not isinstance(list_control, wx.ListBox) or not isinstance(value_control, wx.TextCtrl):
            return
        index = list_control.GetSelection()
        if not (0 <= index < len(self.shortcut_editor_actions)):
            return
        action = self.shortcut_editor_actions[index]
        self.shortcut_editor_current_action = action
        shortcut = self.shortcut_editor_values.get(action) or DEFAULT_KEYBOARD_SHORTCUTS.get(action, "")
        value_control.ChangeValue(shortcut)
        value_control.SetName(f"{self.t('shortcut_value')}. {self.t(self.shortcut_label_key(action))}. {self.t('shortcut_capture_hint')}")
        setattr(value_control, "_apricot_shortcut_action", action)

    def canonical_shortcut(self, shortcut: str) -> str:
        parsed = self.parse_shortcut(shortcut)
        if not parsed:
            return ""
        ctrl, shift, alt, key_name = parsed
        key_code = self.shortcut_key_code(key_name)
        if key_code is None or key_code < 0:
            return ""
        key_label = self.shortcut_name_for_key_code(key_code)
        if not key_label:
            key_label = key_name.strip()
        parts: list[str] = []
        if ctrl:
            parts.append("Ctrl")
        if shift:
            parts.append("Shift")
        if alt:
            parts.append("Alt")
        parts.append(key_label)
        return "+".join(parts).lower()

    def shortcut_conflict(self, shortcut: str, current_action: str = "") -> tuple[str, str] | None:
        wanted = self.canonical_shortcut(shortcut)
        if not wanted:
            return None
        values = self.current_shortcut_values_from_controls()
        for action, label_key in SHORTCUT_DEFINITIONS:
            if action == current_action:
                continue
            if self.canonical_shortcut(values.get(action) or "") == wanted:
                return action, label_key
        return None

    def current_shortcut_values_from_controls(self) -> dict[str, str]:
        values = self.normalized_keyboard_shortcuts(getattr(self.settings, "keyboard_shortcuts", {}) or {})
        if hasattr(self, "controls"):
            if "shortcut_action_list" in self.controls and "shortcut_active_value" in self.controls:
                self.sync_shortcut_editor_value()
                values.update(self.shortcut_editor_values)
            else:
                for action, _label_key in SHORTCUT_DEFINITIONS:
                    control = self.controls.get(f"shortcut_{action}")
                    if isinstance(control, wx.TextCtrl):
                        values[action] = control.GetValue().strip() or DEFAULT_KEYBOARD_SHORTCUTS[action]
        return values

    def validate_shortcut_controls(self) -> bool:
        has_shortcut_editor = hasattr(self, "controls") and "shortcut_action_list" in self.controls and "shortcut_active_value" in self.controls
        has_legacy_controls = hasattr(self, "controls") and any(f"shortcut_{action}" in self.controls for action, _label_key in SHORTCUT_DEFINITIONS)
        if not has_shortcut_editor and not has_legacy_controls:
            return True
        values = self.current_shortcut_values_from_controls()
        seen: dict[str, tuple[str, str]] = {}
        for action, label_key in SHORTCUT_DEFINITIONS:
            canonical = self.canonical_shortcut(values.get(action) or "")
            if not canonical:
                continue
            if canonical in seen:
                _other_action, other_label_key = seen[canonical]
                shortcut = values.get(action) or DEFAULT_KEYBOARD_SHORTCUTS[action]
                message = self.t("shortcut_in_use", shortcut=shortcut, action=self.t(other_label_key))
                wx.MessageBox(message, self.t("shortcut_in_use_title"), wx.OK | wx.ICON_WARNING)
                self.speak_text(message)
                if has_shortcut_editor:
                    list_control = self.controls.get("shortcut_action_list")
                    value_control = self.controls.get("shortcut_active_value")
                    if isinstance(list_control, wx.ListBox) and action in self.shortcut_editor_actions:
                        list_control.SetSelection(self.shortcut_editor_actions.index(action))
                        self.on_shortcut_action_selected(None)
                    if isinstance(value_control, wx.TextCtrl):
                        self.safe_set_focus(value_control)
                else:
                    control = self.controls.get(f"shortcut_{action}") if hasattr(self, "controls") else None
                    if isinstance(control, wx.TextCtrl):
                        self.safe_set_focus(control)
                return False
            seen[canonical] = (action, label_key)
        return True

    @staticmethod
    def is_shortcut_capture_control(control: wx.Window | None) -> bool:
        return isinstance(control, wx.TextCtrl) and bool(getattr(control, "_apricot_shortcut_capture", False))

    def shortcut_matches(self, event: wx.KeyEvent, action: str) -> bool:
        return self.event_matches_shortcut(event, self.shortcut_for(action))

    def shortcut_is_plain_printable(self, action: str) -> bool:
        parsed = self.parse_shortcut(self.shortcut_for(action))
        if not parsed:
            return False
        ctrl, shift, alt, key_name = parsed
        return not ctrl and not alt and len(key_name.strip()) == 1 and key_name.strip().isprintable()

    def event_matches_shortcut(self, event: wx.KeyEvent, shortcut: str) -> bool:
        alternatives = _RE_SHORTCUT_ALT.split(str(shortcut or ""))
        return any(self.event_matches_single_shortcut(event, alternative) for alternative in alternatives if alternative.strip())

    def event_matches_single_shortcut(self, event: wx.KeyEvent, shortcut: str) -> bool:
        parsed = self.parse_shortcut(shortcut)
        if not parsed:
            return False
        ctrl, shift, alt, key_name = parsed
        if bool(event.ControlDown()) != ctrl or bool(event.ShiftDown()) != shift or bool(event.AltDown()) != alt:
            return False
        key_code = self.shortcut_key_code(key_name)
        if key_code is None:
            return False
        event_codes = self.key_event_codes(event)
        if key_code == wx.WXK_RETURN and wx.WXK_NUMPAD_ENTER in event_codes:
            return True
        if wx.WXK_F1 <= key_code <= wx.WXK_F24:
            raw_vk = 0x70 + (key_code - wx.WXK_F1)
            return self.event_key_code(event) == key_code or self.event_raw_key_code(event) == raw_vk
        if len(key_name.strip()) == 1 and key_name.strip().isprintable():
            return self.key_event_matches_letter(event, key_name.strip()) if key_name.strip().isalpha() else key_code in event_codes
        return key_code in event_codes

    def context_menu_shortcut_matches(self, event: wx.KeyEvent) -> bool:
        context_codes = {
            getattr(wx, "WXK_APPS", -1),
            getattr(wx, "WXK_MENU", -1),
            getattr(wx, "WXK_WINDOWS_MENU", -1),
        }
        return self.shortcut_matches(event, "context_menu") or event.GetKeyCode() in context_codes or (event.GetKeyCode() == wx.WXK_F10 and event.ShiftDown())

    def open_notification_center_shortcut(self) -> None:
        self.run_global_navigation_shortcut(self.show_notification_center)

    def run_global_navigation_shortcut(self, handler) -> None:
        self.leave_player_for_global_navigation()
        handler()

    def open_main_menu_shortcut(self) -> None:
        self.run_global_navigation_shortcut(self.show_main_menu)

    def open_play_from_folder_shortcut(self) -> None:
        self.run_global_navigation_shortcut(self.show_play_from_folder)

    def open_direct_link_shortcut(self) -> None:
        self.run_global_navigation_shortcut(self.show_direct_link)

    def open_bookmarks_shortcut(self) -> None:
        self.run_global_navigation_shortcut(self.show_bookmarks)

    def subscribe_shortcut(self) -> None:
        if self.in_main_menu:
            return
        self.subscribe_to_selected_channel(self.active_item())

    def unsubscribe_shortcut(self) -> None:
        if self.in_main_menu:
            return
        if self.subscriptions_screen_active:
            self.remove_subscription()
            return
        self.unsubscribe_from_selected_channel(self.active_item())

    def open_playback_queue_shortcut(self) -> None:
        self.show_playback_queue()

    def background_play_pause_shortcut(self) -> None:
        if self.player_is_active():
            self.player_play_pause()
            return
        self.announce_player(self.t("no_player"))

    def player_shortcuts_allowed(self, focus: wx.Window | None = None) -> bool:
        if self.focus_in_results_control(focus):
            return True
        if self.in_player_screen and not self.focus_accepts_text(focus):
            return self.focus_in_player_controls(focus)
        return self.focus_in_player_controls(focus) or self.focus_in_background_player_controls(focus)

    @staticmethod
    def event_key_code(event: wx.KeyEvent) -> int:
        try:
            return int(event.GetKeyCode())
        except Exception:
            return -1

    @staticmethod
    def event_raw_key_code(event: wx.KeyEvent) -> int:
        getter = getattr(event, "GetRawKeyCode", None)
        if not getter:
            return -1
        try:
            return int(getter())
        except Exception:
            return -1

    def shortcut_allowed_for_focus(self, action: str, focus: wx.Window | None) -> bool:
        return not (self.focus_accepts_text(focus) and self.shortcut_is_plain_printable(action))

    def handle_global_navigation_shortcut(self, event: wx.KeyEvent, focus: wx.Window | None = None) -> bool:
        focus = focus or wx.Window.FindFocus()
        actions = [
            ("open_main_menu", self.open_main_menu_shortcut),
            ("open_search", self.open_search_shortcut),
            ("open_play_from_folder", self.open_play_from_folder_shortcut),
            ("open_direct_link", self.open_direct_link_shortcut),
            ("open_favorites", self.open_favorites_shortcut),
            ("open_bookmarks", self.open_bookmarks_shortcut),
            ("open_playlists", self.open_playlists_shortcut),
            ("open_subscriptions", self.open_subscriptions_shortcut),
            ("open_current_downloads", self.open_current_downloads_shortcut),
            ("open_history", self.open_history_shortcut),
            ("open_podcasts_rss", self.open_podcasts_rss_shortcut),
            ("open_settings", self.open_settings_shortcut),
            ("open_playback_queue", self.open_playback_queue_shortcut),
            ("new_subscription_videos", self.open_notification_center_shortcut),
            ("background_play_pause", self.background_play_pause_shortcut),
            ("copy_diagnostic_report", self.copy_diagnostic_report),
        ]
        for action, handler in actions:
            if self.shortcut_matches(event, action) and self.shortcut_allowed_for_focus(action, focus):
                handler()
                return True
        return False

    def player_details_shortcut_matches(self, event: wx.KeyEvent) -> bool:
        if self.shortcut_matches(event, "player_details"):
            return True
        return (
            not event.ControlDown()
            and not event.ShiftDown()
            and not event.AltDown()
            and self.is_function_key_event(event, 7)
        )

    def handle_player_shortcut_event(self, event: wx.KeyEvent, focus: wx.Window | None, details_has_focus: bool = False) -> bool:
        if not (self.player_control_mode and self.player_shortcuts_allowed(focus)):
            return False
        if (
            (self.is_function_key_event(event, 1) or self.is_function_key_event(event, 5))
            and not event.ControlDown()
            and not event.ShiftDown()
            and not event.AltDown()
        ):
            event.Skip()
            return True
        if self.focus_in_results_control(focus):
            if self.shortcut_matches(event, "player_previous"):
                self.play_relative_item(-1, preserve_focus=True)
                return True
            if self.shortcut_matches(event, "player_next_related"):
                self.play_related_item()
                return True
            if self.shortcut_matches(event, "player_next"):
                self.play_relative_item(1, preserve_focus=True)
                return True
            if self.results_list_owns_key(event):
                event.Skip()
                wx.CallAfter(self.maybe_extend_results)
                return True
            # OLD behaviour: when focus is in results, do not fall through into
            # player-checkbox/edit-mode/details-navigation handlers. Return False so
            # on_char_hook can dispatch the results-specific shortcuts (add_favorite,
            # queue_audio, play_selected, etc.) from its own branch.
            return False
        if self.context_menu_shortcut_matches(event):
            self.open_player_context_menu()
            return True
        player_checkboxes = {
            getattr(self, "fullscreen_checkbox", None),
            getattr(self, "repeat_checkbox", None),
            getattr(self, "bass_boost_checkbox", None),
            getattr(self, "session_autoplay_checkbox", None),
        }
        player_checkboxes.discard(None)
        if focus in player_checkboxes and event.GetKeyCode() in {wx.WXK_RETURN, wx.WXK_NUMPAD_ENTER}:
            if focus is getattr(self, "fullscreen_checkbox", None):
                self.request_player_fullscreen_checkbox_toggle()
            elif focus is getattr(self, "repeat_checkbox", None):
                self.toggle_repeat()
            elif focus is getattr(self, "bass_boost_checkbox", None):
                self.toggle_bass_boost()
            elif focus is getattr(self, "session_autoplay_checkbox", None):
                val = not focus.GetValue()
                focus.SetValue(val)
                self.on_session_autoplay_next_changed(None)
            return True
        if focus in player_checkboxes and self.shortcut_matches(event, "player_play_pause"):
            event.Skip()
            return True
        if details_has_focus and self.details_text_navigation_key(event):
            event.Skip()
            return True
        if self.shortcut_matches(event, "player_output_devices"):
            self.show_output_devices()
            return True
        if self.shortcut_matches(event, "player_copy_link"):
            self.copy_current_player_url()
            return True
        if self.shortcut_matches(event, "player_copy_timestamp_link"):
            self.copy_current_player_timestamp_url()
            return True
        if self.shortcut_matches(event, "open_channel"):
            self.open_item_channel(self.current_video_item or self.current_video_info)
            return True
        if self.shortcut_matches(event, "player_equalizer"):
            self.show_player_equalizer()
            return True
        if self.shortcut_matches(event, "player_replaygain"):
            self.cycle_replaygain_mode()
            return True
        if self.shortcut_matches(event, "player_add_bookmark"):
            self.add_current_bookmark()
            return True
        if self.shortcut_matches(event, "player_bookmarks"):
            self.show_player_bookmarks()
            return True
        if self.shortcut_matches(event, "player_chapters"):
            self.show_chapters()
            return True
        if self.shortcut_matches(event, "player_transcript"):
            self.show_transcript()
            return True
        if self.shortcut_matches(event, "player_lyrics"):
            self.show_lyrics()
            return True
        if self.shortcut_matches(event, "player_comments"):
            self.show_comments()
            return True
        if self.shortcut_matches(event, "player_previous_chapter"):
            self.seek_relative_chapter(-1)
            return True
        if self.shortcut_matches(event, "player_next_chapter"):
            self.seek_relative_chapter(1)
            return True
        if self.shortcut_matches(event, "player_edit_mode"):
            self.toggle_edit_mode()
            return True
        if self.shortcut_matches(event, "player_save_edit_copy"):
            self.save_edited_local_file(replace_original=False)
            return True
        if self.shortcut_matches(event, "player_replace_edit_original"):
            self.save_edited_local_file(replace_original=True)
            return True
        if self.shortcut_matches(event, "player_marker_start"):
            self.set_clip_marker_async("start")
            return True
        if self.shortcut_matches(event, "player_marker_end"):
            self.set_clip_marker_async("end")
            return True
        if self.shortcut_matches(event, "player_preview_marked_clip"):
            self.preview_marked_clip()
            return True
        if self.shortcut_matches(event, "player_previous"):
            self.play_relative_item(-1, preserve_focus=True)
            return True
        if self.shortcut_matches(event, "player_next_related"):
            self.play_related_item()
            return True
        if self.shortcut_matches(event, "player_next"):
            self.play_relative_item(1, preserve_focus=True)
            return True
        if self.shortcut_matches(event, "player_volume_boost"):
            self.toggle_volume_boost()
            return True
        if self.shortcut_matches(event, "player_bass_boost"):
            self.toggle_bass_boost()
            return True
        if self.shortcut_matches(event, "player_repeat"):
            self.toggle_repeat()
            return True
        if self.shortcut_matches(event, "player_shuffle"):
            self.toggle_shuffle()
            return True
        if self.shortcut_matches(event, "player_play_pause"):
            self.player_play_pause()
            return True
        if self.shortcut_matches(event, "player_time"):
            self.announce_time_async()
            return True
        if self.shortcut_matches(event, "player_speed_down"):
            self.change_speed_async(-self.speed_step_value())
            return True
        if self.shortcut_matches(event, "player_speed_up"):
            self.change_speed_async(self.speed_step_value())
            return True
        if self.shortcut_matches(event, "player_pitch_up"):
            self.change_pitch_async(self.pitch_step_value())
            return True
        if self.shortcut_matches(event, "player_pitch_down"):
            self.change_pitch_async(-self.pitch_step_value())
            return True
        if self.player_details_shortcut_matches(event):
            self.show_video_details()
            return True
        if self.shortcut_matches(event, "player_volume_status"):
            self.announce_volume_async()
            return True
        if self.shortcut_matches(event, "player_seek_back_huge"):
            self.start_player_seek_hold(-600, event)
            return True
        if self.shortcut_matches(event, "player_seek_forward_huge"):
            self.start_player_seek_hold(600, event)
            return True
        if self.shortcut_matches(event, "player_seek_back_large"):
            self.start_player_seek_hold(-60, event)
            return True
        if self.shortcut_matches(event, "player_seek_forward_large"):
            self.start_player_seek_hold(60, event)
            return True
        if self.shortcut_matches(event, "player_seek_back"):
            self.start_player_seek_hold(-self.seek_seconds_value(), event)
            return True
        if self.shortcut_matches(event, "player_seek_forward"):
            self.start_player_seek_hold(self.seek_seconds_value(), event)
            return True
        if self.shortcut_matches(event, "player_volume_up"):
            self.change_volume_async(self.settings.volume_step)
            return True
        if self.shortcut_matches(event, "player_volume_down"):
            self.change_volume_async(-self.settings.volume_step)
            return True
        return False

    def handle_active_player_global_shortcut_event(self, event: wx.KeyEvent, focus: wx.Window | None) -> bool:
        if not (self.player_control_mode and self.player_is_active()):
            return False
        if self.player_shortcuts_allowed(focus) or self.focus_accepts_text(focus):
            return False
        if self.focus_in_results_control(focus):
            return False
        if self.shortcut_matches(event, "player_previous"):
            self.play_relative_item(-1, preserve_focus=True)
            return True
        if self.shortcut_matches(event, "player_next_related"):
            self.play_related_item()
            return True
        if self.shortcut_matches(event, "player_next"):
            self.play_relative_item(1, preserve_focus=True)
            return True
        return False

    @staticmethod
    def normalized_keyboard_shortcuts(shortcuts: dict | None) -> dict[str, str]:
        normalized = dict(DEFAULT_KEYBOARD_SHORTCUTS)
        if isinstance(shortcuts, dict):
            for action in DEFAULT_KEYBOARD_SHORTCUTS:
                value = str(shortcuts.get(action) or "").strip()
                if value:
                    normalized[action] = value
        return normalized

    def first_available_shortcut(self, values: dict[str, str], action: str, candidates: list[str]) -> str:
        used = {
            self.canonical_shortcut(shortcut)
            for other_action, shortcut in values.items()
            if other_action != action
        }
        for candidate in candidates:
            canonical = self.canonical_shortcut(candidate)
            if canonical and canonical not in used:
                return candidate
        return ""

    def repair_keyboard_shortcut_conflicts(self, shortcuts: dict[str, str]) -> dict[str, str]:
        repaired = dict(shortcuts)
        if str(repaired.get("player_marker_start", "")).strip() in {"[", "š", "Š"}:
            repaired["player_marker_start"] = DEFAULT_KEYBOARD_SHORTCUTS["player_marker_start"]
        if str(repaired.get("player_marker_end", "")).strip() in {"]", "đ", "Đ"}:
            repaired["player_marker_end"] = DEFAULT_KEYBOARD_SHORTCUTS["player_marker_end"]
        equalizer_shortcut = self.canonical_shortcut(repaired.get("player_equalizer", ""))
        if equalizer_shortcut in {"e", "g"}:
            repaired["player_equalizer"] = DEFAULT_KEYBOARD_SHORTCUTS["player_equalizer"]
        if not repaired.get("player_edit_mode"):
            repaired["player_edit_mode"] = DEFAULT_KEYBOARD_SHORTCUTS["player_edit_mode"]
        details_shortcut = self.canonical_shortcut(repaired.get("player_details", ""))
        volume_status_shortcut = self.canonical_shortcut(repaired.get("player_volume_status", ""))
        if details_shortcut in {"", "v"} or details_shortcut == volume_status_shortcut:
            repaired["player_details"] = DEFAULT_KEYBOARD_SHORTCUTS["player_details"]
        if not repaired.get("player_volume_status") or volume_status_shortcut in {"f7", self.canonical_shortcut(repaired.get("player_details", ""))}:
            repaired["player_volume_status"] = DEFAULT_KEYBOARD_SHORTCUTS["player_volume_status"]
        if self.canonical_shortcut(repaired.get("new_subscription_videos", "")) == self.canonical_shortcut(repaired.get("player_play_pause", "")):
            replacement = self.first_available_shortcut(repaired, "new_subscription_videos", ["Ctrl+Shift+V", "Ctrl+Alt+V", "Alt+N"])
            if replacement:
                repaired["new_subscription_videos"] = replacement
        if self.canonical_shortcut(repaired.get("add_to_playlist", "")) == self.canonical_shortcut("Ctrl+Shift+P"):
            repaired["add_to_playlist"] = DEFAULT_KEYBOARD_SHORTCUTS["add_to_playlist"]
        if self.canonical_shortcut(repaired.get("remove_from_playlist", "")) == self.canonical_shortcut("Ctrl+Shift+R"):
            repaired["remove_from_playlist"] = DEFAULT_KEYBOARD_SHORTCUTS["remove_from_playlist"]
        if not str(repaired.get("add_favorite", "")).strip():
            repaired["add_favorite"] = DEFAULT_KEYBOARD_SHORTCUTS["add_favorite"]
        if not str(repaired.get("remove_favorite", "")).strip():
            repaired["remove_favorite"] = DEFAULT_KEYBOARD_SHORTCUTS["remove_favorite"]
        if self.canonical_shortcut(repaired.get("player_preview_marked_clip", "")) == "f1":
            repaired["player_preview_marked_clip"] = DEFAULT_KEYBOARD_SHORTCUTS["player_preview_marked_clip"]
        for action in list(repaired):
            if self.canonical_shortcut(repaired.get(action, "")) == "f5":
                repaired[action] = DEFAULT_KEYBOARD_SHORTCUTS.get(action, "")
        seen: dict[str, str] = {}
        for action, _label_key in SHORTCUT_DEFINITIONS:
            canonical = self.canonical_shortcut(repaired.get(action, ""))
            if not canonical:
                repaired[action] = DEFAULT_KEYBOARD_SHORTCUTS.get(action, "")
                canonical = self.canonical_shortcut(repaired.get(action, ""))
            if canonical and canonical in seen:
                replacement = self.first_available_shortcut(repaired, action, [DEFAULT_KEYBOARD_SHORTCUTS.get(action, ""), f"Ctrl+Alt+{action[:1].upper()}"])
                if replacement:
                    repaired[action] = replacement
            if canonical:
                seen[self.canonical_shortcut(repaired.get(action, ""))] = action
        return repaired

