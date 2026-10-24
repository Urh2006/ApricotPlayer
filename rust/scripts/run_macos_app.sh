#!/bin/sh
# Builds and starts the macOS edition of ApricotPlayer 2 Beta.
# Usage: rust/scripts/run_macos_app.sh [--first-run]
# --first-run starts with an empty temporary profile, so the one-time
# language choice appears; the real profile stays untouched.
set -eu
cd "$(dirname "$0")/.."
cargo build -p apricot-player
if [ "${1:-}" = "--first-run" ]; then
    HOME="$(mktemp -d)"
    export HOME
fi
exec ./target/debug/apricot-player
