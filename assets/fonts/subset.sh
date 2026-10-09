#!/usr/bin/env bash
# Regenerates the bundled font subsets in this directory.
# Needs curl, unzip, python3 and pyftsubset (fonttools).
set -euo pipefail

INTER_URL="https://github.com/rsms/inter/releases/download/v4.1/Inter-4.1.zip"
LUCIDE_VERSION="1.52.0"
LUCIDE_URL="https://unpkg.com/lucide-static@${LUCIDE_VERSION}"
INTER_RANGES="U+0000-024F,U+0370-03FF,U+0400-04FF,U+2000-206F,U+20A0-20CF,U+2190-21FF"
ICONS="trash-2 x bold italic strikethrough code palette highlighter a-large-small link image plus sliders-horizontal check square search copy download pin bell layers settings"

out="$(cd "$(dirname "$0")" && pwd)"
work="${WORK_DIR:-$(mktemp -d)}"
mkdir -p "$work"

curl -fsSL "$INTER_URL" -o "$work/inter.zip"
unzip -qo "$work/inter.zip" -d "$work/inter"
inter_dir="$(dirname "$(find "$work/inter" -name Inter-Regular.ttf | grep -v -e variable | head -n1)")"

for style in Regular Italic SemiBold Bold BoldItalic; do
  pyftsubset "$inter_dir/Inter-$style.ttf" --unicodes="$INTER_RANGES" \
    --layout-features='*' --name-IDs='*' --output-file="$out/Inter-$style.subset.ttf"
done
cp "$(find "$work/inter" -name LICENSE.txt | head -n1)" "$out/OFL.txt"

curl -fsSL "$LUCIDE_URL/font/lucide.ttf" -o "$work/lucide.ttf"
curl -fsSL "$LUCIDE_URL/font/info.json" -o "$work/info.json"
curl -fsSL "$LUCIDE_URL/LICENSE" -o "$out/LUCIDE-LICENSE.txt"

unicodes="$(python3 - "$work/info.json" $ICONS <<'PY'
import json, sys
info = json.load(open(sys.argv[1]))
print(",".join(info[name]["encodedCode"].replace("\\", "U+").upper() for name in sys.argv[2:]))
PY
)"
pyftsubset "$work/lucide.ttf" --unicodes="$unicodes" --output-file="$out/lucide.subset.ttf"
echo "icon codepoints: $unicodes"
