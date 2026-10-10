#!/bin/sh
# Builds and starts the macOS accessibility spike with a short test video.
# Usage: rust/scripts/run_macos_spike.sh
set -eu
cd "$(dirname "$0")/.."
video="${TMPDIR:-/tmp}/apricot-spike-test.mp4"
if [ ! -f "$video" ]; then
    ffmpeg -hide_banner -loglevel error -y \
        -f lavfi -i testsrc2=size=640x360:rate=30:duration=60 \
        -f lavfi -i sine=frequency=440:duration=60 \
        -c:v libx264 -pix_fmt yuv420p -c:a aac -shortest "$video"
fi
cargo build --release -p apricot-macos-accessibility-spike
exec ./target/release/apricot-macos-accessibility-spike "$video"
