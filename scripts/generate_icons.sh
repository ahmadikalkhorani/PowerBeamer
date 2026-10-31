#!/usr/bin/env bash
# Regenerates all raster app icons (PNG, ICNS, ICO) from ui/icons/app_icon.svg.
# Requires: rsvg-convert (brew install librsvg), iconutil (macOS), Python 3 + Pillow.
# Set PYTHON to use a specific Python environment.
set -euo pipefail
cd "$(dirname "$0")/.."

PYTHON="${PYTHON:-python3}"
for tool in rsvg-convert iconutil; do
  command -v "$tool" >/dev/null || { echo "Missing required tool: $tool" >&2; exit 1; }
done
if ! "$PYTHON" -c "from PIL import Image" 2>/dev/null; then
  echo "Pillow is required to generate the Windows installer icon. Install it in your Python environment or set PYTHON to an interpreter with Pillow." >&2
  exit 1
fi

SVG=ui/icons/app_icon.svg
OUT=ui/icons
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
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

# ICO (Windows): include bitmap frames for compatibility with the NSIS icon loader.
render 256 "$TMP/ico.png"
"$PYTHON" - "$TMP/ico.png" "$OUT/app_icon.ico" <<'PYICON'
import sys
from PIL import Image
Image.open(sys.argv[1]).convert("RGBA").save(
    sys.argv[2], format="ICO", bitmap_format="bmp",
    sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)],
)
PYICON

echo "Icons regenerated from $SVG"
