#!/usr/bin/env bash
# Regenerates all raster app icons (PNG, ICNS, ICO) from ui/icons/app_icon.svg.
# Requires: rsvg-convert (brew install librsvg), iconutil (macOS). ICO needs Python 3 + Pillow (optional).
set -euo pipefail
cd "$(dirname "$0")/.."

SVG=ui/icons/app_icon.svg
OUT=ui/icons
TMP=$(mktemp -d)
ICONSET="$TMP/AppIcon.iconset"
mkdir -p "$ICONSET"

render() { rsvg-convert -w "$1" -h "$1" "$SVG" -o "$2"; }

# Main PNG (used by the macOS Dock icon at runtime and the web page)
render 512 "$OUT/app_icon.png"
cp "$OUT/app_icon.png" web/icons/app_icon.png 2>/dev/null || true
cp "$SVG" web/icons/app_icon.svg 2>/dev/null || true

# ICNS
for s in 16 32 128 256 512; do
  render "$s" "$ICONSET/icon_${s}x${s}.png"
  render $((s * 2)) "$ICONSET/icon_${s}x${s}@2x.png"
done
iconutil -c icns "$ICONSET" -o "$OUT/AppIcon.icns"

# ICO (Windows)
if python3 -c "import PIL" 2>/dev/null; then
  render 256 "$TMP/ico.png"
  python3 - "$TMP/ico.png" "$OUT/app_icon.ico" <<'PY'
import sys
from PIL import Image
Image.open(sys.argv[1]).save(sys.argv[2], sizes=[(16,16),(24,24),(32,32),(48,48),(64,64),(128,128),(256,256)])
PY
else
  echo "Pillow not found; skipped app_icon.ico"
fi

rm -rf "$TMP"
echo "Icons regenerated from $SVG"
