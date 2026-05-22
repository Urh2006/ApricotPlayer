from __future__ import annotations
import json
import os
import queue
import random
import re
import http.cookiejar
import sys
import threading
import time
import xml.etree.ElementTree as ET
import zipfile
import shutil
import tempfile
import urllib.request
import urllib.parse
from urllib.request import Request
from dataclasses import asdict, dataclass, field, fields
from datetime import datetime, timezone
from importlib import import_module
from pathlib import Path
from urllib.parse import parse_qs, parse_qsl, unquote, urlencode, urljoin, urlparse
import wx
import wx.adv
try:
    import winreg
except ImportError:
    pass
try:
    import ctypes
except ImportError:
    pass

from apricot.constants import *
from apricot.locales import TEXT

class DialogsMixin:

    def prompt_initial_language(self) -> None:
        if self.settings.language_prompted:
            return
        choices = [name for _code, name in LANGUAGES]
        current = LANGUAGE_CODES.index(self.settings.language) if self.settings.language in LANGUAGE_CODES else 0
        with wx.SingleChoiceDialog(self, self.t("first_run_language_prompt"), self.t("language"), choices) as dialog:
            dialog.SetSelection(current)
            if dialog.ShowModal() == wx.ID_OK:
                index = dialog.GetSelection()
                if 0 <= index < len(LANGUAGE_CODES):
                    self.settings.language = LANGUAGE_CODES[index]
        self.settings.language_prompted = True
        self.save_settings()
        self.show_main_menu()
        self.announce_player(self.t("settings_saved"))



    def prompt_cookie_refresh_for_playback(self, command: str, url: str, title: str, error: str, announce_start: bool = False, request_generation: int = 0) -> None:
        if not self.playback_request_is_current(request_generation):
            return
        message = f"{self.t('player_failed', error=error)}\n\n{self.t('cookie_refresh_prompt_message')}"
        answer = wx.MessageBox(message, self.t("cookie_refresh_prompt_title"), wx.YES_NO | wx.ICON_QUESTION)
        if answer != wx.YES:
            return
        browser = self.normalized_cookies_browser()
        if not browser:
            self.message(self.t("select_cookies_browser"), wx.ICON_WARNING)
            return
        self.announce_player(self.t("cookie_auto_refresh_start", browser=browser.title()))
        threading.Thread(target=self.refresh_cookies_and_retry_playback_worker, args=(browser, command, url, title, announce_start, request_generation), daemon=True).start()



    def prompt_for_new_default_audio_device(self, values: list[str], labels: list[str]) -> None:
        if not values:
            values, labels = ["auto"], ["auto"]
        self.message(self.t("audio_device_missing"), wx.ICON_WARNING)
        with wx.SingleChoiceDialog(self, self.t("audio_device_missing"), self.t("default_audio_device"), labels) as dialog:
            dialog.SetSelection(0)
            if dialog.ShowModal() != wx.ID_OK:
                self.settings.audio_output_device = "auto"
                self.save_settings()
                return
            index = dialog.GetSelection()
        self.settings.audio_output_device = values[index] if 0 <= index < len(values) else "auto"
        self.save_settings()
        self.announce_player(self.t("settings_saved"))



    def message(self, text: str, style=wx.ICON_INFORMATION) -> None:
        wx.MessageBox(text, APP_NAME, wx.OK | style)


