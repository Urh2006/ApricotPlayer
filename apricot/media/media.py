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

class MediaMixin:

    def show_file_converter(self) -> None:
        self.show_converter_dialog(folder_mode=False)


    def show_folder_converter(self) -> None:
        self.show_converter_dialog(folder_mode=True)


    def converter_input_kind(self, path: str | Path) -> str:
        suffix = Path(path).suffix.lower()
        if suffix in AUDIO_INPUT_EXTENSIONS:
            return "audio"
        if suffix in VIDEO_INPUT_EXTENSIONS:
            return "video"
        return ""


    def converter_format_values(self, input_kind: str = "") -> list[str]:
        if input_kind == "audio":
            return [*AUDIO_CONVERT_FORMATS, *VIDEO_CONVERT_FORMATS]
        if input_kind == "video":
            return [*VIDEO_CONVERT_FORMATS, *AUDIO_CONVERT_FORMATS]
        return [*AUDIO_CONVERT_FORMATS, *VIDEO_CONVERT_FORMATS]


    @staticmethod
    def converter_format_labels(values: list[str]) -> list[str]:
        labels = []
        for value in values:
            labels.append("ALAC (M4A)" if value == "alac" else value.upper())
        return labels


    @staticmethod
    def converter_output_extension(target_format: str) -> str:
        return "m4a" if target_format == "alac" else target_format


    def converter_wildcard_for_target(self, target_format: str) -> str:
        extension = self.converter_output_extension(target_format)
        return f"{extension.upper()} (*.{extension})|*.{extension}|{self.t('all_files')} (*.*)|*.*"


    def converter_default_output_path(self, source: Path, target_format: str) -> Path:
        extension = self.converter_output_extension(target_format)
        return source.with_name(f"{source.stem}.{extension}")


    def converter_is_audio_to_video(self, source_path: str | Path, target_format: str) -> bool:
        return self.converter_input_kind(source_path) == "audio" and target_format in VIDEO_CONVERT_FORMATS



    def converter_media_files_in_folder(self, folder: Path) -> list[Path]:
        try:
            return sorted(
                path
                for path in folder.rglob("*")
                if path.is_file() and path.suffix.lower() in CONVERTER_MEDIA_EXTENSIONS and ".apricot-converting" not in path.name
            )
        except OSError:
            return []


    def show_converter_dialog(self, folder_mode: bool = False) -> None:
        title = self.t("folder_converter" if folder_mode else "file_converter")
        dialog = wx.Dialog(self, title=title, style=wx.DEFAULT_DIALOG_STYLE | wx.RESIZE_BORDER)
        try:
            main = wx.BoxSizer(wx.VERTICAL)
            form = wx.FlexGridSizer(0, 2, 6, 6)
            form.AddGrowableCol(1, 1)

            path_key = "folder_to_convert" if folder_mode else "file_to_convert"
            path_label = wx.StaticText(dialog, label=self.t(path_key))
            path_ctrl = wx.TextCtrl(dialog)
            path_ctrl.SetName(self.t(path_key))
            browse_button = wx.Button(dialog, label=self.t("browse_folder" if folder_mode else "browse_file"))
            browse_button.SetName(self.t("browse_folder" if folder_mode else "browse_file"))
            path_row = wx.BoxSizer(wx.HORIZONTAL)
            path_row.Add(path_ctrl, 1, wx.EXPAND | wx.RIGHT, 4)
            path_row.Add(browse_button, 0)
            form.Add(path_label, 0, wx.ALIGN_CENTER_VERTICAL)
            form.Add(path_row, 1, wx.EXPAND)

            target_choice = wx.Choice(dialog, choices=[])
            target_choice.SetName(self.t("convert_to"))
            target_values: list[str] = []
            form.Add(wx.StaticText(dialog, label=self.t("convert_to")), 0, wx.ALIGN_CENTER_VERTICAL)
            form.Add(target_choice, 1, wx.EXPAND)

            options_label = wx.StaticText(dialog, label=self.t("converter_audio_to_video_options"))
            add_image_box = wx.CheckBox(dialog, label=self.t("add_image"))
            add_image_box.SetName(self.t("add_image"))
            dark_box = wx.CheckBox(dialog, label=self.t("dark_background"))
            dark_box.SetName(self.t("dark_background"))
            image_label = wx.StaticText(dialog, label=self.t("image_path"))
            image_ctrl = wx.TextCtrl(dialog)
            image_ctrl.SetName(self.t("image_path"))
            image_button = wx.Button(dialog, label=self.t("choose_image"))
            image_button.SetName(self.t("choose_image"))
            image_row = wx.BoxSizer(wx.HORIZONTAL)
            image_row.Add(image_ctrl, 1, wx.EXPAND | wx.RIGHT, 4)
            image_row.Add(image_button, 0)
            form.Add(options_label, 0, wx.ALIGN_CENTER_VERTICAL)
            form.Add(add_image_box, 1, wx.EXPAND)
            form.AddSpacer(1)
            form.Add(dark_box, 1, wx.EXPAND)
            form.Add(image_label, 0, wx.ALIGN_CENTER_VERTICAL)
            form.Add(image_row, 1, wx.EXPAND)

            output_mode_label = wx.StaticText(dialog, label=self.t("output_format"))
            create_new_box = wx.CheckBox(dialog, label=self.t("converter_create_new_folder" if folder_mode else "converter_create_new_file"))
            create_new_box.SetName(self.t("converter_create_new_folder" if folder_mode else "converter_create_new_file"))
            replace_box = wx.CheckBox(dialog, label=self.t("converter_replace_originals" if folder_mode else "converter_replace_original_file"))
            replace_box.SetName(self.t("converter_replace_originals" if folder_mode else "converter_replace_original_file"))
            create_new_box.SetValue(True)
            output_mode_row = wx.BoxSizer(wx.VERTICAL)
            output_mode_row.Add(create_new_box, 0, wx.BOTTOM, 3)
            output_mode_row.Add(replace_box, 0)
            form.Add(output_mode_label, 0, wx.ALIGN_TOP)
            form.Add(output_mode_row, 1, wx.EXPAND)

            button_row = wx.BoxSizer(wx.HORIZONTAL)
            convert_button = wx.Button(dialog, label=self.t("convert"))
            convert_button.SetName(self.t("convert"))
            cancel_button = wx.Button(dialog, wx.ID_CANCEL, self.t("back"))
            button_row.Add(convert_button, 0, wx.RIGHT, 6)
            button_row.Add(cancel_button, 0)

            main.Add(form, 1, wx.EXPAND | wx.ALL, 10)
            main.Add(button_row, 0, wx.ALIGN_RIGHT | wx.LEFT | wx.RIGHT | wx.BOTTOM, 10)
            dialog.SetSizer(main)

            def selected_target() -> str:
                selection = target_choice.GetSelection()
                if 0 <= selection < len(target_values):
                    return target_values[selection]
                return target_values[0] if target_values else "mp3"

            def should_show_audio_video_options() -> bool:
                target = selected_target()
                if target not in VIDEO_CONVERT_FORMATS:
                    return False
                path = Path(path_ctrl.GetValue().strip().strip('"'))
                if folder_mode:
                    return not path.exists() or self.folder_has_audio_inputs(path)
                return self.converter_is_audio_to_video(path, target)

            def update_audio_video_controls() -> None:
                show_options = should_show_audio_video_options()
                if show_options and not add_image_box.GetValue() and not dark_box.GetValue():
                    dark_box.SetValue(True)
                show_image = show_options and add_image_box.GetValue()
                for ctrl in (options_label, add_image_box, dark_box):
                    ctrl.Show(show_options)
                image_label.Show(show_image)
                image_ctrl.Show(show_image)
                image_button.Show(show_image)
                dialog.Layout()
                dialog.Fit()

            def update_formats(_event=None) -> None:
                nonlocal target_values
                raw_path = path_ctrl.GetValue().strip().strip('"')
                path = Path(raw_path) if raw_path else Path()
                if folder_mode:
                    input_kind = ""
                else:
                    input_kind = self.converter_input_kind(path) if raw_path else ""
                current = selected_target() if target_values else ""
                target_values = self.converter_format_values(input_kind)
                target_choice.Set(self.converter_format_labels(target_values))
                target_choice.SetSelection(target_values.index(current) if current in target_values else 0)
                update_audio_video_controls()

            def browse_path(_event=None) -> None:
                if folder_mode:
                    with wx.DirDialog(dialog, self.t("browse_folder"), style=wx.DD_DEFAULT_STYLE | wx.DD_DIR_MUST_EXIST) as chooser:
                        if chooser.ShowModal() == wx.ID_OK:
                            path_ctrl.SetValue(chooser.GetPath())
                else:
                    wildcard = self.converter_input_wildcard()
                    with wx.FileDialog(dialog, self.t("browse_file"), wildcard=wildcard, style=wx.FD_OPEN | wx.FD_FILE_MUST_EXIST) as chooser:
                        if chooser.ShowModal() == wx.ID_OK:
                            path_ctrl.SetValue(chooser.GetPath())
                update_formats()

            def browse_image(_event=None) -> None:
                with wx.FileDialog(dialog, self.t("select_image_file"), wildcard=self.converter_image_wildcard(), style=wx.FD_OPEN | wx.FD_FILE_MUST_EXIST) as chooser:
                    if chooser.ShowModal() == wx.ID_OK:
                        image_ctrl.SetValue(chooser.GetPath())

            def on_add_image(_event=None) -> None:
                if add_image_box.GetValue():
                    dark_box.SetValue(False)
                elif not dark_box.GetValue():
                    dark_box.SetValue(True)
                update_audio_video_controls()

            def on_dark(_event=None) -> None:
                if dark_box.GetValue():
                    add_image_box.SetValue(False)
                elif not add_image_box.GetValue():
                    add_image_box.SetValue(True)
                update_audio_video_controls()

            def on_create_new(_event=None) -> None:
                if create_new_box.GetValue():
                    replace_box.SetValue(False)
                elif not replace_box.GetValue():
                    create_new_box.SetValue(True)

            def on_replace(_event=None) -> None:
                if replace_box.GetValue():
                    create_new_box.SetValue(False)
                elif not create_new_box.GetValue():
                    replace_box.SetValue(True)

            def convert(_event=None) -> None:
                raw_path = path_ctrl.GetValue().strip().strip('"')
                source = Path(raw_path).expanduser()
                if not raw_path or not source.exists():
                    self.message(self.t("no_selection"), wx.ICON_WARNING)
                    return
                target = selected_target()
                use_image = bool(add_image_box.IsShown() and add_image_box.GetValue())
                image_path = Path(image_ctrl.GetValue().strip().strip('"')).expanduser() if use_image else None
                if use_image and (not image_path or not image_path.exists()):
                    self.message(self.t("select_image_file"), wx.ICON_WARNING)
                    return
                if folder_mode:
                    if replace_box.GetValue():
                        output_folder = source
                        replace_originals = True
                    else:
                        with wx.DirDialog(dialog, self.t("choose_output_folder"), defaultPath=str(source), style=wx.DD_DEFAULT_STYLE | wx.DD_DIR_MUST_EXIST) as chooser:
                            if chooser.ShowModal() != wx.ID_OK:
                                self.announce_player(self.t("conversion_cancelled"))
                                return
                            chosen = Path(chooser.GetPath()).expanduser()
                        output_folder = self.unique_folder_path(chosen / f"{source.name} converted")
                        replace_originals = False
                    self.start_folder_conversion(source, output_folder, target, image_path, replace_originals=replace_originals)
                else:
                    if not self.converter_input_kind(source):
                        self.message(self.t("unsupported_input_format"), wx.ICON_WARNING)
                        return
                    if replace_box.GetValue():
                        output = source.with_suffix(f".{self.converter_output_extension(target)}")
                        self.start_file_conversion(source, output, target, image_path, replace_original=True)
                    else:
                        default_output = self.converter_default_output_path(source, target)
                        with wx.FileDialog(
                            dialog,
                            self.t("choose_output_file"),
                            defaultDir=str(default_output.parent),
                            defaultFile=default_output.name,
                            wildcard=self.converter_wildcard_for_target(target),
                            style=wx.FD_SAVE | wx.FD_OVERWRITE_PROMPT,
                        ) as chooser:
                            if chooser.ShowModal() != wx.ID_OK:
                                self.announce_player(self.t("conversion_cancelled"))
                                return
                            output = Path(chooser.GetPath()).expanduser()
                        if not output.suffix:
                            output = output.with_suffix(f".{self.converter_output_extension(target)}")
                        self.start_file_conversion(source, output, target, image_path, replace_original=False)
                dialog.EndModal(wx.ID_OK)

            browse_button.Bind(wx.EVT_BUTTON, browse_path)
            image_button.Bind(wx.EVT_BUTTON, browse_image)
            path_ctrl.Bind(wx.EVT_TEXT, update_formats)
            target_choice.Bind(wx.EVT_CHOICE, lambda evt: update_audio_video_controls())
            add_image_box.Bind(wx.EVT_CHECKBOX, on_add_image)
            dark_box.Bind(wx.EVT_CHECKBOX, on_dark)
            create_new_box.Bind(wx.EVT_CHECKBOX, on_create_new)
            replace_box.Bind(wx.EVT_CHECKBOX, on_replace)
            convert_button.Bind(wx.EVT_BUTTON, convert)
            update_formats()
            dialog.Fit()
            dialog.SetMinSize((600, -1))
            dialog.ShowModal()
        finally:
            dialog.Destroy()


    def converter_input_wildcard(self) -> str:
        audio_patterns = ";".join(f"*{extension}" for extension in sorted(AUDIO_INPUT_EXTENSIONS))
        video_patterns = ";".join(f"*{extension}" for extension in sorted(VIDEO_INPUT_EXTENSIONS))
        all_patterns = ";".join(f"*{extension}" for extension in sorted(CONVERTER_MEDIA_EXTENSIONS))
        return (
            f"{self.t('media_files')}|{all_patterns}|"
            f"{self.t('audio_files')}|{audio_patterns}|"
            f"{self.t('video_files')}|{video_patterns}|"
            f"{self.t('all_files')} (*.*)|*.*"
        )


    def converter_image_wildcard(self) -> str:
        patterns = ";".join(f"*{extension}" for extension in sorted(CONVERTER_IMAGE_EXTENSIONS))
        return f"{self.t('image_files')}|{patterns}|{self.t('all_files')} (*.*)|*.*"



    @staticmethod
    def unique_converter_output_path(path: Path, source: Path | None = None) -> Path:
        candidate = path
        counter = 2
        while candidate.exists() or (source is not None and candidate.resolve() == source.resolve()):
            candidate = path.with_name(f"{path.stem} ({counter}){path.suffix}")
            counter += 1
        return candidate



    @staticmethod
    def replace_converted_original(source: Path, work_output: Path, final_output: Path) -> None:
        if not work_output.exists():
            raise RuntimeError("Converted file was not created")
        if source.exists() and source.resolve() != final_output.resolve():
            source.unlink()
        if final_output.exists() and final_output.resolve() != work_output.resolve():
            final_output.unlink()
        if work_output.resolve() != final_output.resolve():
            shutil.move(str(work_output), str(final_output))



    def converter_audio_codec_args(self, target_format: str) -> list[str]:
        fmt = target_format.lower()
        if fmt == "mp3":
            return ["-vn", "-c:a", "libmp3lame", "-b:a", "320k"]
        if fmt in {"m4a", "aac"}:
            return ["-vn", "-c:a", "aac", "-b:a", "256k"]
        if fmt == "alac":
            return ["-vn", "-c:a", "alac"]
        if fmt == "opus":
            return ["-vn", "-c:a", "libopus", "-b:a", "160k"]
        if fmt == "ogg":
            return ["-vn", "-c:a", "libvorbis", "-q:a", "5"]
        if fmt == "wma":
            return ["-vn", "-c:a", "wmav2", "-b:a", "192k"]
        if fmt == "ac3":
            return ["-vn", "-c:a", "ac3", "-b:a", "192k"]
        if fmt == "mp2":
            return ["-vn", "-c:a", "mp2", "-b:a", "192k"]
        if fmt == "aiff":
            return ["-vn", "-c:a", "pcm_s16be"]
        if fmt == "wav":
            return ["-vn", "-c:a", "pcm_s16le"]
        if fmt == "flac":
            return ["-vn", "-c:a", "flac"]
        return ["-vn", "-c:a", "aac", "-b:a", "256k"]


    def converter_video_codec_args(self, target_format: str) -> list[str]:
        fmt = target_format.lower()
        if fmt == "webm":
            return ["-c:v", "libvpx-vp9", "-b:v", "0", "-crf", "32", "-c:a", "libopus", "-b:a", "160k"]
        if fmt == "avi":
            return ["-c:v", "mpeg4", "-q:v", "4", "-c:a", "libmp3lame", "-b:a", "192k"]
        if fmt in {"wmv", "asf"}:
            return ["-c:v", "wmv2", "-b:v", "2500k", "-c:a", "wmav2", "-b:a", "192k"]
        if fmt in {"mpg", "mpeg"}:
            return ["-c:v", "mpeg2video", "-q:v", "4", "-c:a", "mp2", "-b:a", "192k"]
        if fmt in {"ts", "m2ts"}:
            return ["-c:v", "libx264", "-preset", "veryfast", "-crf", "20", "-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a", "192k", "-f", "mpegts"]
        if fmt == "flv":
            return ["-c:v", "flv", "-q:v", "4", "-c:a", "libmp3lame", "-b:a", "192k"]
        if fmt == "ogv":
            return ["-c:v", "libtheora", "-q:v", "7", "-c:a", "libvorbis", "-q:a", "5"]
        extra = ["-movflags", "+faststart"] if fmt in {"mp4", "m4v", "mov"} else []
        return ["-c:v", "libx264", "-preset", "veryfast", "-crf", "20", "-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a", "192k", *extra]


    def converter_ffmpeg_args(self, ffmpeg: str, source: Path, output: Path, target_format: str, image_path: Path | None = None) -> list[str]:
        source_kind = self.converter_input_kind(source)
        if not source_kind:
            raise RuntimeError(self.t("unsupported_input_format"))
        target_format = target_format.lower()
        args = [ffmpeg, "-y", "-hide_banner", "-loglevel", "error"]
        if target_format in AUDIO_CONVERT_FORMATS:
            return [*args, "-i", str(source), *self.converter_audio_codec_args(target_format), str(output)]
        if target_format not in VIDEO_CONVERT_FORMATS:
            raise RuntimeError(self.t("unsupported_input_format"))
        if source_kind == "audio":
            if image_path:
                args.extend(["-loop", "1", "-framerate", "1", "-i", str(image_path), "-i", str(source)])
                args.extend(["-shortest", "-map", "0:v:0", "-map", "1:a:0", "-vf", "scale=1280:720:force_original_aspect_ratio=decrease,pad=1280:720:(ow-iw)/2:(oh-ih)/2,fps=30"])
            else:
                args.extend(["-f", "lavfi", "-i", "color=c=black:s=1280x720:r=30", "-i", str(source)])
                args.extend(["-shortest", "-map", "0:v:0", "-map", "1:a:0"])
            args.extend(self.converter_video_codec_args(target_format))
            args.append(str(output))
            return args
        return [*args, "-i", str(source), *self.converter_video_codec_args(target_format), str(output)]



    def parse_inline_podcast_chapters(self, element: ET.Element) -> list[dict]:
        raw_chapters: list[dict] = []
        for chapters_element in self.children(element, "chapters"):
            chapter_children = self.children(chapters_element, "chapter")
            if not chapter_children:
                continue
            for chapter in chapter_children:
                raw_chapters.append(
                    {
                        "start": chapter.get("start") or chapter.get("time") or self.child_text(chapter, "start"),
                        "end": chapter.get("end") or self.child_text(chapter, "end"),
                        "title": chapter.get("title") or self.child_text(chapter, "title") or (chapter.text or ""),
                    }
                )
        return self.normalized_chapters(raw_chapters)


    def podcast_chapters_reference(self, element: ET.Element, base_url: str) -> tuple[str, str]:
        for child in self.children(element, "chapters"):
            url = str(child.get("url") or child.get("href") or "").strip()
            if url:
                return self.absolute_url(url, base_url), str(child.get("type") or "").strip()
        return "", ""



    @staticmethod
    def parse_chapter_seconds(value) -> float | None:
        if value is None or value == "":
            return None
        if isinstance(value, bool):
            return None
        if isinstance(value, (int, float)):
            return max(0.0, float(value))
        text = str(value).strip().replace(",", ".")
        if not text:
            return None
        if re.fullmatch(r"\d+(?:\.\d+)?", text):
            return max(0.0, float(text))
        parts = text.split(":")
        if 1 < len(parts) <= 3:
            try:
                total = 0.0
                for part in parts:
                    total = total * 60.0 + float(part)
                return max(0.0, total)
            except ValueError:
                return None
        return None


    def normalized_chapters(self, raw_chapters) -> list[dict]:
        chapters: list[dict] = []
        if not isinstance(raw_chapters, list):
            return chapters
        for index, chapter in enumerate(raw_chapters):
            if not isinstance(chapter, dict):
                continue
            start = chapter.get("start_time", chapter.get("time", chapter.get("start", chapter.get("startTime"))))
            end = chapter.get("end_time", chapter.get("end", chapter.get("endTime")))
            start_value = self.parse_chapter_seconds(start)
            if start_value is None:
                continue
            end_value = self.parse_chapter_seconds(end)
            title = str(chapter.get("title") or chapter.get("name") or self.t("chapters")).strip()
            if not title:
                title = f"{self.t('chapters')} {index + 1}"
            normalized = {
                "title": title,
                "start_time": round(start_value, 3),
            }
            if end_value is not None and end_value > start_value:
                normalized["end_time"] = round(end_value, 3)
            chapters.append(normalized)
        return sorted(chapters, key=lambda item: float(item.get("start_time") or 0.0))


    def current_chapters(self) -> list[dict]:
        chapters = self.normalized_chapters((self.current_video_info or {}).get("chapters"))
        if chapters:
            return chapters
        chapters = self.current_podcast_chapters()
        if chapters:
            return chapters
        if self.player_kind == "mpv" and self.mpv_process_alive():
            try:
                chapters = self.normalized_chapters(self.mpv_get_property("chapter-list", timeout=0.5))
            except Exception:
                chapters = []
        if chapters:
            if not isinstance(self.current_video_info, dict):
                self.current_video_info = {}
            self.current_video_info["chapters"] = chapters
            if self.current_video_item is not None:
                self.current_video_item["chapters"] = chapters
        return chapters


    def current_podcast_chapters(self) -> list[dict]:
        item = self.current_video_info or self.current_video_item or {}
        if not isinstance(item, dict):
            return []
        chapters_url = str(item.get("chapters_url") or "").strip()
        if not chapters_url or bool(item.get("_chapters_url_checked")):
            return []
        try:
            chapters = self.fetch_podcast_chapters(chapters_url)
        except Exception:
            chapters = []
        self.cache_current_podcast_chapters(chapters, checked=True)
        return chapters


    def fetch_podcast_chapters(self, url: str) -> list[dict]:
        request = Request(url, headers={"User-Agent": f"{APP_NAME}/{APP_VERSION}"})
        with self.open_url(request, timeout=20) as response:
            payload = json.loads(response.read(1_000_000).decode("utf-8", errors="replace"))
        if isinstance(payload, dict):
            raw = payload.get("chapters") or payload.get("items") or []
        else:
            raw = payload
        return self.normalized_chapters(raw)


    def cache_current_podcast_chapters(self, chapters: list[dict], checked: bool = False) -> None:
        if not isinstance(self.current_video_info, dict):
            self.current_video_info = {}
        if chapters:
            self.current_video_info["chapters"] = chapters
        if checked:
            self.current_video_info["_chapters_url_checked"] = True
        if self.current_video_item is not None:
            if chapters:
                self.current_video_item["chapters"] = chapters
            if checked:
                self.current_video_item["_chapters_url_checked"] = True


    def chapter_line(self, chapter: dict, index: int) -> str:
        title = str(chapter.get("title") or f"{self.t('chapters')} {index + 1}")
        start = self.format_seconds(float(chapter.get("start_time") or 0.0))
        end = chapter.get("end_time")
        if end is not None:
            return f"{index + 1}. {start} - {self.format_seconds(float(end))}. {title}"
        return f"{index + 1}. {start}. {title}"



    def current_chapter_index(self, chapters: list[dict] | None = None) -> int:
        chapters = chapters or self.current_chapters()
        if not chapters:
            return -1
        try:
            position = float(self.mpv_get_property("time-pos", timeout=0.35) or 0.0)
        except Exception:
            position = 0.0
        selected = 0
        for index, chapter in enumerate(chapters):
            if float(chapter.get("start_time") or 0.0) <= position + 0.1:
                selected = index
            else:
                break
        return selected



    def current_transcript_entries(self) -> list[dict]:
        for source in (self.current_video_info, self.current_video_item):
            if not isinstance(source, dict):
                continue
            entries = source.get("transcript_entries")
            if isinstance(entries, list) and entries:
                return [entry for entry in entries if isinstance(entry, dict)]
        return []


    def cache_current_transcript_entries(self, entries: list[dict], checked: bool = False, source_key: str = "") -> None:
        if not isinstance(self.current_video_info, dict):
            self.current_video_info = {}
        for target in (self.current_video_info, self.current_video_item):
            if not isinstance(target, dict):
                continue
            if entries:
                target["transcript_entries"] = entries
            if checked:
                target["_transcript_checked"] = True
            if source_key:
                target["transcript_source_key"] = source_key


    def local_transcript_entries(self, item: dict | None = None) -> list[dict]:
        item = item if isinstance(item, dict) else self.current_player_item()
        path = self.local_media_path_from_input(str(item.get("path") or item.get("url") or item.get("webpage_url") or ""))
        if not path:
            return []
        languages = self.transcript_language_candidates()
        candidates = [
            path.with_suffix(".vtt"),
            path.with_suffix(".srt"),
            path.with_name(f"{path.stem}.captions.vtt"),
            path.with_name(f"{path.stem}.captions.srt"),
            path.with_name(f"{path.stem}.transcript.vtt"),
            path.with_name(f"{path.stem}.transcript.srt"),
        ]
        for language in languages:
            safe_language = re.sub(r"[^A-Za-z0-9_-]+", "", language)
            if not safe_language:
                continue
            candidates.extend([
                path.with_name(f"{path.stem}.{safe_language}.vtt"),
                path.with_name(f"{path.stem}.{safe_language}.srt"),
            ])
        seen: set[Path] = set()
        for candidate in candidates:
            if candidate in seen:
                continue
            seen.add(candidate)
            try:
                if candidate.exists() and candidate.is_file() and candidate.stat().st_size <= 5_000_000:
                    text = candidate.read_text(encoding="utf-8", errors="replace")
                    entries = self.parse_transcript_text(text, candidate.suffix.lower())
                    if entries:
                        return entries
            except OSError:
                continue
        return []


    def transcript_language_candidates(self) -> list[str]:
        languages = self.parse_csv(str(getattr(self.settings, "subtitle_languages", "") or ""))
        languages.extend(["en", "sl"])
        result: list[str] = []
        seen: set[str] = set()
        for language in languages:
            value = str(language or "").strip()
            if not value:
                continue
            lowered = value.lower()
            if lowered in seen:
                continue
            seen.add(lowered)
            result.append(value)
        return result


    @staticmethod
    def transcript_track_matches_language(track_language: str, requested_language: str) -> bool:
        available = str(track_language or "").lower()
        requested = str(requested_language or "").lower()
        if not available or not requested:
            return False
        return available == requested or available.startswith(f"{requested}-") or requested.startswith(f"{available}-")


    def transcript_source_url(self, item: dict | None = None) -> str:
        item = item if isinstance(item, dict) else self.current_player_item()
        if self.item_is_local_media(item):
            return ""
        for key in ("webpage_url", "original_url", "watch_url", "url"):
            candidate = str((item or {}).get(key) or "").strip()
            if not candidate:
                continue
            try:
                parsed = urlparse(candidate)
                host = (parsed.netloc or "").lower()
            except Exception:
                continue
            if not parsed.scheme.startswith("http") or "googlevideo.com" in host:
                continue
            return candidate
        return ""


    def select_transcript_track(self, info: dict) -> tuple[dict | None, str]:
        if not isinstance(info, dict):
            return None, ""
        candidates = self.transcript_language_candidates()
        groups = [
            (info.get("requested_subtitles"), "transcript_source_subtitles"),
            (info.get("subtitles"), "transcript_source_subtitles"),
            (info.get("automatic_captions"), "transcript_source_auto_captions"),
        ]
        for group, source_key in groups:
            track = self.select_transcript_track_from_group(group, candidates)
            if track:
                return track, source_key
        return None, ""


    def select_transcript_track_from_group(self, group, candidates: list[str]) -> dict | None:
        if not isinstance(group, dict):
            return None
        ordered_languages: list[str] = []
        for requested in candidates:
            for language in group:
                if language not in ordered_languages and self.transcript_track_matches_language(language, requested):
                    ordered_languages.append(language)
        for language in group:
            if language not in ordered_languages:
                ordered_languages.append(language)
        for language in ordered_languages:
            tracks = group.get(language)
            if isinstance(tracks, dict):
                tracks = [tracks]
            if not isinstance(tracks, list):
                continue
            normalized = [track for track in tracks if isinstance(track, dict) and track.get("url")]
            for extension in ("vtt", "srt"):
                for track in normalized:
                    if str(track.get("ext") or "").lower() == extension:
                        return track
            if normalized:
                return normalized[0]
        return None


    def fetch_transcript_worker(self, item: dict, callback) -> None:
        entries: list[dict] = []
        source_key = ""
        error = ""
        try:
            entries, source_key = self.fetch_transcript_entries(item)
        except Exception as exc:
            error = self.friendly_error(exc)
        wx.CallAfter(callback, entries, source_key, error)


    def fetch_transcript_entries(self, item: dict | None = None) -> tuple[list[dict], str]:
        item = item if isinstance(item, dict) else self.current_player_item()
        cached = self.current_transcript_entries()
        if cached:
            return cached, str((self.current_video_info or {}).get("transcript_source_key") or "transcript_source_subtitles")
        local_entries = self.local_transcript_entries(item)
        if local_entries:
            self.cache_current_transcript_entries(local_entries, checked=True, source_key="transcript_source_local")
            return local_entries, "transcript_source_local"
        if bool((self.current_video_info or {}).get("_transcript_checked")) or bool((self.current_video_item or {}).get("_transcript_checked")):
            return [], ""
        url = self.transcript_source_url(item)
        if not url:
            self.cache_current_transcript_entries([], checked=True)
            return [], ""
        options = {
            "quiet": True,
            "skip_download": True,
            "noplaylist": True,
            "writesubtitles": True,
            "writeautomaticsub": True,
            "subtitleslangs": self.transcript_language_candidates(),
            "subtitlesformat": "vtt/srt/best",
            "ignore_no_formats_error": True,
        }
        info = self.ydl_extract_info(url, options=options, download=False, allow_cookie_retry=False)
        track, source_key = self.select_transcript_track(info)
        if not track:
            self.cache_current_transcript_entries([], checked=True)
            return [], ""
        text = self.fetch_transcript_text_from_url(str(track.get("url") or ""))
        entries = self.parse_transcript_text(text, str(track.get("ext") or ""))
        self.cache_current_transcript_entries(entries, checked=True, source_key=source_key if entries else "")
        return entries, source_key if entries else ""


    def fetch_transcript_text_from_url(self, url: str) -> str:
        if not url:
            return ""
        request = Request(url, headers={"User-Agent": f"{APP_NAME}/{APP_VERSION}"})
        with self.open_url(request, timeout=20) as response:
            return response.read(5_000_000).decode("utf-8", errors="replace")


    def parse_transcript_text(self, text: str, source: str = "") -> list[dict]:
        lines = str(text or "").replace("\ufeff", "").replace("\r\n", "\n").replace("\r", "\n").split("\n")
        entries: list[dict] = []
        index = 0
        while index < len(lines):
            line = lines[index].strip()
            upper = line.upper()
            if not line or upper == "WEBVTT" or upper.startswith("KIND:") or upper.startswith("LANGUAGE:"):
                index += 1
                continue
            if upper.startswith(("NOTE", "STYLE", "REGION")):
                index += 1
                while index < len(lines) and lines[index].strip():
                    index += 1
                continue
            if "-->" not in line and index + 1 < len(lines) and "-->" in lines[index + 1]:
                index += 1
                line = lines[index].strip()
            if "-->" not in line:
                index += 1
                continue
            start, end = self.parse_transcript_timing(line)
            index += 1
            text_lines: list[str] = []
            while index < len(lines) and lines[index].strip():
                cue_line = lines[index].strip()
                if cue_line and not cue_line.isdigit():
                    text_lines.append(cue_line)
                index += 1
            body = self.clean_transcript_line(" ".join(text_lines))
            if body and start is not None:
                entry = {"start": round(start, 3), "text": body}
                if end is not None and end > start:
                    entry["end"] = round(end, 3)
                if not entries or entries[-1].get("text") != body or abs(float(entries[-1].get("start") or 0.0) - start) > 0.2:
                    entries.append(entry)
        return entries


    def parse_transcript_timing(self, line: str) -> tuple[float | None, float | None]:
        left, _sep, right = str(line or "").partition("-->")
        start = self.parse_chapter_seconds(left.strip())
        end_token = str(right or "").strip().split(" ")[0] if right else ""
        end = self.parse_chapter_seconds(end_token.strip())
        return start, end


    @staticmethod
    def clean_transcript_line(text: str) -> str:
        cleaned = re.sub(r"<\d{1,2}:\d{2}(?::\d{2})?[.,]\d+>", " ", str(text or ""))
        cleaned = re.sub(r"<[^>]+>", " ", cleaned)
        try:
            cleaned = import_module("html").unescape(cleaned)
        except Exception:
            pass
        cleaned = cleaned.replace("\xa0", " ")
        return re.sub(r"\s+", " ", cleaned).strip()


    def transcript_line(self, entry: dict, index: int) -> str:
        start = self.format_seconds(float(entry.get("start") or 0.0))
        end = entry.get("end")
        text = str(entry.get("text") or "").strip()
        if end is not None:
            return f"{index + 1}. {start} - {self.format_seconds(float(end))}. {text}"
        return f"{index + 1}. {start}. {text}"


    def transcript_full_text(self, entries: list[dict]) -> str:
        return "\n".join(self.transcript_line(entry, index) for index, entry in enumerate(entries))


    def local_lyrics_text(self) -> str:
        item = self.current_video_item or self.current_video_info or {}
        path = self.local_media_path_from_input(str(item.get("path") or item.get("url") or item.get("webpage_url") or ""))
        if not path:
            return ""
        candidates = [
            path.with_suffix(".lrc"),
            path.with_suffix(".txt"),
            path.with_name(f"{path.stem}.lyrics.txt"),
        ]
        for candidate in candidates:
            try:
                if candidate.exists() and candidate.is_file() and candidate.stat().st_size <= 512_000:
                    return candidate.read_text(encoding="utf-8", errors="replace").strip()
            except OSError:
                continue
        return ""


    def lyrics_search_terms(self) -> tuple[str, str, str, int]:
        info = self.current_video_info or self.current_video_item or {}
        title = str(info.get("track") or info.get("title") or "").strip()
        artist = str(info.get("artist") or info.get("creator") or "").strip()
        album = str(info.get("album") or "").strip()
        if not artist and " - " in title:
            left, right = title.split(" - ", 1)
            artist = left.strip()
            title = right.strip()
        title = re.sub(r"\s*[\(\[]\s*(official\s+)?(music\s+video|video|lyrics?|lyric\s+video|audio|visualizer|remaster(?:ed)?)\s*[\)\]]\s*", " ", title, flags=re.IGNORECASE)
        title = re.sub(r"\s+", " ", title).strip(" -")
        if not artist:
            artist = str(info.get("channel") or "").strip()
        duration = self.to_int(str(info.get("duration_seconds") or 0), 0, 0)
        return artist, title, album, duration


    def fetch_lyrics_worker(self, search_terms: tuple[str, str, str, int], callback) -> None:
        text = ""
        try:
            text = self.fetch_online_lyrics(search_terms)
        except Exception:
            text = ""
        wx.CallAfter(callback, text, self.t("lyrics_source_online") if text else "")


    def fetch_online_lyrics(self, search_terms: tuple[str, str, str, int] | None = None) -> str:
        artist, title, album, duration = search_terms or self.lyrics_search_terms()
        if not title:
            return ""
        params = {"track_name": title}
        if artist:
            params["artist_name"] = artist
        if album:
            params["album_name"] = album
        if duration:
            params["duration"] = str(duration)
        request = Request(f"{LRCLIB_API_GET_URL}?{urlencode(params)}", headers={"User-Agent": f"{APP_NAME}/{APP_VERSION}"})
        with self.open_url(request, timeout=20) as response:
            payload = json.loads(response.read().decode("utf-8", errors="replace"))
        if not isinstance(payload, dict):
            return ""
        return str(payload.get("syncedLyrics") or payload.get("plainLyrics") or "").strip()


    def normalize_comment_snippet(self, snippet: dict) -> dict:
        text = self.strip_html(str(snippet.get("textOriginal") or snippet.get("textDisplay") or ""))
        try:
            text = import_module("html").unescape(text)
        except Exception:
            pass
        return {
            "author": str(snippet.get("authorDisplayName") or "").strip(),
            "text": text.strip(),
            "published": str(snippet.get("publishedAt") or "").strip(),
            "updated": str(snippet.get("updatedAt") or "").strip(),
            "likes": snippet.get("likeCount", 0),
        }

    def comment_line(self, comment: dict, index: int) -> str:
        text = " ".join(str(comment.get("text") or "").split())
        if len(text) > 140:
            text = text[:137].rstrip() + "..."
        author = str(comment.get("author") or self.t("comments"))
        likes = self.format_count(comment.get("likes"))
        replies = self.to_int(str(comment.get("reply_count") or 0), 0, 0)
        parts = [f"{index + 1}. {author}", text]
        if likes:
            parts.append(self.t("comment_likes", count=likes))
        if replies:
            parts.append(self.t("comment_replies_count", count=replies))
        return " | ".join(part for part in parts if part)



    def comment_details_text(self, comment: dict) -> str:
        lines = [
            str(comment.get("author") or ""),
            str(comment.get("published") or ""),
            self.t("comment_likes", count=self.format_count(comment.get("likes"))) if comment.get("likes") not in (None, "") else "",
            "",
            str(comment.get("text") or ""),
        ]
        replies = list(comment.get("replies") or [])
        if replies:
            lines.extend(["", self.t("comment_replies")])
            for reply in replies:
                lines.extend(["", str(reply.get("author") or ""), str(reply.get("text") or "")])
        reply_count = self.to_int(str(comment.get("reply_count") or 0), 0, 0)
        if reply_count and reply_count > len(replies):
            lines.extend(["", self.t("comment_more_replies", count=reply_count - len(replies))])
        return "\n".join(line for line in lines if line is not None)


