#!/bin/sh
# Wraps target/release/snap-notes in "Snap Notes.app" and puts it in a .dmg
# with an Applications link, so it installs by drag and drop.
# Usage: packaging/macos/bundle.sh <output.dmg>   (run from the repo root)
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

# Ad-hoc signature: not notarized, but keeps the bundle intact on Apple silicon.
codesign --force --deep --sign - "$app"

ln -s /Applications "$work/dmg/Applications"
mkdir -p "$(dirname "$out")"
rm -f "$out"
hdiutil create -volname "Snap Notes" -srcfolder "$work/dmg" -format UDZO "$out" >/dev/null
rm -rf "$work"
echo "$out"
