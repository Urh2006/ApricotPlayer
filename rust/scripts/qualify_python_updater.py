"""Run the original 1.0.21 portable updater against a fresh Windows package.

Uses only the standard library. All test data stays in rust/local-dist and is
removed afterwards. No installed application or user profile is changed.
"""

from __future__ import annotations

import ast
import hashlib
import os
import re
import stat as stat_module
import subprocess
import sys
import tempfile
import zipfile
from importlib import import_module
from pathlib import Path


REPO = Path(__file__).resolve().parents[2]


def baseline(path: str) -> ast.Module:
    text = subprocess.check_output(
        ["git", "show", f"v1.0.21:{path}"], cwd=REPO, encoding="utf-8"
    )
    return ast.parse(text)


def legacy_updater():
    # Compile the original methods and constants without importing wxPython
    # or initializing the old application. Their bodies are unchanged.
    constants = {
        "PORTABLE_ZIP_ASSET_NAME", "LEGACY_PORTABLE_ZIP_ASSET_NAME",
        "UPDATE_ZIP_MAX_ENTRIES", "UPDATE_ZIP_MAX_UNCOMPRESSED_BYTES",
        "UPDATE_ZIP_MAX_MEMBER_BYTES", "UPDATE_ZIP_MAX_COMPRESSION_RATIO",
        "WINDOWS_RESERVED_PATH_STEMS", "UPDATE_RELAUNCH_ARG",
    }
    nodes = [n for n in baseline("apricot/constants.py").body
             if isinstance(n, ast.Assign)
             and any(isinstance(t, ast.Name) and t.id in constants for t in n.targets)]
    methods = []
    for path, name, wanted in [
        ("apricot/utils.py", "UtilsMixin", {
            "validate_zip_member_path", "validate_zip_archive", "powershell_literal",
        }),
        ("apricot/updater/updater.py", "AppUpdaterMixin", {
            "validate_update_package", "is_portable_zip_asset",
            "write_secure_update_script", "write_portable_zip_update_script",
        }),
    ]:
        cls = next(n for n in baseline(path).body if isinstance(n, ast.ClassDef) and n.name == name)
        methods.extend(n for n in cls.body if isinstance(n, ast.FunctionDef) and n.name in wanted)
    nodes.append(ast.ClassDef(name="LegacyUpdater", bases=[], keywords=[], body=methods, decorator_list=[]))
    namespace = dict(Path=Path, re=re, stat_module=stat_module, zipfile=zipfile,
                     import_module=import_module, tempfile=tempfile)
    module = ast.Module(body=[ast.ImportFrom(module="__future__", names=[ast.alias(name="annotations")], level=0), *nodes], type_ignores=[])
    exec(compile(ast.fix_missing_locations(module), "<original-Python-1.0.21-updater>", "exec"), namespace)
    return namespace["LegacyUpdater"]


def run_update(legacy, package: Path, target: Path, work: Path) -> subprocess.CompletedProcess:
    log = work / "updater.log"
    with package.open("rb") as handle:
        digest = hashlib.file_digest(handle, "sha256").hexdigest()
    script = legacy.write_portable_zip_update_script(
        str(package), str(target), str(target / "ApricotPlayer.exe"), 0, str(log),
        restart=False, expected_sha256=digest,
    )
    result = subprocess.run(
        ["powershell.exe", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(script)],
        env={**os.environ, "TEMP": str(work), "TMP": str(work)},
        capture_output=True, text=True, creationflags=subprocess.CREATE_NO_WINDOW,
    )
    if result.returncode:
        print(log.read_text(encoding="utf-8-sig"))
    return result


def main() -> None:
    package = Path(sys.argv[1]).resolve()
    legacy = legacy_updater()
    legacy.validate_update_package(package)
    legacy.validate_update_package(package.with_name("ApricotPlayerSetup.exe"))
    print("Original Python 1.0.21 package validation: PASS")
    previous_temp = tempfile.tempdir
    with tempfile.TemporaryDirectory(prefix="legacy-updater-", dir=REPO / "rust/local-dist") as name:
        work = Path(name)
        tempfile.tempdir = str(work)
        try:
            target = work / "installed"
            (target / "_internal").mkdir(parents=True)
            original = b"old Python installation"
            (target / "ApricotPlayer.exe").write_bytes(original)
            (target / "_internal/old.txt").write_text("old")
            (target / "user-data.txt").write_text("keep")
            downloaded = work / "ApricotPlayer.zip"
            downloaded.hardlink_to(package)
            assert run_update(legacy, downloaded, target, work).returncode == 0
            for flag in ["--qualification-smoke", "--qualification-youtube-helper", "--qualification-ytdlp", "--qualification-playback"]:
                result = subprocess.run([str(target / "ApricotPlayer.exe"), flag],
                                        env={**os.environ, "TEMP": str(work), "TMP": str(work)},
                                        creationflags=subprocess.CREATE_NO_WINDOW)
                assert result.returncode == 0, flag
            assert (target / "user-data.txt").read_text() == "keep"
            assert not (target / "_internal/old.txt").exists()
            print("Original Python 1.0.21 -> packaged Rust upgrade and media checks: PASS")
            # Fail after copying, so the old script must actually restore the
            # executable and _internal, not just reject a download checksum.
            before = hashlib.sha256((target / "ApricotPlayer.exe").read_bytes()).hexdigest()
            with zipfile.ZipFile(downloaded, "w", compression=zipfile.ZIP_STORED) as archive:
                archive.writestr("ApricotPlayer/ApricotPlayer.exe", b"MZ")
                archive.writestr("ApricotPlayer/_internal/padding", os.urandom(1024 * 1024))
            legacy.validate_update_package(downloaded)
            assert run_update(legacy, downloaded, target, work).returncode != 0
            assert hashlib.sha256((target / "ApricotPlayer.exe").read_bytes()).hexdigest() == before
            assert (target / "_internal/mpv/libmpv-2.dll").is_file()
            assert (target / "user-data.txt").read_text() == "keep"
            print("Original Python updater forced replacement failure and rollback: PASS")
        finally:
            tempfile.tempdir = previous_temp


if __name__ == "__main__":
    main()
