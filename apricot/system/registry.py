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

class RegistryMixin:

    def open_windows_default_apps_settings(self) -> None:
        try:
            if os.name == "nt" and not self.media_association_registry_complete():
                self.register_media_associations_current_user()
                self.set_status(self.t("media_association_registered"))
            os.startfile("ms-settings:defaultapps")  # type: ignore[attr-defined]
            self.announce_player(self.t("default_player_settings_opened"))
        except Exception as exc:
            try:
                subprocess.Popen(["control.exe", "/name", "Microsoft.DefaultPrograms"])
                self.announce_player(self.t("default_player_settings_opened"))
            except Exception:
                self.message(self.t("default_player_settings_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)


    @staticmethod
    def registry_read_value(root, subkey: str, value_name: str = "") -> str:
        if winreg is None:
            return ""
        try:
            with winreg.OpenKey(root, subkey, 0, winreg.KEY_READ) as key:
                value, _value_type = winreg.QueryValueEx(key, value_name)
                return str(value or "")
        except Exception:
            return ""


    def media_association_registry_complete(self) -> bool:
        if os.name != "nt" or winreg is None:
            return True
        expected_exe = str(self.current_executable_path()).lower()
        required_extensions = [".mp3", ".mp4", ".mkv", ".m4a", ".flac", ".wav", ".webm"]
        for root in (winreg.HKEY_CURRENT_USER, winreg.HKEY_LOCAL_MACHINE):
            registered = self.registry_read_value(root, r"Software\RegisteredApplications", APP_NAME)
            command = self.registry_read_value(root, rf"Software\Classes\{APP_NAME}.Media\shell\open\command")
            if not registered or not command:
                continue
            command_lower = command.lower()
            if expected_exe not in command_lower or "%1" not in command_lower:
                continue
            extension_commands_ok = True
            for extension in required_extensions:
                extension_command = self.registry_read_value(root, rf"Software\Classes\SystemFileAssociations\{extension}\shell\{APP_NAME}\command")
                extension_command_lower = extension_command.lower()
                if expected_exe not in extension_command_lower or "%1" not in extension_command_lower:
                    extension_commands_ok = False
                    break
            if extension_commands_ok:
                return True
        return False


    @staticmethod
    def registry_set_value(root, subkey: str, value_name: str, value_type: int, value) -> None:
        if winreg is None:
            raise RuntimeError("Windows registry is not available")
        with winreg.CreateKeyEx(root, subkey, 0, winreg.KEY_WRITE) as key:
            winreg.SetValueEx(key, value_name, 0, value_type, value)


    def register_media_associations_current_user(self) -> None:
        if os.name != "nt" or winreg is None:
            return
        exe_path = str(self.current_executable_path())
        command = f'"{exe_path}" "%1"'
        icon = f"{exe_path},0"
        root = winreg.HKEY_CURRENT_USER
        self.registry_set_value(root, r"Software\RegisteredApplications", APP_NAME, winreg.REG_SZ, rf"Software\{APP_NAME}\Capabilities")
        self.registry_set_value(root, rf"Software\{APP_NAME}\Capabilities", "ApplicationName", winreg.REG_SZ, APP_NAME)
        self.registry_set_value(
            root,
            rf"Software\{APP_NAME}\Capabilities",
            "ApplicationDescription",
            winreg.REG_SZ,
            "Accessible media player, YouTube player, downloader, podcast and RSS player",
        )
        for extension in sorted(LOCAL_MEDIA_EXTENSIONS):
            self.registry_set_value(root, rf"Software\{APP_NAME}\Capabilities\FileAssociations", extension, winreg.REG_SZ, f"{APP_NAME}.Media")
            self.registry_set_value(root, rf"Software\Classes\{extension}\OpenWithProgids", f"{APP_NAME}.Media", winreg.REG_NONE, b"")
            extension_base = rf"Software\Classes\SystemFileAssociations\{extension}\shell\{APP_NAME}"
            self.registry_set_value(root, extension_base, "MUIVerb", winreg.REG_SZ, f"Play with {APP_NAME}")
            self.registry_set_value(root, extension_base, "Icon", winreg.REG_SZ, icon)
            self.registry_set_value(root, rf"{extension_base}\command", "", winreg.REG_SZ, command)
        self.registry_set_value(root, rf"Software\Classes\{APP_NAME}.Media", "", winreg.REG_SZ, f"{APP_NAME} media file")
        self.registry_set_value(root, rf"Software\Classes\{APP_NAME}.Media\DefaultIcon", "", winreg.REG_SZ, icon)
        self.registry_set_value(root, rf"Software\Classes\{APP_NAME}.Media\shell\open\command", "", winreg.REG_SZ, command)
        for media_kind in ("audio", "video"):
            base = rf"Software\Classes\SystemFileAssociations\{media_kind}\shell\{APP_NAME}"
            self.registry_set_value(root, base, "MUIVerb", winreg.REG_SZ, f"Play with {APP_NAME}")
            self.registry_set_value(root, base, "Icon", winreg.REG_SZ, icon)
            self.registry_set_value(root, rf"{base}\command", "", winreg.REG_SZ, command)
        try:
            ctypes.windll.shell32.SHChangeNotify(0x08000000, 0x0000, None, None)
        except Exception:
            pass


    def maybe_prompt_media_association_registration(self) -> None:
        if not self.is_installed_build() or self.media_association_registry_complete():
            return
        if str(getattr(self.settings, "media_association_prompted_version", "")) == APP_VERSION:
            return
        result = wx.MessageBox(
            self.t("media_association_prompt_message"),
            self.t("media_association_prompt_title"),
            wx.YES_NO | wx.ICON_QUESTION,
        )
        self.settings.media_association_prompted_version = APP_VERSION
        self.save_settings()
        if result != wx.YES:
            return
        try:
            self.register_media_associations_current_user()
            self.set_status(self.t("media_association_registered"))
            self.speak_text(self.t("media_association_registered"))
        except Exception as exc:
            self.message(self.t("media_association_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)

