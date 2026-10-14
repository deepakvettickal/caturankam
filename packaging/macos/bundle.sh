#!/bin/bash
# Builds Caturankam.app from a release binary, on macOS.
#
#   packaging/macos/bundle.sh <binary> <output.app> <version>
#
# Set CODESIGN_IDENTITY to sign with a Developer ID (hardened runtime, ready
# for notarization). Without it the app is ad-hoc signed: Apple Silicon won't
# launch a completely unsigned app.
#
# Structure after spotifast's packaging/macos/bundle.sh (MIT).
set -euo pipefail

binary="$1"
app="$2"
version="$3"
here="$(cd "$(dirname "$0")" && pwd)"

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$binary" "$app/Contents/MacOS/Caturankam"
chmod 755 "$app/Contents/MacOS/Caturankam"

# CFBundleVersion must be numeric: a pre-release suffix (-rc1) comes off.
build="${version%%-*}"
sed -e "s/__VERSION__/$version/g" -e "s/__BUILD__/$build/g" \
    "$here/Info.plist" > "$app/Contents/Info.plist"
plutil -lint "$app/Contents/Info.plist" >/dev/null

# The .icns is generated from the committed 1024 px PNG; iconutil only reads
# these base sizes, each with an @2x.
iconset="$(mktemp -d)/caturankam.iconset"
mkdir -p "$iconset"
for size in 16 32 128 256 512; do
    sips -z $size $size "$here/icon-1024.png" --out "$iconset/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z $double $double "$here/icon-1024.png" --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/caturankam.icns"

if [ -n "${CODESIGN_IDENTITY:-}" ]; then
    codesign --force --timestamp --options runtime --sign "$CODESIGN_IDENTITY" "$app"
else
    codesign --force --sign - "$app"
fi
codesign --verify --strict "$app"
echo "$app"
