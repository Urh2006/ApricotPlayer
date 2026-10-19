#!/bin/sh
# Builds "ApricotPlayer 2 Beta.app" for Apple silicon with everything it
# needs inside (libmpv and mpv, FFmpeg, yt-dlp, Node.js, the YouTube helper
# and the sound asset), signs it ad hoc and packs it into a DMG and a zip
# for the updater, in rust/local-dist/macos/<version>. It publishes nothing.
#
# Needs Homebrew's mpv, ffmpeg and dylibbundler; yt-dlp and Node.js are
# downloaded once into rust/.cargo-local/macos and checked by SHA-256.
# Usage: rust/scripts/build_macos_app.sh [--skip-checks] [--stable] [--channel beta]
# --stable builds "ApricotPlayer.app", the identity of the published releases
# (as build_stable_release.ps1 on Windows); --channel beta lets it install
# updates from GitHub prereleases.
set -eu

SKIP_CHECKS=false
STABLE=false
CHANNEL=local-only
while [ $# -gt 0 ]; do
    case "$1" in
        --skip-checks) SKIP_CHECKS=true ;;
        --stable) STABLE=true ;;
        --channel) shift; CHANNEL="$1" ;;
        *) echo "Unknown option $1" >&2; exit 2 ;;
    esac
    shift
done

RUST_ROOT=$(cd "$(dirname "$0")/.." && pwd)
REPO_ROOT=$(dirname "$RUST_ROOT")
cd "$RUST_ROOT"

if [ "$STABLE" = true ]; then
    PRODUCT="ApricotPlayer"
    EXECUTABLE="ApricotPlayer"
    BUNDLE_ID="com.apricotplayer.ApricotPlayer"
    ASSET="ApricotPlayer-macos-arm64"
    DMG_ASSET="ApricotPlayer-macOS-arm64"
    APP_DATA="ApricotPlayer"
else
    PRODUCT="ApricotPlayer 2 Beta"
    EXECUTABLE="ApricotPlayer2Beta"
    BUNDLE_ID="com.apricotplayer.ApricotPlayer2Beta"
    ASSET="ApricotPlayer2Beta-macos-arm64"
    DMG_ASSET="$ASSET"
    APP_DATA="ApricotPlayer2Beta"
fi
FEATURES=""
[ "$CHANNEL" = beta ] && FEATURES="apricot-player/release-beta"
[ "$STABLE" = true ] && FEATURES="${FEATURES:+$FEATURES,}apricot-player/stable-identity"
YTDLP_VERSION="2026.08.19"
# The folder build of yt-dlp: the one-file yt-dlp_macos unpacks itself on
# every start, about four seconds per search.
YTDLP_SHA256="07e54b0865303c864006925913bce2604f8ee8cc6f18699bac9c309f9328a6d8"
NODE_VERSION="v24.21.0"
NODE_SHA256="bed7eea5325e1108f32ce5228ddd6a5f0f08a499ee42aa7442aea583702f6057"
BREW_PREFIX=$(brew --prefix)

for tool in "$BREW_PREFIX/bin/mpv" "$BREW_PREFIX/lib/libmpv.2.dylib" \
    "$BREW_PREFIX/bin/ffmpeg" "$BREW_PREFIX/bin/dylibbundler"; do
    [ -e "$tool" ] || { echo "Missing $tool; run: brew install mpv ffmpeg dylibbundler" >&2; exit 1; }
done

VERSION=$(cargo metadata --no-deps --format-version 1 | python3 -c '
import json, sys
print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"] == "apricot-player"))')
COMMIT=$(git rev-parse HEAD)
if [ -n "$(git status --porcelain --untracked-files=no)" ]; then DIRTY=true; else DIRTY=false; fi
if [ "$STABLE" = true ] && [ "$DIRTY" = true ]; then
    echo "Commit the changes first; a release is built from a clean tree" >&2
    exit 1
fi

if [ "$SKIP_CHECKS" != true ]; then
    cargo test --workspace --locked --quiet
    cargo clippy --workspace --all-targets --locked --quiet -- -D warnings
fi
if [ -n "$FEATURES" ]; then
    cargo build --release --locked -p apricot-player -p apricot-youtube-helper --features "$FEATURES"
else
    cargo build --release --locked -p apricot-player -p apricot-youtube-helper
fi

# Downloads, checked by SHA-256 and kept for the next build.
CACHE="$RUST_ROOT/.cargo-local/macos"
mkdir -p "$CACHE"
fetch() { # url file sha256
    if [ ! -f "$CACHE/$2" ] || [ "$(shasum -a 256 "$CACHE/$2" | cut -d' ' -f1)" != "$3" ]; then
        curl -fL --retry 3 -o "$CACHE/$2.part" "$1"
        [ "$(shasum -a 256 "$CACHE/$2.part" | cut -d' ' -f1)" = "$3" ] || {
            echo "SHA-256 of $2 does not match" >&2; rm -f "$CACHE/$2.part"; exit 1; }
        mv "$CACHE/$2.part" "$CACHE/$2"
    fi
}
fetch "https://github.com/yt-dlp/yt-dlp/releases/download/$YTDLP_VERSION/yt-dlp_macos.zip" \
    "yt-dlp_macos-$YTDLP_VERSION.zip" "$YTDLP_SHA256"
NODE_ARCHIVE="node-$NODE_VERSION-darwin-arm64.tar.gz"
fetch "https://nodejs.org/dist/$NODE_VERSION/$NODE_ARCHIVE" "$NODE_ARCHIVE" "$NODE_SHA256"

OUTPUT="$RUST_ROOT/local-dist/macos/$VERSION"
# Finder/File Provider can restore FinderInfo on bundles under Documents while
# codesign is running. Assemble and sign outside the source/output directory.
STAGING=$(mktemp -d /private/tmp/apricot-macos.XXXXXX)
trap 'rm -rf "$STAGING"' EXIT
APP="$STAGING/$PRODUCT.app"
rm -rf "$OUTPUT"
mkdir -p "$OUTPUT"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
MACOS="$APP/Contents/MacOS"

cp target/release/apricot-player "$MACOS/$EXECUTABLE"
mkdir -p "$MACOS/components" "$MACOS/mpv" "$MACOS/ffmpeg" "$MACOS/node" \
    "$APP/Contents/Resources/assets"
# yt-dlp carries data files, so it lives in Resources (see the comment
# below); the application looks for it there.
ditto -x -k "$CACHE/yt-dlp_macos-$YTDLP_VERSION.zip" "$APP/Contents/Resources/yt-dlp_macos"
cp target/release/apricot-youtube-helper "$MACOS/components/apricot-youtube-helper"
tar -xzf "$CACHE/$NODE_ARCHIVE" -C "$STAGING" "node-$NODE_VERSION-darwin-arm64/bin/node"
mv "$STAGING/node-$NODE_VERSION-darwin-arm64/bin/node" "$MACOS/node/node"
rm -rf "$STAGING/node-$NODE_VERSION-darwin-arm64"
# Data lives in Resources: codesign accepts only code in Contents/MacOS.
cp "$REPO_ROOT/assets/default_reached.wav" "$APP/Contents/Resources/assets/"
cp "$BREW_PREFIX/bin/mpv" "$MACOS/mpv/mpv"
cp "$BREW_PREFIX/lib/libmpv.2.dylib" "$MACOS/mpv/libmpv.2.dylib"
cp "$BREW_PREFIX/bin/ffmpeg" "$MACOS/ffmpeg/ffmpeg"
chmod u+w "$MACOS/mpv/"* "$MACOS/ffmpeg/ffmpeg"
chmod +x "$MACOS/components/"* "$MACOS/node/node"

# Every Homebrew library goes into mpv/; mpv, libmpv and ffmpeg find them
# relative to themselves, so the application runs without Homebrew.
dylibbundler -of -cd -b \
    -x "$MACOS/mpv/mpv" -x "$MACOS/mpv/libmpv.2.dylib" -x "$MACOS/ffmpeg/ffmpeg" \
    -d "$MACOS/mpv/" -p "@loader_path/../mpv/" >/dev/null 2>&1
install_name_tool -id "@loader_path/../mpv/libmpv.2.dylib" "$MACOS/mpv/libmpv.2.dylib" 2>/dev/null
# dyld refuses a binary that lists the same run path twice.
for binary in "$MACOS/mpv/"* "$MACOS/ffmpeg/ffmpeg"; do
    otool -l "$binary" | awk '/LC_RPATH/ { getline; getline; print $2 }' | sort | uniq -d |
        while read -r duplicate; do
            while [ "$(otool -l "$binary" | grep -c " path $duplicate ")" -gt 1 ]; do
                install_name_tool -delete_rpath "$duplicate" "$binary" 2>/dev/null
            done
        done
done
if otool -L "$MACOS/mpv/"* "$MACOS/ffmpeg/ffmpeg" | grep -q "$BREW_PREFIX"; then
    echo "A bundled library still points into Homebrew" >&2
    exit 1
fi

sed -e "s/@VERSION@/$VERSION/g" -e "s/@EXECUTABLE@/$EXECUTABLE/g" \
    -e "s/@BUNDLE_ID@/$BUNDLE_ID/g" -e "s/@PRODUCT@/$PRODUCT/g" \
    "$RUST_ROOT/packaging/macos/Info.plist" > "$APP/Contents/Info.plist"
plutil -lint "$APP/Contents/Info.plist" >/dev/null

cat > "$APP/Contents/Resources/build-info.json" <<EOF
{
  "schema_version": 1,
  "application_id": "$BUNDLE_ID",
  "product_name": "$PRODUCT",
  "executable_name": "$EXECUTABLE",
  "version": "$VERSION",
  "commit": "$COMMIT",
  "dirty": $DIRTY,
  "built_at_utc": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "rust": "$(rustc --version)",
  "data_schema_version": 1,
  "update_channel": "$CHANNEL",
  "app_data_directory": "~/Library/Application Support/$APP_DATA",
  "bundled_components": {
    "mpv": "mpv/mpv",
    "libmpv": "mpv/libmpv.2.dylib",
    "rusty_ytdl_helper": "components/apricot-youtube-helper",
    "yt_dlp": "../Resources/yt-dlp_macos/yt-dlp_macos",
    "yt_dlp_version": "$YTDLP_VERSION",
    "ffmpeg": "ffmpeg/ffmpeg",
    "node": "node/node",
    "node_version": "$NODE_VERSION"
  }
}
EOF

# The package's OS floor must include its bundled dependencies, which may have
# been built for a newer macOS than our Rust executable or plist template.
python3 - "$APP" <<'PY'
import json
import pathlib
import plistlib
import re
import subprocess
import sys

app = pathlib.Path(sys.argv[1])
plist_path = app / "Contents/Info.plist"
plist = plistlib.loads(plist_path.read_bytes())
versions = [plist["LSMinimumSystemVersion"]]
magic = {bytes.fromhex(value) for value in
         ("feedface", "cefaedfe", "feedfacf", "cffaedfe",
          "cafebabe", "bebafeca", "cafebabf", "bfbafeca")}
for path in app.rglob("*"):
    if not path.is_file() or path.is_symlink():
        continue
    with path.open("rb") as stream:
        if stream.read(4) not in magic:
            continue
    commands = subprocess.check_output(["otool", "-l", str(path)], text=True)
    for block in re.split(r"Load command \d+", commands):
        if "cmd LC_BUILD_VERSION" in block:
            versions.extend(re.findall(r"\bminos\s+([\d.]+)", block))
        elif "cmd LC_VERSION_MIN_MACOSX" in block:
            versions.extend(re.findall(r"\bversion\s+([\d.]+)", block))
minimum = max(versions, key=lambda value: tuple(map(int, value.split("."))))
plist["LSMinimumSystemVersion"] = minimum
plist_path.write_bytes(plistlib.dumps(plist))
info_path = app / "Contents/Resources/build-info.json"
info = json.loads(info_path.read_text())
info["macos_minimum_version"] = minimum
info_path.write_text(json.dumps(info, indent=2) + "\n")
print(f"MACOS_MINIMUM_VERSION={minimum}")
PY

# Ad hoc signature: the bundle, every library and helper inside it.
# Archive-provided Finder metadata is not permitted by codesign.
xattr -cr "$APP"
find "$MACOS" -type f \( -name "*.dylib" -o -perm -u+x \) ! -path "$MACOS/$EXECUTABLE" \
    -exec codesign --force --sign - {} \; 2>/dev/null
codesign --force --sign - "$APP"
codesign --verify --deep --strict "$APP"

# The packaged application must start its smoke check and find its tools.
"$MACOS/$EXECUTABLE" --qualification-smoke
"$MACOS/$EXECUTABLE" --qualification-ytdlp
"$MACOS/ffmpeg/ffmpeg" -hide_banner -version >/dev/null
"$MACOS/mpv/mpv" --no-config --version >/dev/null

ditto -c -k --keepParent "$APP" "$OUTPUT/$ASSET.zip"
ln -s /Applications "$STAGING/Applications"
hdiutil create -quiet -volname "$PRODUCT" -srcfolder "$STAGING" -ov -format UDZO \
    "$OUTPUT/$DMG_ASSET.dmg"
rm "$STAGING/Applications"
mv "$APP" "$OUTPUT/"
rmdir "$STAGING"
(cd "$OUTPUT" && shasum -a 256 "$DMG_ASSET.dmg" "$ASSET.zip" > SHA256SUMS.txt)
# Only the two newest builds stay in local-dist/macos (AGENTS.md, Prostor na disku).
ls -1dt "$RUST_ROOT/local-dist/macos"/*/ 2>/dev/null | tail -n +3 | while read -r old; do
    rm -rf "$old"
done
echo "MACOS_APP_READY=$OUTPUT"
