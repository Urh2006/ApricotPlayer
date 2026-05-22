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

class CookiesMixin:
    def choose_cookies_file(self) -> None:
        with wx.FileDialog(
            self,
            self.t("choose_cookies_file"),
            wildcard="cookies.txt (*.txt)|*.txt|All files (*.*)|*.*",
            style=wx.FD_OPEN | wx.FD_FILE_MUST_EXIST,
        ) as dialog:
            if dialog.ShowModal() != wx.ID_OK:
                return
            path = dialog.GetPath()
        try:
            result = self.import_cookie_file_to_cache(path)
        except Exception as exc:
            self.message(self.t("cookies_file_load_failed", error=self.friendly_error(exc)), wx.ICON_WARNING)
            return
        imported_path = str(result["path"])
        self.settings.cookies_file = imported_path
        self.settings.cookies_from_browser = "none"
        self.settings.cookies_browser_profile = COOKIE_PROFILE_AUTO
        self.cookie_repair_suppressed_until = 0.0
        self.save_settings()
        if hasattr(self, "controls"):
            if "cookies" in self.controls:
                self.controls["cookies"].SetValue(imported_path)
            if "cookies_from_browser" in self.controls:
                self.controls["cookies_from_browser"].SetSelection(0)
            if "cookies_browser_profile" in self.controls:
                self.controls["cookies_browser_profile"].SetSelection(0)
        message_key = {
            "json": "cookies_file_json_imported",
            "header": "cookies_file_header_imported",
            "netscape": "cookies_file_netscape_imported",
        }.get(str(result.get("kind") or ""), "cookies_file_imported")
        self.announce_player(self.t(message_key, path=imported_path))
        if result.get("has_login"):
            self.announce_player(self.t("cookies_file_login_found"))
        else:
            self.message(self.t("cookies_file_no_login_warning"), wx.ICON_WARNING)



    def cookie_browser_process_names(self, browser: str) -> list[str]:
        return COOKIES_BROWSER_PROCESS_NAMES.get(str(browser or "").lower(), [])


    def cookie_browser_is_running(self, browser: str) -> bool:
        if os.name != "nt":
            return False
        creationflags = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0
        for name in self.cookie_browser_process_names(browser):
            try:
                result = subprocess.run(
                    ["tasklist", "/FI", f"IMAGENAME eq {name}.exe", "/NH"],
                    capture_output=True,
                    text=True,
                    timeout=2,
                    creationflags=creationflags,
                )
                if f"{name}.exe".lower() in (result.stdout or "").lower():
                    return True
            except Exception:
                continue
        return False


    def close_cookie_browser_processes(self, browser: str) -> bool:
        if os.name != "nt":
            return False
        creationflags = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0
        attempted = False
        for name in self.cookie_browser_process_names(browser):
            attempted = True
            try:
                subprocess.run(
                    ["taskkill", "/IM", f"{name}.exe", "/T", "/F"],
                    capture_output=True,
                    text=True,
                    timeout=5,
                    creationflags=creationflags,
                )
            except Exception:
                continue
        if attempted:
            time.sleep(1.0)
        return attempted and not self.cookie_browser_is_running(browser)


    def export_browser_cookies_worker(self, browser: str) -> None:
        try:
            result = self.export_browser_cookies_blocking(browser, allow_close=True)
            wx.CallAfter(self.finish_browser_cookies_export, str(result["path"]), str(result["profile_label"]), browser)
        except Exception as exc:
            wx.CallAfter(self.message, self.t("browser_cookies_export_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)


    def finish_browser_cookies_export(self, path: str, profile_label: str, browser: str) -> None:
        self.settings.cookies_file = path
        self.settings.cookies_from_browser = browser
        self.cookie_repair_suppressed_until = 0.0
        self.save_settings()
        if hasattr(self, "controls"):
            if "cookies" in self.controls:
                self.controls["cookies"].SetValue(path)
            if "cookies_from_browser" in self.controls:
                selection = COOKIES_BROWSER_OPTIONS.index(browser) if browser in COOKIES_BROWSER_OPTIONS else 0
                self.controls["cookies_from_browser"].SetSelection(selection)
        self.announce_player(self.t("browser_cookies_exported", path=path, profile=profile_label))


