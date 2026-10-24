#!/bin/sh
# Wraps target/release/snap-notes in "Snap Notes.app" and puts it in a .dmg
# with an Applications link, so it installs by drag and drop.
# Usage: packaging/macos/bundle.sh <output.dmg>   (run from the repo root)
#
# With MACOS_SIGN_IDENTITY set (a "Developer ID Application" identity in the
# keychain), the app is signed with the hardened runtime. With APPLE_ID,
# APPLE_TEAM_ID and APPLE_APP_PASSWORD (an app-specific password) also set,
# the .dmg is notarized and stapled, so Gatekeeper opens it without warning.
# Without them the app gets an ad-hoc signature.
set -eu

out=${1:?usage: bundle.sh <output.dmg>}
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
work=$(mktemp -d)
app="$work/dmg/Snap Notes.app"

mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/release/snap-notes "$app/Contents/MacOS/snap-notes"
sed "s/@VERSION@/$version/g" packaging/macos/Info.plist > "$app/Contents/Info.plist"

iconset="$work/AppIcon.iconset"
mkdir "$iconset"
for size in 16 32 128 256 512; do
    sips -z $size $size assets/icon.png --out "$iconset/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z $double $double assets/icon.png --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/AppIcon.icns"

identity=${MACOS_SIGN_IDENTITY:-}
if [ -n "$identity" ]; then
    codesign --force --options runtime --timestamp --sign "$identity" "$app"
else
    # Ad-hoc signature: not notarized, but keeps the bundle intact on Apple silicon.
    codesign --force --sign - "$app"
fi

ln -s /Applications "$work/dmg/Applications"
mkdir -p "$(dirname "$out")"
rm -f "$out"
hdiutil create -volname "Snap Notes" -srcfolder "$work/dmg" -format UDZO "$out" >/dev/null
rm -rf "$work"

if [ -n "$identity" ]; then
    codesign --force --timestamp --sign "$identity" "$out"
    if [ -n "${APPLE_ID:-}" ]; then
        xcrun notarytool submit "$out" --wait \
            --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" --password "$APPLE_APP_PASSWORD"
        xcrun stapler staple "$out"
    fi
fi
echo "$out"
