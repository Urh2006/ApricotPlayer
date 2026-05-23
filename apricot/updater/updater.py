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

class AppUpdaterMixin:

    def start_ytdlp_update_check(self, manual: bool = False) -> None:
        threading.Thread(target=self.update_ytdlp_worker, args=(manual,), daemon=True).start()


    def manual_ytdlp_update_check(self) -> None:
        self.apply_settings_from_visible_controls()
        self.set_status(self.t("checking_updates"))
        self.announce_player(self.t("checking_updates"))
        self.start_ytdlp_update_check(manual=True)


    def update_ytdlp_worker(self, manual: bool = False) -> None:
        ytdlp = get_yt_dlp()
        if ytdlp is None:
            self.ui_queue.put(("announce", self.t("missing_ytdlp")))
            return
        try:
            updated = self.update_ytdlp_component_package(ytdlp)
            if updated:
                self.ui_queue.put(("announce", self.t("components_updated")))
            elif manual:
                self.ui_queue.put(("announce", self.t("updates_ok")))
        except Exception as exc:
            self.ui_queue.put(("announce", self.t("updates_failed", error=exc)))


    def update_ytdlp_component_package(self, ytdlp_module) -> bool:
        try:
            current_version = str(import_module("yt_dlp.version").__version__)
        except Exception:
            current_version = str(getattr(ytdlp_module, "__version__", "0") or "0")
        latest_version, wheel_url, wheel_sha256 = self.fetch_latest_ytdlp_wheel()
        if not self.is_component_version_newer(latest_version, current_version):
            return False
        if not wheel_url:
            raise RuntimeError("yt-dlp wheel URL is empty")
        self.validate_trusted_download_url(wheel_url, {"files.pythonhosted.org", "pypi.org", "pypi.python.org"})
        self.ui_queue.put(("announce", self.t("components_updating")))
        COMPONENTS_DIR.mkdir(parents=True, exist_ok=True)
        temp_dir = Path(tempfile.mkdtemp(prefix="apricotplayer-ytdlp-"))
        wheel_path = temp_dir / "yt_dlp.whl"
        extract_dir = temp_dir / "extract"
        try:
            request = Request(wheel_url, headers={"User-Agent": f"{APP_NAME}/{APP_VERSION}"})
            with self.open_url(request, timeout=120) as response, wheel_path.open("wb") as handle:
                self.validate_https_response_url(response.geturl())
                shutil.copyfileobj(response, handle)
            if wheel_sha256:
                self.verify_file_sha256(wheel_path, wheel_sha256)
            extract_dir.mkdir(parents=True, exist_ok=True)
            zipfile_module = import_module("zipfile")
            with zipfile_module.ZipFile(wheel_path) as archive:
                self.safe_extract_zip(archive, extract_dir)
            package_source = extract_dir / "yt_dlp"
            if not package_source.exists():
                raise RuntimeError("yt-dlp wheel did not contain yt_dlp package")
            package_target = COMPONENTS_DIR / "yt_dlp"
            old_target = COMPONENTS_DIR / "yt_dlp.old"
            renamed_old = False
            if old_target.exists():
                shutil.rmtree(old_target, ignore_errors=True)
            if package_target.exists():
                package_target.rename(old_target)
                renamed_old = True
            try:
                shutil.copytree(package_source, package_target)
            except Exception:
                if renamed_old and old_target.exists() and not package_target.exists():
                    old_target.rename(package_target)
                raise
            for dist_info in COMPONENTS_DIR.glob("yt_dlp-*.dist-info"):
                shutil.rmtree(dist_info, ignore_errors=True)
            for dist_info in extract_dir.glob("yt_dlp-*.dist-info"):
                shutil.copytree(dist_info, COMPONENTS_DIR / dist_info.name)
            if old_target.exists():
                shutil.rmtree(old_target, ignore_errors=True)
            self.reload_ytdlp_after_component_update()
            return True
        finally:
            shutil.rmtree(temp_dir, ignore_errors=True)


    def fetch_latest_ytdlp_wheel(self) -> tuple[str, str, str]:
        request = Request(YTDLP_PYPI_JSON_URL, headers={"User-Agent": f"{APP_NAME}/{APP_VERSION}"})
        with self.open_url(request, timeout=30) as response:
            payload = json.loads(response.read().decode("utf-8"))
        latest_version = str((payload.get("info") or {}).get("version") or "")
        urls = payload.get("urls") or []
        for item in urls:
            filename = str(item.get("filename") or "")
            if filename.endswith(".whl") and str(item.get("packagetype") or "") == "bdist_wheel":
                digest = str(((item.get("digests") or {}).get("sha256")) or "")
                return latest_version, str(item.get("url") or ""), digest
        raise RuntimeError("Could not find a yt-dlp wheel on PyPI")


    @staticmethod
    def is_component_version_newer(remote_version: str, current_version: str) -> bool:
        def parts(value: str) -> tuple[int, ...]:
            return tuple(int(part) for part in re.findall(r"\d+", value)[:4]) or (0,)

        remote_parts = parts(remote_version)
        current_parts = parts(current_version)
        length = max(len(remote_parts), len(current_parts))
        return remote_parts + (0,) * (length - len(remote_parts)) > current_parts + (0,) * (length - len(current_parts))


    @staticmethod
    def reload_ytdlp_after_component_update() -> None:
        global yt_dlp, yt_dlp_import_error
        for name in list(sys.modules):
            if name == "yt_dlp" or name.startswith("yt_dlp."):
                sys.modules.pop(name, None)
        yt_dlp = None
        yt_dlp_import_error = None


    def manual_app_update_check(self) -> None:
        self.apply_settings_from_visible_controls()
        self.save_settings()
        self.start_app_update_check(manual=True)


    def start_app_update_check(self, manual: bool = False, prompt: bool = True, notify: bool = False) -> None:
        if not manual and not self.settings.auto_update_app:
            self.set_status(self.t("app_update_disabled"))
            return
        if self.app_update_check_running:
            if manual:
                self.announce_player(self.t("checking_app_updates"))
            return
        self.app_update_check_running = True
        self.set_status(self.t("checking_app_updates"))
        if manual:
            self.announce_player(self.t("checking_app_updates"))
        threading.Thread(target=self.app_update_worker, args=(manual, prompt, notify), daemon=True).start()


    def app_update_worker(self, manual: bool = False, prompt: bool = True, notify: bool = False) -> None:
        try:
            release = self.fetch_latest_release()
            if not release:
                self.report_app_update_status(self.t("app_up_to_date"), manual)
                return
            remote_version = self.release_version(release)
            if not self.is_newer_version(remote_version, APP_VERSION):
                self.report_app_update_status(self.t("app_up_to_date"), manual)
                return
            if not manual and remote_version == self.settings.skipped_update_version:
                self.report_app_update_status(self.t("update_skip_status", version=remote_version), manual)
                return
            asset = self.find_release_asset(release)
            if not asset:
                self.report_app_update_status(self.t("app_update_failed", error="no Windows asset found in release"), manual)
                return
            try:
                cumulative = self.cumulative_changelog_text(APP_VERSION, remote_version)
                if cumulative:
                    release["_cumulative_changelog"] = cumulative
            except Exception:
                pass
            if prompt:
                wx.CallAfter(self.prompt_for_app_update, release, asset)
            else:
                wx.CallAfter(self.store_pending_app_update, release, asset, notify)
        except Exception as exc:
            message = self.t("app_update_failed", error=exc)
            self.report_app_update_status(message, manual)
            if manual:
                wx.CallAfter(self.message, message, wx.ICON_ERROR)
        finally:
            self.app_update_check_running = False


    def report_app_update_status(self, message: str, manual: bool = False) -> None:
        self.ui_queue.put(("status", message))
        if manual:
            wx.CallAfter(self.announce_player, message)


    def store_pending_app_update(self, release: dict, asset: dict, notify: bool = False) -> None:
        version = self.release_version(release)
        if not self.is_newer_version(version, APP_VERSION):
            return
        self.pending_app_update_release = release
        self.pending_app_update_asset = asset
        message = self.t("app_update_ready_status", version=version)
        self.set_status(message)
        if notify:
            self.show_desktop_notification(
                self.t("update_available_title"),
                self.t("app_update_notification_message", version=version),
                enabled=self.settings.app_update_notifications,
                only_when_unfocused=True,
            )
        if self.in_main_menu:
            self.show_main_menu()


    def prompt_for_app_update(self, release: dict, asset: dict) -> None:
        version = self.release_version(release)
        self.log_update_event(f"Prompting for update {version} with asset {asset.get('name')}")
        if not getattr(sys, "frozen", False):
            self.message(self.t("update_source_only", version=version))
            return
        changelog = self.release_changelog_text(release)
        if self.show_update_prompt(version, changelog):
            self.log_update_event(f"User selected update now for {version}")
            if self.settings.skipped_update_version:
                self.settings.skipped_update_version = ""
                self.save_settings()
            self.begin_app_update_install(release, asset)
        else:
            self.log_update_event(f"User skipped update {version}")
            self.settings.skipped_update_version = version
            self.pending_app_update_release = None
            self.pending_app_update_asset = None
            self.save_settings()
            self.announce_player(self.t("update_skipped", version=version))


    def show_update_prompt(self, version: str, changelog: str) -> bool:
        dialog = wx.Dialog(self, title=self.t("update_available_title"), style=wx.DEFAULT_DIALOG_STYLE | wx.RESIZE_BORDER)
        dialog.SetName(self.t("update_available_title"))
        dialog.SetMinSize((640, 420))
        root = wx.BoxSizer(wx.VERTICAL)
        version_label = wx.StaticText(dialog, label=self.t("update_version_heading", version=version))
        root.Add(version_label, 0, wx.LEFT | wx.RIGHT | wx.TOP, 10)
        intro = wx.StaticText(dialog, label=self.t("whats_new"))
        root.Add(intro, 0, wx.LEFT | wx.RIGHT | wx.TOP, 10)
        details = wx.TextCtrl(dialog, value=changelog, style=wx.TE_MULTILINE | wx.TE_READONLY | wx.TE_RICH2)
        details.SetName(self.t("whats_new"))
        details.SetMinSize((580, 260))
        root.Add(details, 1, wx.EXPAND | wx.LEFT | wx.RIGHT, 10)
        question = wx.StaticText(dialog, label=self.t("update_now"))
        root.Add(question, 0, wx.ALL, 10)
        buttons = wx.StdDialogButtonSizer()
        update_button = wx.Button(dialog, wx.ID_YES, self.t("update_now_button"))
        skip_button = wx.Button(dialog, wx.ID_NO, self.t("skip_version_button"))
        update_button.SetName(self.t("update_now_button"))
        skip_button.SetName(self.t("skip_version_button"))
        update_button.SetDefault()
        try:
            dialog.SetAffirmativeId(wx.ID_YES)
            dialog.SetEscapeId(wx.ID_NO)
        except Exception:
            pass
        update_button.Bind(wx.EVT_BUTTON, lambda _event: dialog.EndModal(wx.ID_YES))
        skip_button.Bind(wx.EVT_BUTTON, lambda _event: dialog.EndModal(wx.ID_NO))
        buttons.AddButton(update_button)
        buttons.AddButton(skip_button)
        buttons.Realize()
        root.Add(buttons, 0, wx.EXPAND | wx.ALL, 10)
        dialog.SetSizerAndFit(root)
        wx.CallAfter(self.safe_set_focus, details)
        try:
            return dialog.ShowModal() == wx.ID_YES
        finally:
            dialog.Destroy()


    def begin_app_update_install(self, release: dict, asset: dict) -> None:
        version = self.release_version(release)
        self.log_update_event(f"Beginning update {version}; asset={asset.get('name')}")
        self.close_update_progress_dialog()
        self.update_progress_dialog = wx.ProgressDialog(
            self.t("update_progress_title"),
            self.t("update_download_unknown", version=version),
            maximum=100,
            parent=self,
            style=wx.PD_APP_MODAL | wx.PD_ELAPSED_TIME | wx.PD_ESTIMATED_TIME,
        )
        self.update_progress_dialog.Pulse(self.t("update_download_unknown", version=version))
        self.announce_player(self.t("downloading_update", version=version))
        threading.Thread(target=self.download_and_install_update, args=(release, asset), daemon=True).start()


    def close_update_progress_dialog(self) -> None:
        if self.update_progress_dialog:
            try:
                self.update_progress_dialog.Destroy()
            except Exception:
                pass
            self.update_progress_dialog = None


    def update_app_update_progress(self, version: str, percent: int | None) -> None:
        if not self.update_progress_dialog:
            return
        try:
            if percent is None:
                self.update_progress_dialog.Pulse(self.t("update_download_unknown", version=version))
            else:
                percent = min(100, max(0, percent))
                self.update_progress_dialog.Update(percent, self.t("update_download_percent", version=version, percent=percent))
        except Exception:
            pass


    def update_app_update_finished(self, version: str) -> None:
        if self.update_progress_dialog:
            try:
                self.update_progress_dialog.Update(100, self.t("update_download_complete"))
            except Exception:
                pass
        self.announce_player(self.t("update_download_complete"))


    def update_app_update_failed(self, error: Exception | str) -> None:
        self.log_update_event(f"Update failed before install: {error}")
        self.close_update_progress_dialog()
        self.message(self.t("app_update_failed", error=error), wx.ICON_ERROR)


    def download_and_install_update(self, release: dict, asset: dict) -> None:
        version = self.release_version(release)
        temp_dir: Path | None = None
        try:
            self.ui_queue.put(("status", self.t("downloading_update", version=version)))
            temp_dir = Path(tempfile.mkdtemp(prefix="apricotplayer-update-"))
            downloaded_path = temp_dir / self.safe_asset_filename(asset)
            self.log_update_event(f"Downloading update {version} to {downloaded_path}")
            self.download_update_asset(asset, downloaded_path, version)
            self.log_update_event(f"Downloaded update {version}; size={downloaded_path.stat().st_size}")
            self.verify_release_asset_file(asset, downloaded_path)
            self.validate_update_package(downloaded_path)
            wx.CallAfter(self.update_app_update_finished, version)
            wx.CallAfter(self.finish_app_update_install, str(downloaded_path), version)
        except Exception as exc:
            if temp_dir:
                try:
                    shutil.rmtree(temp_dir, ignore_errors=True)
                except Exception:
                    pass
            wx.CallAfter(self.update_app_update_failed, exc)


    def download_update_asset(self, asset: dict, downloaded_path: Path, version: str) -> None:
        attempts: list[tuple[str, dict[str, str]]] = []
        browser_url = str(asset.get("browser_download_url") or "")
        api_url = str(asset.get("url") or "")
        if browser_url:
            self.validate_trusted_download_url(browser_url, {"github.com"})
            attempts.append((browser_url, self.github_headers("", accept="application/octet-stream")))
        if api_url:
            self.validate_trusted_download_url(api_url, {"api.github.com"})
            attempts.append((api_url, self.github_headers("", accept="application/octet-stream")))
        if not attempts:
            raise RuntimeError("missing download url")
        last_error: Exception | None = None
        for download_url, headers in attempts:
            try:
                started = time.monotonic()
                self.log_update_event(f"Download attempt: host={urlparse(download_url).hostname or ''}; asset={asset.get('name')}; expected_size={asset.get('size') or 'unknown'}")
                request = Request(download_url, headers=headers)
                with self.open_url(request, timeout=300) as response, downloaded_path.open("wb") as handle:
                    self.validate_https_response_url(response.geturl())
                    total_header = response.headers.get("Content-Length")
                    total = int(total_header) if total_header and total_header.isdigit() else 0
                    downloaded = 0
                    last_percent = -1
                    last_progress_time = 0.0
                    while True:
                        chunk = response.read(UPDATE_DOWNLOAD_CHUNK_SIZE)
                        if not chunk:
                            break
                        handle.write(chunk)
                        downloaded += len(chunk)
                        now = time.monotonic()
                        if total:
                            percent = int(downloaded * 100 / total)
                            if percent != last_percent and (percent >= 100 or now - last_progress_time >= UPDATE_PROGRESS_MIN_INTERVAL):
                                last_percent = percent
                                last_progress_time = now
                                wx.CallAfter(self.update_app_update_progress, version, percent)
                        elif now - last_progress_time >= UPDATE_PROGRESS_MIN_INTERVAL:
                            last_progress_time = now
                            wx.CallAfter(self.update_app_update_progress, version, None)
                elapsed = max(0.001, time.monotonic() - started)
                self.log_update_event(f"Download completed: bytes={downloaded_path.stat().st_size}; seconds={elapsed:.1f}; mbps={(downloaded_path.stat().st_size * 8 / 1_000_000 / elapsed):.2f}")
                return
            except Exception as exc:
                last_error = exc
                self.log_update_event(f"Download attempt failed from {urlparse(download_url).hostname or download_url}: {exc}")
                try:
                    downloaded_path.unlink(missing_ok=True)
                except Exception:
                    pass
        raise RuntimeError(last_error or "download failed")


    @staticmethod
    def safe_asset_filename(asset: dict) -> str:
        name = Path(str(asset.get("name") or "")).name
        if name not in {INSTALLER_ASSET_NAME, PORTABLE_ZIP_ASSET_NAME, LEGACY_PORTABLE_ZIP_ASSET_NAME}:
            raise RuntimeError(f"unexpected update asset name: {name or 'missing'}")
        return name


    @staticmethod
    def validate_trusted_download_url(download_url: str, allowed_hosts: set[str]) -> None:
        parsed = urlparse(str(download_url or ""))
        host = (parsed.hostname or "").lower()
        if parsed.scheme.lower() != "https" or host not in {allowed.lower() for allowed in allowed_hosts}:
            raise RuntimeError(f"untrusted download URL: {download_url}")


    @staticmethod
    def validate_https_response_url(download_url: str) -> None:
        parsed = urlparse(str(download_url or ""))
        if parsed.scheme.lower() != "https":
            raise RuntimeError(f"download redirected to a non-HTTPS URL: {download_url}")


    @staticmethod
    def file_sha256(path: Path) -> str:
        digest = hashlib.sha256()
        with path.open("rb") as handle:
            for chunk in iter(lambda: handle.read(1024 * 1024), b""):
                digest.update(chunk)
        return digest.hexdigest()


    @classmethod
    def verify_file_sha256(cls, path: Path, expected_sha256: str) -> None:
        expected = str(expected_sha256 or "").strip().lower()
        if expected.startswith("sha256:"):
            expected = expected.split(":", 1)[1]
        if not expected:
            return
        actual = cls.file_sha256(path)
        if actual.lower() != expected:
            raise RuntimeError("downloaded file checksum did not match the published SHA-256 digest")


    @classmethod
    def verify_release_asset_file(cls, asset: dict, path: Path) -> None:
        expected_size = asset.get("size")
        if isinstance(expected_size, int) and expected_size > 0 and path.stat().st_size != expected_size:
            raise RuntimeError("downloaded update size did not match the GitHub release asset size")
        cls.verify_file_sha256(path, str(asset.get("digest") or ""))


    def finish_app_update_install(self, downloaded_path: str, version: str) -> None:
        if not getattr(sys, "frozen", False):
            self.message(self.t("update_source_only", version=version))
            return
        current_exe = Path(sys.executable)
        self.log_update_event(f"Preparing install for {version}; package={downloaded_path}; current_exe={current_exe}")
        if self.is_installer_asset(downloaded_path):
            script_path = self.write_installer_update_script(downloaded_path, str(current_exe.parent), os.getpid(), str(UPDATE_LOG_FILE), restart=True)
        elif self.is_portable_zip_asset(downloaded_path):
            script_path = self.write_portable_zip_update_script(downloaded_path, str(current_exe.parent), str(current_exe), os.getpid(), str(UPDATE_LOG_FILE), restart=True)
        else:
            script_path = self.write_update_script(downloaded_path, str(current_exe), os.getpid(), str(UPDATE_LOG_FILE), restart=True)
        self.log_update_event(f"Launching update script {script_path}")
        self.launch_update_script(script_path)
        self.set_status(self.t("installing_update", version=version))
        self.close_update_progress_dialog()
        self.announce_player(self.t("update_install_started"))
        self.set_status(self.t("update_install_log", path=UPDATE_LOG_FILE))
        self.log_update_event("Exiting ApricotPlayer for update")
        self.exit_for_update()


    @staticmethod
    def is_installer_asset(path_or_name: str | Path) -> bool:
        name = Path(path_or_name).name.lower()
        return name == INSTALLER_ASSET_NAME.lower() or "setup" in name or "installer" in name


    @staticmethod
    def is_portable_zip_asset(path_or_name: str | Path) -> bool:
        name = Path(path_or_name).name.lower()
        return name in {PORTABLE_ZIP_ASSET_NAME.lower(), LEGACY_PORTABLE_ZIP_ASSET_NAME.lower()} or (name.endswith(".zip") and "apricotplayer" in name)


    @staticmethod
    def validate_zip_member_path(member_name: str) -> None:
        normalized = member_name.replace("\\", "/")
        if not normalized or normalized.startswith("/") or re.match(r"^[A-Za-z]:", normalized):
            raise RuntimeError("zip package contains an unsafe absolute path")
        parts = [part for part in normalized.split("/") if part]
        if any(part == ".." for part in parts):
            raise RuntimeError("zip package contains an unsafe parent-directory path")

    @classmethod
    def validate_update_package(cls, path: Path) -> None:
        if not path.exists() or path.stat().st_size < 1024 * 1024:
            raise RuntimeError("downloaded update is not a valid package")
        if cls.is_portable_zip_asset(path):
            zipfile_module = import_module("zipfile")
            if not zipfile_module.is_zipfile(path):
                raise RuntimeError("downloaded portable update is not a valid zip file")
            with zipfile_module.ZipFile(path) as archive:
                for member in archive.infolist():
                    cls.validate_zip_member_path(member.filename)
                if not any(Path(member.filename.replace("\\", "/")).name.lower() == "apricotplayer.exe" for member in archive.infolist()):
                    raise RuntimeError("downloaded portable update does not contain ApricotPlayer.exe")
            return
        with path.open("rb") as handle:
            if handle.read(2) != b"MZ":
                raise RuntimeError("downloaded update is not a Windows executable")


    @classmethod
    def write_update_script(cls, downloaded_path: str, target_path: str, process_id: int, log_path: str, restart: bool = True) -> Path:
        script_path = Path(tempfile.gettempdir()) / f"apricotplayer-update-{int(time.time())}.ps1"
        restart_value = "$true" if restart else "$false"
        script = "\n".join(
            [
                "$ErrorActionPreference = 'Stop'",
                f"$source = {cls.powershell_literal(downloaded_path)}",
                f"$target = {cls.powershell_literal(target_path)}",
                f"$log = {cls.powershell_literal(log_path)}",
                f"$processIdToWait = {int(process_id)}",
                f"$restart = {restart_value}",
                "$targetDir = Split-Path -Parent $target",
                "$oldTarget = \"$target.old\"",
                "New-Item -ItemType Directory -Path (Split-Path -Parent $log) -Force | Out-Null",
                "function Log($message) { Add-Content -LiteralPath $log -Value ((Get-Date -Format o) + ' ' + $message) -Encoding UTF8 }",
                "Set-Content -LiteralPath $log -Value ((Get-Date -Format o) + ' Starting ApricotPlayer update') -Encoding UTF8",
                "Log \"Source: $source\"",
                "Log \"Target: $target\"",
                "Start-Sleep -Milliseconds 500",
                "if ($processIdToWait -gt 0) {",
                "    try { Wait-Process -Id $processIdToWait -Timeout 15 -ErrorAction SilentlyContinue } catch { Log \"Wait-Process warning: $($_.Exception.Message)\" }",
                "    try {",
                "        $stillRunning = Get-Process -Id $processIdToWait -ErrorAction SilentlyContinue",
                "        if ($stillRunning) { Log 'ApricotPlayer did not exit; forcing shutdown'; Stop-Process -Id $processIdToWait -Force -ErrorAction SilentlyContinue }",
                "    } catch { Log \"Force shutdown warning: $($_.Exception.Message)\" }",
                "}",
                "$copied = $false",
                "for ($attempt = 0; $attempt -lt 180; $attempt++) {",
                "    try {",
                "        if (Test-Path -LiteralPath $oldTarget) { Remove-Item -LiteralPath $oldTarget -Force -ErrorAction SilentlyContinue }",
                "        if (Test-Path -LiteralPath $target) { Rename-Item -LiteralPath $target -NewName (Split-Path -Leaf $oldTarget) -Force -ErrorAction Stop }",
                "        Copy-Item -LiteralPath $source -Destination $target -Force -ErrorAction Stop",
                "        if ((Get-Item -LiteralPath $target).Length -lt 1048576) { throw 'Copied file is too small.' }",
                "        $copied = $true",
                "        Log \"Copy succeeded on attempt $attempt\"",
                "        break",
                "    } catch {",
                "        Log \"Copy attempt $attempt failed: $($_.Exception.Message)\"",
                "        if ((Test-Path -LiteralPath $oldTarget) -and -not (Test-Path -LiteralPath $target)) {",
                "            try { Rename-Item -LiteralPath $oldTarget -NewName (Split-Path -Leaf $target) -Force -ErrorAction SilentlyContinue } catch { }",
                "        }",
                "        Start-Sleep -Seconds 1",
                "    }",
                "}",
                "if (-not $copied) { Log 'Update failed: could not copy new executable'; exit 1 }",
                "Remove-Item -LiteralPath $source -Force -ErrorAction SilentlyContinue",
                "if (Test-Path -LiteralPath $oldTarget) { Remove-Item -LiteralPath $oldTarget -Force -ErrorAction SilentlyContinue }",
                f"if ($restart) {{ Log 'Restarting ApricotPlayer'; Start-Process -FilePath $target -WorkingDirectory $targetDir -ArgumentList {cls.powershell_literal(UPDATE_RELAUNCH_ARG)} }}",
                "Log 'Update complete'",
                "Start-Sleep -Seconds 2",
                "Remove-Item -LiteralPath $PSCommandPath -Force -ErrorAction SilentlyContinue",
            ]
        )
        script_path.write_text(script, encoding="utf-8-sig")
        return script_path


    @classmethod
    def write_portable_zip_update_script(cls, downloaded_path: str, target_dir: str, target_exe: str, process_id: int, log_path: str, restart: bool = True) -> Path:
        script_path = Path(tempfile.gettempdir()) / f"apricotplayer-portable-update-{int(time.time())}.ps1"
        restart_value = "$true" if restart else "$false"
        script = "\n".join(
            [
                "$ErrorActionPreference = 'Stop'",
                f"$source = {cls.powershell_literal(downloaded_path)}",
                f"$targetDir = {cls.powershell_literal(target_dir)}",
                f"$targetExe = {cls.powershell_literal(target_exe)}",
                f"$log = {cls.powershell_literal(log_path)}",
                f"$processIdToWait = {int(process_id)}",
                f"$restart = {restart_value}",
                "$extractRoot = Join-Path ([IO.Path]::GetTempPath()) ('apricotplayer-portable-' + [Guid]::NewGuid().ToString())",
                "New-Item -ItemType Directory -Path (Split-Path -Parent $log) -Force | Out-Null",
                "function Log($message) { Add-Content -LiteralPath $log -Value ((Get-Date -Format o) + ' ' + $message) -Encoding UTF8 }",
                "Set-Content -LiteralPath $log -Value ((Get-Date -Format o) + ' Starting ApricotPlayer portable update') -Encoding UTF8",
                "Log \"Source: $source\"",
                "Log \"Target directory: $targetDir\"",
                "Start-Sleep -Milliseconds 500",
                "if ($processIdToWait -gt 0) {",
                "    try { Wait-Process -Id $processIdToWait -Timeout 15 -ErrorAction SilentlyContinue } catch { Log \"Wait-Process warning: $($_.Exception.Message)\" }",
                "    try {",
                "        $stillRunning = Get-Process -Id $processIdToWait -ErrorAction SilentlyContinue",
                "        if ($stillRunning) { Log 'ApricotPlayer did not exit; forcing shutdown'; Stop-Process -Id $processIdToWait -Force -ErrorAction SilentlyContinue }",
                "    } catch { Log \"Force shutdown warning: $($_.Exception.Message)\" }",
                "}",
                "try {",
                "    New-Item -ItemType Directory -Path $extractRoot -Force | Out-Null",
                "    Expand-Archive -LiteralPath $source -DestinationPath $extractRoot -Force",
                "    $sourceAppDir = Join-Path $extractRoot 'ApricotPlayer'",
                "    if (-not (Test-Path -LiteralPath (Join-Path $sourceAppDir 'ApricotPlayer.exe'))) {",
                "        $candidate = Get-ChildItem -LiteralPath $extractRoot -Filter 'ApricotPlayer.exe' -Recurse -File | Select-Object -First 1",
                "        if (-not $candidate) { throw 'ApricotPlayer.exe was not found in portable zip.' }",
                "        $sourceAppDir = Split-Path -Parent $candidate.FullName",
                "    }",
                "    Log \"Extracted app directory: $sourceAppDir\"",
                "    Get-ChildItem -LiteralPath $sourceAppDir -Force | ForEach-Object {",
                "        Copy-Item -LiteralPath $_.FullName -Destination $targetDir -Recurse -Force -ErrorAction Stop",
                "    }",
                "    if (-not (Test-Path -LiteralPath $targetExe)) { throw 'Updated ApricotPlayer.exe is missing after copy.' }",
                "    Remove-Item -LiteralPath $source -Force -ErrorAction SilentlyContinue",
                "    Remove-Item -LiteralPath $extractRoot -Recurse -Force -ErrorAction SilentlyContinue",
                f"    if ($restart) {{ Log 'Restarting ApricotPlayer'; Start-Process -FilePath $targetExe -WorkingDirectory $targetDir -ArgumentList {cls.powershell_literal(UPDATE_RELAUNCH_ARG)} }}",
                "    Log 'Update complete'",
                "} catch {",
                "    Log \"Portable update failed: $($_.Exception.Message)\"",
                "    try { if (Test-Path -LiteralPath $extractRoot) { Remove-Item -LiteralPath $extractRoot -Recurse -Force -ErrorAction SilentlyContinue } } catch { }",
                "    exit 1",
                "}",
                "Start-Sleep -Seconds 2",
                "Remove-Item -LiteralPath $PSCommandPath -Force -ErrorAction SilentlyContinue",
            ]
        )
        script_path.write_text(script, encoding="utf-8-sig")
        return script_path


    @classmethod
    def write_installer_update_script(cls, downloaded_path: str, install_dir: str, process_id: int, log_path: str, restart: bool = True) -> Path:
        script_path = Path(tempfile.gettempdir()) / f"apricotplayer-installer-update-{int(time.time())}.ps1"
        restart_value = "$true" if restart else "$false"
        script = "\n".join(
            [
                "$ErrorActionPreference = 'Stop'",
                f"$source = {cls.powershell_literal(downloaded_path)}",
                f"$installDir = {cls.powershell_literal(install_dir)}",
                f"$log = {cls.powershell_literal(log_path)}",
                f"$processIdToWait = {int(process_id)}",
                f"$restart = {restart_value}",
                "$installerLog = [IO.Path]::ChangeExtension($log, '.inno.log')",
                "$silentArgs = @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/CLOSEAPPLICATIONS', '/TASKS=desktopicon,mediaassoc', ('/DIR=\"' + $installDir + '\"'), ('/LOG=\"' + $installerLog + '\"'))",
                "$installCandidates = @()",
                "function Normalize-ExecutablePath([string]$path) {",
                "    if (-not $path) { return '' }",
                "    $candidate = $path.Trim().Trim('\"')",
                "    if ($candidate -match '^(.*?\\.exe)') { $candidate = $matches[1] }",
                "    return $candidate",
                "}",
                "function Add-InstallCandidate([string]$path) {",
                "    $candidate = Normalize-ExecutablePath $path",
                "    if ($candidate -and -not ($script:installCandidates -contains $candidate)) { $script:installCandidates += $candidate }",
                "}",
                "function Find-InstalledApricotExe {",
                "    $roots = @(",
                "        'HKLM:\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\*',",
                "        'HKLM:\\Software\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\*',",
                "        'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\*'",
                "    )",
                "    foreach ($root in $roots) {",
                "        try {",
                "            $items = @(Get-ItemProperty -Path $root -ErrorAction SilentlyContinue)",
                "            foreach ($item in $items) {",
                "                if ($item.DisplayName -ne 'ApricotPlayer') { continue }",
                "                if ($item.InstallLocation) {",
                "                    $candidate = Join-Path $item.InstallLocation 'ApricotPlayer.exe'",
                "                    if (Test-Path -LiteralPath $candidate) { return $candidate }",
                "                }",
                "                $icon = Normalize-ExecutablePath ([string]$item.DisplayIcon)",
                "                if ($icon -and (Test-Path -LiteralPath $icon)) { return $icon }",
                "            }",
                "        } catch { }",
                "    }",
                "    return $null",
                "}",
                "function Stop-ApricotProcesses([string[]]$dirs) {",
                "    try {",
                "        $normalizedDirs = @($dirs | Where-Object { $_ } | ForEach-Object { try { [IO.Path]::GetFullPath($_).TrimEnd('\\') } catch { $_ } } | Select-Object -Unique)",
                "        Get-CimInstance Win32_Process -Filter \"Name = 'ApricotPlayer.exe'\" -ErrorAction SilentlyContinue | ForEach-Object {",
                "            $processPath = $_.ExecutablePath",
                "            if (-not $processPath) { return }",
                "            $processDir = Split-Path -Parent $processPath",
                "            try { $processDir = [IO.Path]::GetFullPath($processDir).TrimEnd('\\') } catch { }",
                "            if ($normalizedDirs -contains $processDir) {",
                "                Log \"Stopping ApricotPlayer process $($_.ProcessId) at $processPath\"",
                "                Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue",
                "            }",
                "        }",
                "    } catch { Log \"Process cleanup warning: $($_.Exception.Message)\" }",
                "}",
                "Add-InstallCandidate (Join-Path $installDir 'ApricotPlayer.exe')",
                "if ($env:ProgramFiles) { Add-InstallCandidate (Join-Path $env:ProgramFiles 'ApricotPlayer\\ApricotPlayer.exe') }",
                "if (${env:ProgramFiles(x86)}) { Add-InstallCandidate (Join-Path ${env:ProgramFiles(x86)} 'ApricotPlayer\\ApricotPlayer.exe') }",
                "New-Item -ItemType Directory -Path (Split-Path -Parent $log) -Force | Out-Null",
                "function Log($message) { Add-Content -LiteralPath $log -Value ((Get-Date -Format o) + ' ' + $message) -Encoding UTF8 }",
                "Set-Content -LiteralPath $log -Value ((Get-Date -Format o) + ' Starting ApricotPlayer installer update') -Encoding UTF8",
                "Log \"Installer: $source\"",
                "Log \"Install directory: $installDir\"",
                "Start-Sleep -Milliseconds 500",
                "if ($processIdToWait -gt 0) {",
                "    try { Wait-Process -Id $processIdToWait -Timeout 15 -ErrorAction SilentlyContinue } catch { Log \"Wait-Process warning: $($_.Exception.Message)\" }",
                "    try {",
                "        $stillRunning = Get-Process -Id $processIdToWait -ErrorAction SilentlyContinue",
                "        if ($stillRunning) { Log 'ApricotPlayer did not exit; forcing shutdown'; Stop-Process -Id $processIdToWait -Force -ErrorAction SilentlyContinue }",
                "    } catch { Log \"Force shutdown warning: $($_.Exception.Message)\" }",
                "}",
                "$knownDirs = @($installCandidates | ForEach-Object { Split-Path -Parent $_ } | Where-Object { $_ } | Select-Object -Unique)",
                "Stop-ApricotProcesses $knownDirs",
                "try {",
                "    Log 'Launching installer'",
                "    $process = Start-Process -FilePath $source -ArgumentList $silentArgs -Verb runAs -Wait -PassThru",
                "    if ($process -and $process.ExitCode -ne 0) { throw \"Installer exited with code $($process.ExitCode)\" }",
                "    Log 'Installer completed'",
                "    $installedExe = Find-InstalledApricotExe",
                "    if (-not $installedExe) { $installedExe = Join-Path $installDir 'ApricotPlayer.exe' }",
                "    Add-InstallCandidate $installedExe",
                "    if (-not (Test-Path -LiteralPath $installedExe)) { throw \"Installed ApricotPlayer.exe was not found at $installedExe\" }",
                "    $installedItem = Get-Item -LiteralPath $installedExe",
                "    if ($installedItem.Length -lt 1048576) { throw 'Installed ApricotPlayer.exe is too small.' }",
                "    Log \"Installed executable: $installedExe size=$($installedItem.Length) modified=$($installedItem.LastWriteTimeUtc.ToString('o'))\"",
                "    Remove-Item -LiteralPath $source -Force -ErrorAction SilentlyContinue",
                "    $knownDirs = @($installCandidates | ForEach-Object { Split-Path -Parent $_ } | Where-Object { $_ } | Select-Object -Unique)",
                "    Stop-ApricotProcesses $knownDirs",
                "    if ($restart) {",
                "        $installedDir = Split-Path -Parent $installedExe",
                "        Log \"Restarting ApricotPlayer from $installedExe\"",
                f"        Start-Process -FilePath $installedExe -WorkingDirectory $installedDir -ArgumentList {cls.powershell_literal(UPDATE_RELAUNCH_ARG)}",
                "    }",
                "    Log 'Update complete'",
                "} catch {",
                "    Log \"Installer update failed: $($_.Exception.Message)\"",
                "    exit 1",
                "}",
                "Start-Sleep -Seconds 2",
                "Remove-Item -LiteralPath $PSCommandPath -Force -ErrorAction SilentlyContinue",
            ]
        )
        script_path.write_text(script, encoding="utf-8-sig")
        return script_path


    @staticmethod
    def launch_update_script(script_path: Path) -> None:
        powershell = shutil.which("powershell.exe") or shutil.which("pwsh.exe")
        if not powershell:
            raise RuntimeError("PowerShell was not found")
        args = [powershell, "-NoProfile"]
        if Path(powershell).name.lower() == "powershell.exe":
            args.extend(["-ExecutionPolicy", "Bypass"])
        args.extend(["-File", str(script_path)])
        subprocess.Popen(args, creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0), close_fds=True)

    @staticmethod
    def log_update_event(message: str) -> None:
        try:
            APP_DIR.mkdir(parents=True, exist_ok=True)
            line = f"{datetime.now(timezone.utc).isoformat()} {message}\n"
            with UPDATE_LOG_FILE.open("a", encoding="utf-8") as handle:
                handle.write(line)
        except Exception:
            pass


    def exit_for_update(self) -> None:
        try:
            self.exiting = True
            self.destroy_taskbar_icon()
            self.Destroy()
            app = wx.GetApp()
            if app:
                app.ExitMainLoop()
        finally:
            os._exit(0)


    def fetch_latest_release(self) -> dict | None:
        channel = getattr(self.settings, "update_channel", "stable")
        if channel == "beta":
            try:
                releases = self.fetch_public_releases()
                if releases:
                    return releases[0]
            except Exception:
                pass
            return None
        else:
            try:
                release = self.fetch_github_latest_release()
                if release and not release.get("prerelease"):
                    return release
            except Exception:
                pass
            try:
                releases = self.fetch_public_releases()
                if releases:
                    return releases[0]
            except Exception:
                pass
            return None


    def fetch_github_latest_release(self) -> dict | None:
        latest_request = Request(
            GITHUB_LATEST_RELEASE_API_URL,
            headers=self.github_headers(""),
        )
        try:
            with self.open_url(latest_request, timeout=30) as response:
                release = json.loads(response.read().decode("utf-8"))
            if isinstance(release, dict) and not release.get("draft"):
                return release
        except Exception as exc:
            if exc.__class__.__name__ != "HTTPError" or getattr(exc, "code", None) != 404:
                raise
        return None


    def fetch_public_releases(self) -> list[dict]:
        request = Request(GITHUB_RELEASES_API_URL, headers=self.github_headers(""))
        with self.open_url(request, timeout=30) as response:
            payload = json.loads(response.read().decode("utf-8"))
        channel = getattr(self.settings, "update_channel", "stable")
        if not isinstance(payload, list):
            releases = []
        else:
            releases = []
            for release in payload:
                if not isinstance(release, dict):
                    continue
                if release.get("draft"):
                    continue
                if channel == "stable" and release.get("prerelease"):
                    continue
                releases.append(release)
        try:
            latest = self.fetch_github_latest_release()
            if latest and not (channel == "stable" and latest.get("prerelease")):
                latest_tag = str(latest.get("tag_name") or "")
                latest_id = latest.get("id")
                if not any(release.get("id") == latest_id or str(release.get("tag_name") or "") == latest_tag for release in releases):
                    releases.append(latest)
        except Exception:
            pass
        releases.sort(key=lambda release: self.parse_version(self.release_version(release)), reverse=True)
        return releases


    def cumulative_changelog_text(self, current_version: str, latest_version: str) -> str:
        sections: list[str] = []
        for release in self.fetch_public_releases():
            version = self.release_version(release)
            if not version:
                continue
            if self.is_newer_version(version, current_version) and not self.is_newer_version(version, latest_version):
                body = str(release.get("body") or "").replace("\r\n", "\n").strip() or self.t("no_changelog")
                if re.match(r"^#*\s*what'?s new in version", body, flags=re.IGNORECASE):
                    sections.append(body)
                else:
                    sections.append(f"What's new in version {version}\n\n{body}")
        text = "\n\n".join(sections).strip()
        if len(text) > 12000:
            return text[:12000].rstrip() + "\n\n..."
        return text


    def find_release_asset(self, release: dict) -> dict | None:
        assets = release.get("assets") or []
        portable_names = [PORTABLE_ZIP_ASSET_NAME, LEGACY_PORTABLE_ZIP_ASSET_NAME]
        preferred_names = [INSTALLER_ASSET_NAME, *portable_names] if self.is_installed_build() else [*portable_names, INSTALLER_ASSET_NAME]
        for preferred_name in preferred_names:
            for asset in assets:
                if asset.get("name") == preferred_name:
                    return asset
        return None

    @staticmethod
    def release_version(release: dict) -> str:
        return str(release.get("tag_name") or release.get("name") or "").strip().lstrip("v")


    def release_changelog_text(self, release: dict) -> str:
        cumulative = str(release.get("_cumulative_changelog") or "").strip()
        if cumulative:
            return cumulative
        body = str(release.get("body") or "").replace("\r\n", "\n").strip()
        if not body:
            return self.t("no_changelog")
        if len(body) > 6000:
            return body[:6000].rstrip() + "\n\n..."
        return body

    @classmethod
    def is_newer_version(cls, remote_version: str, current_version: str) -> bool:
        return cls.parse_version(remote_version) > cls.parse_version(current_version)

