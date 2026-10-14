#!/bin/bash
# Packs Caturankam.app into a compressed DMG with an Applications shortcut,
# so installing is a drag. Notarizes and staples it when the Apple
# credentials are set (see .github/workflows/release.yml).
#
#   packaging/macos/dmg.sh <Caturankam.app> <output.dmg>
set -euo pipefail

app="$1"
dmg="$2"
stage="$(mktemp -d)/Caturankam"
mkdir -p "$stage"
cp -R "$app" "$stage/"
ln -s /Applications "$stage/Applications"
rm -f "$dmg"
hdiutil create -volname Caturankam -srcfolder "$stage" -ov -format UDZO -quiet "$dmg"

if [ -n "${CODESIGN_IDENTITY:-}" ]; then
    codesign --force --timestamp --sign "$CODESIGN_IDENTITY" "$dmg"
fi
if [ -n "${APPLE_ID:-}" ] && [ -n "${APPLE_TEAM_ID:-}" ] && [ -n "${APPLE_APP_PASSWORD:-}" ]; then
    xcrun notarytool submit "$dmg" --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" \
        --password "$APPLE_APP_PASSWORD" --wait
    xcrun stapler staple "$dmg"
fi
hdiutil verify -quiet "$dmg"
echo "$dmg"
