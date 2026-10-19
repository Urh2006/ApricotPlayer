#!/usr/bin/env python3
"""Test reporting in an isolated workspace, without account or event submissions."""
from pathlib import Path
import os
import shutil
import subprocess
import tempfile

rust = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix="apricot-listening-tests-") as directory:
    root = Path(directory)
    for name in ("core", "playback", "protocol", "connect"):
        shutil.copytree(rust / "vendor" / f"librespot-{name}", root / name)
    shutil.copyfile(rust / "Cargo.lock", root / "Cargo.lock")
    (root / "Cargo.toml").write_text('''[workspace]
resolver = "3"
members = ["core", "playback", "protocol", "connect"]
[patch.crates-io]
librespot-core = { path = "core" }
librespot-playback = { path = "playback" }
librespot-protocol = { path = "protocol" }
librespot-connect = { path = "connect" }
''')
    command = ["cargo", "+1.98.0", "test", "--manifest-path", str(root / "Cargo.toml"),
               "-p", "librespot-core", "-p", "librespot-playback",
               "--no-default-features", "--features", "rustls-tls-native-roots", "listening"]
    if os.environ.get("APRICOT_TEST_OFFLINE") == "1":
        command.insert(3, "--offline")
    subprocess.run(command, check=True)
    transport = ["cargo", "+1.98.0", "test", "--manifest-path", str(root / "Cargo.toml"),
                 "-p", "librespot-connect", "--no-default-features", "--features", "rustls-tls-native-roots", "transport_regressions"]
    if os.environ.get("APRICOT_TEST_OFFLINE") == "1":
        transport.insert(3, "--offline")
    subprocess.run(transport, check=True)
    retries = ["cargo", "+1.98.0", "test", "--manifest-path", str(root / "Cargo.toml"),
               "-p", "librespot-playback", "--no-default-features", "--features", "rustls-tls-native-roots", "load_retry"]
    if os.environ.get("APRICOT_TEST_OFFLINE") == "1":
        retries.insert(3, "--offline")
    subprocess.run(retries, check=True)
    keys = ["cargo", "+1.98.0", "test", "--manifest-path", str(root / "Cargo.toml"),
            "-p", "librespot-core", "--no-default-features", "--features", "rustls-tls-native-roots", "audio_key"]
    if os.environ.get("APRICOT_TEST_OFFLINE") == "1":
        keys.insert(3, "--offline")
    subprocess.run(keys, check=True)
