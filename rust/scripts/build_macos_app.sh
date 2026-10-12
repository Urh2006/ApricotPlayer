#!/bin/sh
# Builds "ApricotPlayer 2 Beta.app" for Apple silicon with everything it
# needs inside (libmpv and mpv, FFmpeg, yt-dlp, Node.js, the YouTube helper
# and the sound asset), signs it ad hoc and packs it into a DMG and a zip
# for the updater, in rust/local-dist/macos/<version>. It publishes nothing.
#
# Needs Homebrew's mpv, ffmpeg and dylibbundler; yt-dlp and Node.js are
# downloaded once into rust/.cargo-local/macos and checked by SHA-256.
# Usage: rust/scripts/build_macos_app.sh [--skip-checks]
set -eu

RUST_ROOT=$(cd "$(dirname "$0")/.." && pwd)
REPO_ROOT=$(dirname "$RUST_ROOT")
cd "$RUST_ROOT"

PRODUCT="ApricotPlayer 2 Beta"
EXECUTABLE="ApricotPlayer2Beta"
BUNDLE_ID="com.apricotplayer.ApricotPlayer2Beta"
YTDLP_VERSION="2026.08.19"
YTDLP_SHA256="0f192b7ec147ab6288885d6351d9ab67367640029b4377576ef46dd79cf7b202"
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

if [ "${1:-}" != "--skip-checks" ]; then
    cargo test --workspace --quiet
    cargo clippy --workspace --all-targets --quiet -- -D warnings
fi
cargo build --release -p apricot-player -p apricot-youtube-helper

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
fetch "https://github.com/yt-dlp/yt-dlp/releases/download/$YTDLP_VERSION/yt-dlp_macos" \
    "yt-dlp_macos-$YTDLP_VERSION" "$YTDLP_SHA256"
NODE_ARCHIVE="node-$NODE_VERSION-darwin-arm64.tar.gz"
fetch "https://nodejs.org/dist/$NODE_VERSION/$NODE_ARCHIVE" "$NODE_ARCHIVE" "$NODE_SHA256"

OUTPUT="$RUST_ROOT/local-dist/macos/$VERSION"
STAGING="$OUTPUT/staging"
APP="$STAGING/$PRODUCT.app"
rm -rf "$OUTPUT"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
MACOS="$APP/Contents/MacOS"

cp target/release/apricot-player "$MACOS/$EXECUTABLE"
mkdir -p "$MACOS/components" "$MACOS/mpv" "$MACOS/ffmpeg" "$MACOS/node" \
    "$APP/Contents/Resources/assets"
cp "$CACHE/yt-dlp_macos-$YTDLP_VERSION" "$MACOS/components/yt-dlp"
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
  "update_channel": "beta",
  "app_data_directory": "~/Library/Application Support/ApricotPlayer2Beta",
  "bundled_components": {
    "mpv": "mpv/mpv",
    "libmpv": "mpv/libmpv.2.dylib",
    "rusty_ytdl_helper": "components/apricot-youtube-helper",
    "yt_dlp": "components/yt-dlp",
    "yt_dlp_version": "$YTDLP_VERSION",
    "ffmpeg": "ffmpeg/ffmpeg",
    "node": "node/node",
    "node_version": "$NODE_VERSION"
  }
}
EOF

# Ad hoc signature: the bundle, every library and helper inside it.
find "$MACOS" -type f \( -name "*.dylib" -o -perm -u+x \) ! -path "$MACOS/$EXECUTABLE" \
    -exec codesign --force --sign - {} \; 2>/dev/null
codesign --force --sign - "$APP"
codesign --verify --deep --strict "$APP"

# The packaged application must start its smoke check and find its tools.
"$MACOS/$EXECUTABLE" --qualification-smoke
"$MACOS/$EXECUTABLE" --qualification-ytdlp
"$MACOS/ffmpeg/ffmpeg" -hide_banner -version >/dev/null
"$MACOS/mpv/mpv" --no-config --version >/dev/null

ditto -c -k --keepParent "$APP" "$OUTPUT/ApricotPlayer2Beta-macos-arm64.zip"
ln -s /Applications "$STAGING/Applications"
hdiutil create -quiet -volname "$PRODUCT" -srcfolder "$STAGING" -ov -format UDZO \
    "$OUTPUT/ApricotPlayer2Beta-macos-arm64.dmg"
rm "$STAGING/Applications"
mv "$APP" "$OUTPUT/"
rmdir "$STAGING"
(cd "$OUTPUT" && shasum -a 256 ApricotPlayer2Beta-macos-arm64.dmg ApricotPlayer2Beta-macos-arm64.zip > SHA256SUMS.txt)
echo "MACOS_APP_READY=$OUTPUT"
