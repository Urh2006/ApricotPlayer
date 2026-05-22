from apricot.constants import *
import wx
import os
from pathlib import Path

class EqualizerUI:
    def set_equalizer_slider_accessibility(self, ctrl: wx.Slider, label: str) -> None:
        value_text = f"{float(ctrl.GetValue()) / 10.0:.1f} dB"
        name = str(label).strip() or self.t("equalizer")
        full_text = f"{name}: {value_text}"
        previous_value = getattr(ctrl, "_apricot_accessible_value", None)
        if ctrl.GetName() != name:
            ctrl.SetName(name)
        if ctrl.GetLabel() != name:
            ctrl.SetLabel(name)
        ctrl.SetToolTip(full_text)
        ctrl._apricot_accessible_name = name
        ctrl._apricot_accessible_description = full_text
        ctrl._apricot_accessible_value = value_text
        if not getattr(ctrl, "_apricot_accessible", None):
            try:
                ctrl._apricot_accessible = SliderAccessible(ctrl)
                ctrl.SetAccessible(ctrl._apricot_accessible)
            except Exception:
                pass
        if (
            previous_value != value_text
            and getattr(ctrl, "_apricot_accessible_initialized", False)
            and not getattr(ctrl, "_apricot_suppress_accessible_notify", False)
        ):
            try:
                wx.Accessible.NotifyEvent(wx.ACC_EVENT_OBJECT_VALUECHANGE, ctrl, wx.OBJID_CLIENT, 0)
            except Exception:
                pass
        ctrl._apricot_accessible_initialized = True

    def effective_equalizer_state(self) -> tuple[bool, dict[str, float]]:
        enabled, gains = self.base_equalizer_state()
        if self.bass_boost_enabled:
            return True, self.equalizer_gains_with_bass_boost(gains if enabled else default_equalizer_gains())
        return enabled, gains

    def base_equalizer_state(self) -> tuple[bool, dict[str, float]]:
        if self.session_equalizer_enabled is not None:
            return bool(self.session_equalizer_enabled), self.normalized_equalizer_gains(self.session_equalizer_gains)
        preset = self.normalized_equalizer_preset(getattr(self.settings, "global_equalizer_preset", EQ_PRESET_FLAT))
        return bool(getattr(self.settings, "global_equalizer_enabled", False)), self.equalizer_gains_for_preset(preset)

    def equalizer_gains_with_bass_boost(self, gains: dict[str, float]) -> dict[str, float]:
        combined = self.normalized_equalizer_gains(gains)
        boost = self.factory_equalizer_gains_for_preset("bass_boost")
        for band_id, _band_label in EQ_BANDS:
            combined[band_id] = round(max(-24.0, min(24.0, combined.get(band_id, 0.0) + boost.get(band_id, 0.0))), 1)
        return combined

    def use_global_equalizer_for_live_preview(self) -> None:
        self.session_equalizer_enabled = None
        self.session_equalizer_gains = {}
        self.session_equalizer_before_bass_boost = None

    def use_visible_equalizer_for_live_preview(self) -> None:
        self.session_equalizer_enabled = True
        self.session_equalizer_gains = self.visible_equalizer_gains()
        self.session_equalizer_before_bass_boost = None

    @staticmethod
    def equalizer_band_filter(band_id: str, gain: float) -> str:
        width = EQ_FILTER_Q_WIDTHS.get(str(band_id), EQ_FILTER_Q_WIDTH)
        return f"equalizer=f={band_id}:t=q:w={width:g}:g={gain:.1f}"

    @staticmethod
    def equalizer_has_positive_gain(gains: dict[str, float]) -> bool:
        for band_id, _band_label in EQ_BANDS:
            try:
                if float(gains.get(band_id, 0.0) or 0.0) > 0.05:
                    return True
            except (TypeError, ValueError):
                continue
        return False

    @classmethod
    def equalizer_filter(cls, gains: dict[str, float], protect_clipping: bool = False, label: str = EQ_FILTER_LABEL) -> str:
        filters = []
        if protect_clipping:
            headroom = __import__("wx_main").MainFrame.equalizer_clipping_headroom_db(gains)
            if headroom <= -0.05:
                filters.append(f"volume={headroom:.1f}dB")
        for band_id, _band_label in EQ_BANDS:
            gain = max(-24.0, min(24.0, float(gains.get(band_id, 0.0) or 0.0)))
            if abs(gain) >= 0.05:
                filters.append(__import__("wx_main").MainFrame.equalizer_band_filter(band_id, gain))
        if protect_clipping and filters:
            filters.append(EQ_LIMITER_FILTER)
        return f"@{label}:lavfi=[{','.join(filters)}]"

    @staticmethod
    def equalizer_clipping_headroom_db(gains: dict[str, float]) -> float:
        max_positive = 0.0
        for band_id, _band_label in EQ_BANDS:
            try:
                max_positive = max(max_positive, float(gains.get(band_id, 0.0) or 0.0))
            except (TypeError, ValueError):
                continue
        return -min(EQ_CLIPPING_HEADROOM_LIMIT_DB, max(0.0, max_positive))

    def equalizer_clipping_protection_active(self, gains: dict[str, float]) -> bool:
        return (
            bool(getattr(self.settings, "equalizer_clipping_protection", False))
            and self.equalizer_has_positive_gain(gains)
        )

    def schedule_equalizer_apply(self, delay_ms: int = EQ_APPLY_DELAY_MS) -> None:
        self.equalizer_apply_generation += 1
        generation = self.equalizer_apply_generation
        timer = getattr(self, "equalizer_apply_timer", None)
        if timer is not None and timer.IsRunning():
            timer.Stop()
        self.equalizer_apply_timer = wx.CallLater(max(0, int(delay_ms)), self.apply_scheduled_equalizer_to_player, generation)

    def apply_scheduled_equalizer_to_player(self, generation: int) -> None:
        if generation != getattr(self, "equalizer_apply_generation", 0):
            return
        self.apply_equalizer_to_player()

    def apply_equalizer_to_player(self, retries: int = 2) -> None:
        if self.player_kind != "mpv" or not self.mpv_process_alive():
            return
        enabled, gains = self.effective_equalizer_state()
        if not enabled or not any(abs(float(value)) >= 0.05 for value in gains.values()):
            self.clear_equalizer_filters()
            return
        current_ref = getattr(self, "equalizer_filter_ref", EQ_FILTER_REF) if self.equalizer_filter_active else ""
        next_ref = EQ_FILTER_ALT_REF if current_ref == EQ_FILTER_REF else EQ_FILTER_REF
        next_label = EQ_FILTER_ALT_LABEL if next_ref == EQ_FILTER_ALT_REF else EQ_FILTER_LABEL
        try:
            self.mpv_request(["af", "remove", next_ref], timeout=0.8)
            response = self.mpv_request(
                ["af", "add", self.equalizer_filter(gains, self.equalizer_clipping_protection_active(gains), next_label)],
                timeout=1.2,
            )
            if response.get("error") == "success":
                if current_ref and current_ref != next_ref:
                    self.mpv_request(["af", "remove", current_ref], timeout=0.8)
                stale_ref = EQ_FILTER_ALT_REF if next_ref == EQ_FILTER_REF else EQ_FILTER_REF
                if stale_ref != current_ref:
                    self.mpv_request(["af", "remove", stale_ref], timeout=0.8)
                self.equalizer_filter_ref = next_ref
                self.equalizer_filter_active = True
                return
        except Exception:
            pass
        if not current_ref:
            self.equalizer_filter_active = False
        if retries > 0:
            wx.CallLater(180, self.apply_equalizer_to_player, retries - 1)

    def clear_equalizer_filters(self) -> None:
        for filter_ref in (EQ_FILTER_REF, EQ_FILTER_ALT_REF):
            try:
                self.mpv_request(["af", "remove", filter_ref], timeout=0.8)
            except Exception:
                pass
        self.equalizer_filter_ref = EQ_FILTER_REF
        self.equalizer_filter_active = False

    def show_player_equalizer(self) -> None:
        if not self.player_is_active():
            return
        original_enabled = self.session_equalizer_enabled
        original_gains = dict(self.session_equalizer_gains)
        original_db_range = self.equalizer_db_range_value()
        _enabled, gains = self.base_equalizer_state()
        active_preset = self.normalized_equalizer_preset(getattr(self.settings, "global_equalizer_preset", EQ_PRESET_FLAT))
        dialog_db_range = original_db_range
        dialog = wx.Dialog(self, title=self.t("equalizer"), style=wx.DEFAULT_DIALOG_STYLE | wx.RESIZE_BORDER)
        dialog.SetName(self.t("equalizer"))
        dialog.SetMinSize((520, 520))
        preset_options = self.equalizer_preset_options()
        dialog_visible_preset = active_preset
        outer = wx.BoxSizer(wx.VERTICAL)
        outer.Add(wx.StaticText(dialog, label=self.t("equalizer_preset")), 0, wx.LEFT | wx.RIGHT | wx.TOP, 8)
        preset_choice = wx.Choice(dialog, choices=self.equalizer_preset_labels())
        preset_choice.SetName(self.t("equalizer_preset"))
        preset_choice.SetSelection(preset_options.index(active_preset) if active_preset in preset_options else 0)
        outer.Add(preset_choice, 0, wx.LEFT | wx.RIGHT | wx.BOTTOM | wx.EXPAND, 8)
        outer.Add(wx.StaticText(dialog, label=self.t("equalizer_db_range")), 0, wx.LEFT | wx.RIGHT | wx.TOP, 8)
        range_choice = wx.Choice(dialog, choices=EQ_RANGE_OPTIONS)
        range_choice.SetName(self.t("equalizer_db_range"))
        range_choice.SetSelection(EQ_RANGE_OPTIONS.index(str(dialog_db_range)) if str(dialog_db_range) in EQ_RANGE_OPTIONS else 1)
        outer.Add(range_choice, 0, wx.LEFT | wx.RIGHT | wx.BOTTOM | wx.EXPAND, 8)
        name_label = wx.StaticText(dialog, label=self.t("equalizer_preset_name"))
        name_ctrl = wx.TextCtrl(dialog, value=self.equalizer_custom_name(active_preset))
        name_ctrl.SetName(self.t("equalizer_preset_name"))
        outer.Add(name_label, 0, wx.LEFT | wx.RIGHT | wx.TOP, 8)
        outer.Add(name_ctrl, 0, wx.LEFT | wx.RIGHT | wx.BOTTOM | wx.EXPAND, 8)
        sliders: dict[str, wx.Slider] = {}
        dialog_gains = self.normalized_equalizer_gains(gains)
        for band_id, band_label in EQ_BANDS:
            label_text = self.t("equalizer_band_gain", band=band_label)
            outer.Add(wx.StaticText(dialog, label=label_text), 0, wx.LEFT | wx.RIGHT | wx.TOP, 8)
            band_value = min(max(int(round(dialog_gains.get(band_id, 0.0) * 10)), -dialog_db_range * 10), dialog_db_range * 10)
            slider = wx.Slider(
                dialog,
                value=band_value,
                minValue=-dialog_db_range * 10,
                maxValue=dialog_db_range * 10,
                style=wx.SL_HORIZONTAL,
            )
            slider._apricot_eq_band_id = str(band_id)
            self.configure_equalizer_slider_steps(slider)
            self.set_equalizer_slider_accessibility(slider, label_text)
            sliders[band_id] = slider
            outer.Add(slider, 0, wx.LEFT | wx.RIGHT | wx.BOTTOM | wx.EXPAND, 8)

        buttons = wx.StdDialogButtonSizer()
        ok_button = wx.Button(dialog, wx.ID_OK)
        cancel_button = wx.Button(dialog, wx.ID_CANCEL)
        reset_button = wx.Button(dialog, label=self.t("reset_equalizer"))
        save_global_button = wx.Button(dialog, label=self.t("save_equalizer_as_global"))
        add_profile_button = wx.Button(dialog, label=self.t("add_equalizer_profile"))
        delete_profile_button = wx.Button(dialog, label=self.t("delete_equalizer_profile"))
        buttons.AddButton(ok_button)
        buttons.AddButton(cancel_button)
        buttons.Realize()
        row = wx.BoxSizer(wx.HORIZONTAL)
        row.Add(add_profile_button, 0, wx.RIGHT, 8)
        row.Add(delete_profile_button, 0, wx.RIGHT, 8)
        row.Add(save_global_button, 0, wx.RIGHT, 8)
        row.Add(reset_button, 0, wx.RIGHT, 8)
        row.Add(buttons, 0)
        outer.Add(row, 0, wx.ALIGN_RIGHT | wx.ALL, 8)
        dialog.SetSizer(outer)

        def current_dialog_gains() -> dict[str, float]:
            return self.normalized_equalizer_gains(dialog_gains)

        def current_preset() -> str:
            index = preset_choice.GetSelection()
            return preset_options[index] if 0 <= index < len(preset_options) else EQ_PRESET_FLAT

        def set_dialog_slider_value(slider: wx.Slider, value: int, label: str, *, notify: bool = False) -> None:
            slider._apricot_suppress_accessible_notify = not notify
            slider._apricot_eq_programmatic_update = True
            try:
                slider.SetValue(value)
                band_id = self.equalizer_slider_band_id(slider)
                if band_id:
                    dialog_gains[band_id] = self.equalizer_gain_from_slider_value(value)
                self.set_equalizer_slider_accessibility(slider, label)
            finally:
                slider._apricot_eq_programmatic_update = False
                slider._apricot_suppress_accessible_notify = False

        def update_custom_name_visibility() -> None:
            visible = self.is_custom_equalizer_preset(current_preset())
            name_label.Show(visible)
            name_ctrl.Show(visible)
            delete_profile_button.Enable(visible)
            dialog.Layout()

        def save_current_dialog_name(preset_id: str | None = None) -> None:
            preset_id = self.normalized_equalizer_preset(preset_id or dialog_visible_preset)
            if not self.is_custom_equalizer_preset(preset_id):
                return
            names = self.normalized_equalizer_custom_names(getattr(self.settings, "equalizer_custom_names", {}) or {})
            names[preset_id] = name_ctrl.GetValue().strip()[:80] or self.equalizer_custom_name(preset_id)
            self.settings.equalizer_custom_names = names

        def live_apply() -> None:
            self.session_equalizer_enabled = True
            self.session_equalizer_gains = current_dialog_gains()
            self.schedule_equalizer_apply()

        def load_preset_into_sliders(preset_id: str) -> None:
            nonlocal dialog_visible_preset
            nonlocal dialog_gains
            dialog_visible_preset = self.normalized_equalizer_preset(preset_id)
            preset_gains = self.equalizer_gains_for_preset(preset_id)
            dialog_gains = self.normalized_equalizer_gains(preset_gains)
            for band_id, band_label in EQ_BANDS:
                value = min(max(preset_gains.get(band_id, 0.0), -dialog_db_range), dialog_db_range)
                set_dialog_slider_value(sliders[band_id], int(round(value * 10)), self.t("equalizer_band_gain", band=band_label))
            name_ctrl.SetValue(self.equalizer_custom_name(preset_id))
            update_custom_name_visibility()
            live_apply()

        def apply_dialog_db_range(value: int) -> None:
            nonlocal dialog_db_range
            dialog_db_range = min(24, max(6, int(value or 12)))
            slider_min = -dialog_db_range * 10
            slider_max = dialog_db_range * 10
            for band_id, band_label in EQ_BANDS:
                slider = sliders[band_id]
                current = min(max(int(round(dialog_gains.get(band_id, 0.0) * 10)), slider_min), slider_max)
                slider._apricot_eq_programmatic_update = True
                try:
                    slider.SetRange(slider_min, slider_max)
                finally:
                    slider._apricot_eq_programmatic_update = False
                self.configure_equalizer_slider_steps(slider)
                set_dialog_slider_value(slider, current, self.t("equalizer_band_gain", band=band_label))
            live_apply()

        def refresh_preset_choices(selected_preset: str) -> None:
            nonlocal preset_options
            nonlocal dialog_visible_preset
            dialog_visible_preset = self.normalized_equalizer_preset(selected_preset)
            preset_options = self.equalizer_preset_options()
            preset_choice.SetItems(self.equalizer_preset_labels())
            preset_choice.SetSelection(preset_options.index(selected_preset) if selected_preset in preset_options else 0)
            update_custom_name_visibility()

        def on_preset_changed(_event: wx.CommandEvent) -> None:
            save_current_dialog_name(dialog_visible_preset)
            load_preset_into_sliders(current_preset())

        def on_slider(event: wx.CommandEvent, label: str) -> None:
            ctrl = event.GetEventObject()
            if isinstance(ctrl, wx.Slider):
                if getattr(ctrl, "_apricot_eq_programmatic_update", False):
                    event.Skip()
                    return
                band_id = self.equalizer_slider_band_id(ctrl)
                if not band_id:
                    return
                dialog_gains[band_id] = self.equalizer_gain_from_slider_value(ctrl.GetValue())
                self.set_equalizer_slider_accessibility(ctrl, label)
            live_apply()

        def on_range_changed(_event: wx.CommandEvent) -> None:
            index = range_choice.GetSelection()
            value = EQ_RANGE_OPTIONS[index] if 0 <= index < len(EQ_RANGE_OPTIONS) else str(original_db_range)
            apply_dialog_db_range(self.to_int(value, original_db_range, 6, 24))

        for band_id, band_label in EQ_BANDS:
            self.bind_equalizer_slider_events(sliders[band_id], lambda evt, label=self.t("equalizer_band_gain", band=band_label): on_slider(evt, label))
        preset_choice.Bind(wx.EVT_CHOICE, on_preset_changed)
        range_choice.Bind(wx.EVT_CHOICE, on_range_changed)

        def reset_dialog_equalizer(_event=None) -> None:
            nonlocal dialog_gains
            preset_gains = self.factory_equalizer_gains_for_preset(current_preset())
            dialog_gains = self.normalized_equalizer_gains(preset_gains)
            for band_id, band_label in EQ_BANDS:
                value = min(max(preset_gains.get(band_id, 0.0), -dialog_db_range), dialog_db_range)
                set_dialog_slider_value(sliders[band_id], int(round(value * 10)), self.t("equalizer_band_gain", band=band_label))
            live_apply()

        reset_button.Bind(wx.EVT_BUTTON, reset_dialog_equalizer)

        def add_profile_from_dialog(_event=None) -> None:
            preset_id = self.create_equalizer_profile_dialog(current_dialog_gains())
            if not preset_id:
                return
            refresh_preset_choices(preset_id)
            name_ctrl.SetValue(self.equalizer_custom_name(preset_id))
            live_apply()

        def save_dialog_as_global(_event=None) -> None:
            save_current_dialog_name()
            preset_id = self.choose_equalizer_profile_for_save(current_dialog_gains())
            if not preset_id:
                return
            self.settings.global_equalizer_enabled = True
            self.settings.global_equalizer_preset = preset_id
            self.save_settings()
            refresh_preset_choices(preset_id)
            self.announce_player(self.t("equalizer_profile_saved"))

        add_profile_button.Bind(wx.EVT_BUTTON, add_profile_from_dialog)
        save_global_button.Bind(wx.EVT_BUTTON, save_dialog_as_global)

        def delete_profile_from_dialog(_event=None) -> None:
            preset = current_preset()
            replacement = self.delete_equalizer_profile(preset)
            if not replacement:
                return
            refresh_preset_choices(replacement)
            load_preset_into_sliders(replacement)

        delete_profile_button.Bind(wx.EVT_BUTTON, delete_profile_from_dialog)
        update_custom_name_visibility()
        result = dialog.ShowModal()
        if result == wx.ID_OK:
            save_current_dialog_name()
            self.settings.equalizer_db_range = dialog_db_range
            self.save_settings()
        dialog.Destroy()
        if result == wx.ID_OK:
            self.announce_player(self.t("equalizer_saved"))
            return
        self.session_equalizer_enabled = original_enabled
        self.session_equalizer_gains = original_gains
        self.settings.equalizer_db_range = original_db_range
        self.apply_equalizer_to_player()
        self.announce_player(self.t("equalizer_closed"))

    def ffmpeg_equalizer_filters(self, gains: dict[str, float]) -> list[str]:
        filters: list[str] = []
        protect_clipping = self.equalizer_clipping_protection_active(gains)
        if protect_clipping:
            headroom = self.equalizer_clipping_headroom_db(gains)
            if headroom <= -0.05:
                filters.append(f"volume={headroom:.1f}dB")
        for band_id, _band_label in EQ_BANDS:
            gain = max(-24.0, min(24.0, float(gains.get(band_id, 0.0) or 0.0)))
            if abs(gain) >= 0.05:
                filters.append(self.equalizer_band_filter(band_id, gain))
        if protect_clipping and filters:
            filters.append(EQ_LIMITER_FILTER)
        return filters

    def equalizer_db_range_value(self) -> int:
        try:
            value = int(getattr(self.settings, "equalizer_db_range", 12) or 12)
        except (TypeError, ValueError):
            value = 12
        return min(24, max(6, value))

    @staticmethod
    def configure_equalizer_slider_steps(ctrl: wx.Slider) -> None:
        for setter_name, value in (("SetLineSize", 10), ("SetPageSize", 30)):
            setter = getattr(ctrl, setter_name, None)
            if setter:
                try:
                    setter(value)
                except Exception:
                    pass

    @staticmethod
    def bind_equalizer_slider_events(ctrl: wx.Slider, handler) -> None:
        event_names = ("EVT_SLIDER",)
        seen: set[int] = set()
        for event_name in event_names:
            binder = getattr(wx, event_name, None)
            if binder is None or id(binder) in seen:
                continue
            seen.add(id(binder))
            try:
                ctrl.Bind(binder, handler)
            except Exception:
                pass

    def normalized_equalizer_preset(self, preset: str | None) -> str:
        value = str(preset or EQ_PRESET_FLAT).strip()
        if value in EQ_FACTORY_PRESET_VALUES or value in EQ_CUSTOM_PRESET_IDS or value.startswith(("custom_", "user_")):
            return value
        return value if value in self.equalizer_preset_options() else EQ_PRESET_FLAT

    @staticmethod
    def normalized_equalizer_gains(gains: dict | None) -> dict[str, float]:
        normalized = default_equalizer_gains()
        if isinstance(gains, dict):
            for band_id, _band_label in EQ_BANDS:
                try:
                    normalized[band_id] = round(max(-24.0, min(24.0, float(gains.get(band_id, 0.0) or 0.0))), 1)
                except (TypeError, ValueError):
                    normalized[band_id] = 0.0
        return normalized

    def normalized_equalizer_preset_gains(self, presets: dict | None) -> dict[str, dict[str, float]]:
        normalized = default_equalizer_preset_gains()
        if isinstance(presets, dict):
            for preset_id in EQ_CUSTOM_PRESET_IDS:
                gains = presets.get(preset_id)
                if isinstance(gains, dict):
                    normalized[preset_id] = self.normalized_equalizer_gains(gains)
            for preset_id, gains in presets.items():
                preset_text = str(preset_id or "").strip()
                if not preset_text or preset_text in normalized or preset_text in EQ_FACTORY_PRESET_VALUES:
                    continue
                if isinstance(gains, dict):
                    normalized[preset_text] = self.normalized_equalizer_gains(gains)
        return normalized

    def normalized_equalizer_custom_names(self, names: dict | None) -> dict[str, str]:
        normalized = default_equalizer_custom_names()
        if isinstance(names, dict):
            for custom_id, value in names.items():
                custom_text = str(custom_id or "").strip()
                if not custom_text or custom_text in EQ_FACTORY_PRESET_VALUES:
                    continue
                name = str(value or "").strip()
                if name:
                    normalized[custom_text] = name[:80]
        return normalized

    def equalizer_custom_ids(self) -> list[str]:
        settings = getattr(self, "settings", None)
        names = self.normalized_equalizer_custom_names(getattr(settings, "equalizer_custom_names", {}) or {})
        presets = self.normalized_equalizer_preset_gains(getattr(settings, "equalizer_preset_gains", {}) or {})
        custom_ids = set(EQ_CUSTOM_PRESET_IDS)
        custom_ids.update(key for key in names if key not in EQ_FACTORY_PRESET_VALUES)
        custom_ids.update(key for key in presets if key not in EQ_FACTORY_PRESET_VALUES)
        return sorted(custom_ids, key=lambda key: (0, EQ_CUSTOM_PRESET_IDS.index(key)) if key in EQ_CUSTOM_PRESET_IDS else (1, key.lower()))

    def equalizer_preset_options(self) -> list[str]:
        return list(EQ_FACTORY_PRESET_VALUES.keys()) + self.equalizer_custom_ids()

    def is_custom_equalizer_preset(self, preset_id: str) -> bool:
        return preset_id not in EQ_FACTORY_PRESET_VALUES

    def equalizer_custom_name(self, preset_id: str) -> str:
        names = self.normalized_equalizer_custom_names(getattr(self.settings, "equalizer_custom_names", {}) or {})
        return names.get(preset_id, default_equalizer_custom_names().get(preset_id, preset_id))

    def equalizer_preset_label(self, preset_id: str) -> str:
        if self.is_custom_equalizer_preset(preset_id):
            return self.equalizer_custom_name(preset_id)
        return self.t(f"eq_preset_{preset_id}")

    def equalizer_preset_labels(self) -> list[str]:
        return [self.equalizer_preset_label(preset_id) for preset_id in self.equalizer_preset_options()]

    def equalizer_gains_for_preset(self, preset_id: str | None) -> dict[str, float]:
        preset_id = self.normalized_equalizer_preset(preset_id)
        if preset_id in EQ_FACTORY_PRESET_VALUES:
            return self.factory_equalizer_gains_for_preset(preset_id)
        presets = self.normalized_equalizer_preset_gains(getattr(self.settings, "equalizer_preset_gains", {}) or {})
        return self.normalized_equalizer_gains(presets.get(preset_id) or {})

    def factory_equalizer_gains_for_preset(self, preset_id: str | None) -> dict[str, float]:
        preset_id = self.normalized_equalizer_preset(preset_id)
        if preset_id in EQ_FACTORY_PRESET_VALUES:
            return equalizer_gains_from_values(EQ_FACTORY_PRESET_VALUES[preset_id])
        return default_equalizer_gains()

    def equalizer_slider_band_id(self, ctrl: wx.Window | None) -> str:
        band_id = str(getattr(ctrl, "_apricot_eq_band_id", "") or "")
        return band_id if band_id in EQ_BAND_IDS else ""

    @staticmethod
    def equalizer_gain_from_slider_value(value: int | float) -> float:
        return round(max(-24.0, min(24.0, float(value) / 10.0)), 1)

    def visible_equalizer_base_gains(self) -> dict[str, float]:
        preset = self.normalized_equalizer_preset(
            getattr(self, "visible_equalizer_preset", getattr(self.settings, "global_equalizer_preset", EQ_PRESET_FLAT))
        )
        return self.equalizer_gains_for_preset(preset)

    def update_visible_equalizer_draft_from_slider(self, ctrl: wx.Slider) -> str:
        band_id = self.equalizer_slider_band_id(ctrl)
        if not band_id:
            return ""
        draft = getattr(self, "visible_equalizer_draft_gains", None)
        if not isinstance(draft, dict) or not all(band in draft for band in EQ_BAND_IDS):
            draft = self.visible_equalizer_base_gains()
        draft = self.normalized_equalizer_gains(draft)
        draft[band_id] = self.equalizer_gain_from_slider_value(ctrl.GetValue())
        self.visible_equalizer_draft_gains = draft
        return band_id

    def visible_equalizer_gains_from_controls(self) -> dict[str, float]:
        gains: dict[str, float] = {}
        if not hasattr(self, "controls"):
            return gains
        for band_id, _band_label in EQ_BANDS:
            ctrl = self.controls.get(f"eq_{band_id}")
            if isinstance(ctrl, wx.Slider):
                gains[band_id] = round(float(ctrl.GetValue()) / 10.0, 1)
        return gains

    def visible_equalizer_gains(self) -> dict[str, float]:
        draft = getattr(self, "visible_equalizer_draft_gains", None)
        if isinstance(draft, dict) and all(band_id in draft for band_id in EQ_BAND_IDS):
            return self.normalized_equalizer_gains(draft)
        return self.visible_equalizer_gains_from_controls()

    def save_visible_equalizer_gains_to_preset(self, preset_id: str | None = None) -> None:
        preset_id = self.normalized_equalizer_preset(preset_id or getattr(self, "visible_equalizer_preset", EQ_PRESET_FLAT))
        gains = self.visible_equalizer_gains()
        if not gains:
            return
        normalized_gains = self.normalized_equalizer_gains(gains)
        if self.is_custom_equalizer_preset(preset_id):
            presets = self.normalized_equalizer_preset_gains(getattr(self.settings, "equalizer_preset_gains", {}) or {})
            presets[preset_id] = normalized_gains
            self.settings.equalizer_preset_gains = presets
        self.settings.global_equalizer_gains = normalized_gains

    def equalizer_default_profile_name(self) -> str:
        return f"Custom {len(self.equalizer_custom_ids()) + 1}"

    def next_equalizer_profile_id(self) -> str:
        existing = set(self.equalizer_preset_options())
        counter = 1
        while True:
            candidate = f"custom_{counter}"
            if candidate not in existing:
                return candidate
            counter += 1

    def create_equalizer_profile(self, name: str, gains: dict[str, float] | None = None) -> str:
        preset_id = self.next_equalizer_profile_id()
        names = self.normalized_equalizer_custom_names(getattr(self.settings, "equalizer_custom_names", {}) or {})
        names[preset_id] = (name.strip()[:80] if name.strip() else self.equalizer_default_profile_name())
        presets = self.normalized_equalizer_preset_gains(getattr(self.settings, "equalizer_preset_gains", {}) or {})
        presets[preset_id] = self.normalized_equalizer_gains(gains or default_equalizer_gains())
        self.settings.equalizer_custom_names = names
        self.settings.equalizer_preset_gains = presets
        return preset_id

    def create_equalizer_profile_dialog(self, gains: dict[str, float] | None = None) -> str:
        with wx.TextEntryDialog(self, self.t("equalizer_profile_name"), self.t("add_equalizer_profile"), "") as dialog:
            if dialog.ShowModal() != wx.ID_OK:
                return ""
            name = dialog.GetValue().strip()
        preset_id = self.create_equalizer_profile(name, gains)
        self.settings.global_equalizer_preset = preset_id
        self.save_settings()
        self.announce_player(self.t("equalizer_profile_saved"))
        return preset_id

    def delete_equalizer_profile(self, preset_id: str | None, confirm: bool = True) -> str:
        preset_id = self.normalized_equalizer_preset(preset_id)
        if not self.is_custom_equalizer_preset(preset_id):
            return ""
        if confirm:
            answer = wx.MessageBox(self.t("equalizer_profile_delete_confirm"), self.t("equalizer"), wx.YES_NO | wx.ICON_QUESTION)
            if answer != wx.YES:
                return ""
        names = self.normalized_equalizer_custom_names(getattr(self.settings, "equalizer_custom_names", {}) or {})
        presets = self.normalized_equalizer_preset_gains(getattr(self.settings, "equalizer_preset_gains", {}) or {})
        names.pop(preset_id, None)
        if preset_id in EQ_CUSTOM_PRESET_IDS:
            presets[preset_id] = default_equalizer_gains()
        else:
            presets.pop(preset_id, None)
        self.settings.equalizer_custom_names = names
        self.settings.equalizer_preset_gains = presets
        replacement = EQ_PRESET_FLAT
        self.settings.global_equalizer_preset = replacement
        self.settings.global_equalizer_gains = self.equalizer_gains_for_preset(replacement)
        self.save_settings()
        self.announce_player(self.t("equalizer_profile_deleted"))
        return replacement

    def choose_equalizer_profile_for_save(self, gains: dict[str, float]) -> str:
        profile_ids = self.equalizer_custom_ids()
        labels = [self.equalizer_custom_name(profile_id) for profile_id in profile_ids]
        labels.append(self.t("add_equalizer_profile"))
        with wx.SingleChoiceDialog(self, self.t("save_equalizer_as_global"), self.t("equalizer"), labels) as dialog:
            if dialog.ShowModal() != wx.ID_OK:
                return ""
            selection = dialog.GetSelection()
        if selection == len(profile_ids):
            return self.create_equalizer_profile_dialog(gains)
        if selection < 0 or selection >= len(profile_ids):
            return ""
        preset_id = profile_ids[selection]
        presets = self.normalized_equalizer_preset_gains(getattr(self.settings, "equalizer_preset_gains", {}) or {})
        presets[preset_id] = self.normalized_equalizer_gains(gains)
        self.settings.equalizer_preset_gains = presets
        self.save_settings()
        return preset_id

    def on_global_equalizer_toggle(self, _event: wx.CommandEvent) -> None:
        ctrl = self.controls.get("global_equalizer") if hasattr(self, "controls") else None
        self.save_visible_equalizer_gains_to_preset(getattr(self, "visible_equalizer_preset", EQ_PRESET_FLAT))
        if isinstance(ctrl, wx.CheckBox):
            self.settings.global_equalizer_enabled = ctrl.GetValue()
        if self.player_is_active():
            self.use_global_equalizer_for_live_preview()
            self.schedule_equalizer_apply(30)
        wx.CallAfter(self.render_settings_section_and_focus, "global_equalizer")

    def on_equalizer_clipping_protection_changed(self, _event: wx.CommandEvent) -> None:
        ctrl = self.controls.get("equalizer_clipping_protection") if hasattr(self, "controls") else None
        if isinstance(ctrl, wx.CheckBox):
            self.settings.equalizer_clipping_protection = bool(ctrl.GetValue())
        if self.player_is_active():
            self.schedule_equalizer_apply(30)

    def on_equalizer_range_changed(self, _event: wx.CommandEvent) -> None:
        self.save_visible_equalizer_gains_to_preset(getattr(self, "visible_equalizer_preset", EQ_PRESET_FLAT))
        next_range = self.to_int(self.selected_choice_value("equalizer_db_range"), 12, 6, 24)
        self.settings.equalizer_db_range = next_range
        draft = self.visible_equalizer_gains()
        self.visible_equalizer_draft_gains = {
            band_id: round(max(-float(next_range), min(float(next_range), float(draft.get(band_id, 0.0) or 0.0))), 1)
            for band_id, _band_label in EQ_BANDS
        }
        self.save_visible_equalizer_gains_to_preset(getattr(self, "visible_equalizer_preset", EQ_PRESET_FLAT))
        wx.CallAfter(self.render_settings_section_and_focus, "equalizer_db_range")

    def reset_visible_equalizer_controls(self) -> None:
        if not hasattr(self, "controls"):
            return
        preset = self.normalized_equalizer_preset(self.selected_choice_value("equalizer_preset") or getattr(self, "visible_equalizer_preset", EQ_PRESET_FLAT))
        gains = self.factory_equalizer_gains_for_preset(preset)
        self.visible_equalizer_draft_gains = self.normalized_equalizer_gains(gains)
        if self.is_custom_equalizer_preset(preset):
            presets = self.normalized_equalizer_preset_gains(getattr(self.settings, "equalizer_preset_gains", {}) or {})
            presets[preset] = gains
            self.settings.equalizer_preset_gains = presets
        self.settings.global_equalizer_gains = self.normalized_equalizer_gains(gains)
        for band_id, band_label in EQ_BANDS:
            ctrl = self.controls.get(f"eq_{band_id}")
            if isinstance(ctrl, wx.Slider):
                value = gains.get(band_id, 0.0)
                ctrl._apricot_eq_programmatic_update = True
                try:
                    ctrl.SetValue(int(round(value * 10)))
                    self.set_equalizer_slider_accessibility(ctrl, self.t("equalizer_band_gain", band=band_label))
                finally:
                    ctrl._apricot_eq_programmatic_update = False
        if self.player_is_active():
            if self.is_custom_equalizer_preset(preset):
                self.use_global_equalizer_for_live_preview()
            else:
                self.use_visible_equalizer_for_live_preview()
            self.schedule_equalizer_apply(30)
        self.announce_player(self.t("equalizer_saved"))

