#!/usr/bin/env bash
set -e

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$DIR"

echo "Building PowerBeamer macOS application bundle..."

# 1. Compile native binary
cargo build

APP_DIR="target/PowerBeamer.app"
CONTENTS="$APP_DIR/Contents"
MACOS="$CONTENTS/MacOS"
RESOURCES="$CONTENTS/Resources"

# 2. Re-create bundle hierarchy
rm -rf "$APP_DIR" "target/Presenter.app"
mkdir -p "$MACOS" "$RESOURCES"

# 3. Copy binary and libraries
if [ -f "target/debug/powerbeamer" ]; then
    cp target/debug/powerbeamer "$MACOS/powerbeamer"
elif [ -f "target/debug/presenter" ]; then
    cp target/debug/presenter "$MACOS/powerbeamer"
fi
ln -sf powerbeamer "$MACOS/presenter"
if [ -f "libpdfium.dylib" ]; then
    cp libpdfium.dylib "$MACOS/libpdfium.dylib"
fi

# 4. Copy application icons (.icns)
cp ui/icons/AppIcon.icns "$RESOURCES/AppIcon.icns"
cp ui/icons/app_icon.png "$RESOURCES/app_icon.png"

# 5. Generate Info.plist
cat <<EOF > "$CONTENTS/Info.plist"
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <key>CFBundleExecutable</key>
    <string>powerbeamer</string>
    <key>CFBundleIconFile</key>
    <string>AppIcon</string>
    <key>CFBundleIdentifier</key>
    <string>com.powerbeamer.app</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>PowerBeamer</string>
    <key>CFBundleDisplayName</key>
    <string>PowerBeamer</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>0.1.0</string>
    <key>CFBundleVersion</key>
    <string>1</string>
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSSupportsAutomaticGraphicsSwitching</key>
    <true/>
</dict>
</plist>
EOF

echo "Created $APP_DIR successfully!"
