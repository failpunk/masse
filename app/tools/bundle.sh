#!/bin/bash
# Packages the release binary as Shim.app.
#
# The bundle identifier matters more than it looks: WKWebView derives its
# persistent data store from app identity, so changing CFBundleIdentifier makes
# the app forget every Google login. Do not edit it casually.
set -euo pipefail
cd "$(dirname "$0")/.."

BUNDLE_ID="com.failpunk.shim"
APP="target/Shim.app"
VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)

cargo build --release
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/shim "$APP/Contents/MacOS/shim"

# Icon: reuse the PNG generator, upscaled to the sizes iconutil wants.
ICONSET=$(mktemp -d)/Shim.iconset
mkdir -p "$ICONSET"
ICON_OUT="$ICONSET" node ../tools/make-icons.mjs 16 32 64 128 256 512 1024 >/dev/null
for sz in 16 32 128 256 512; do
  mv "$ICONSET/icon$sz.png" "$ICONSET/icon_${sz}x${sz}.png"
done
mv "$ICONSET/icon32.png"   "$ICONSET/icon_16x16@2x.png"   2>/dev/null || true
mv "$ICONSET/icon64.png"   "$ICONSET/icon_32x32@2x.png"   2>/dev/null || true
mv "$ICONSET/icon1024.png" "$ICONSET/icon_512x512@2x.png" 2>/dev/null || true
cp "$ICONSET/icon_256x256.png" "$ICONSET/icon_128x128@2x.png"
cp "$ICONSET/icon_512x512.png" "$ICONSET/icon_256x256@2x.png"
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/Shim.icns"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Shim</string>
  <key>CFBundleDisplayName</key><string>Shim</string>
  <key>CFBundleIdentifier</key><string>$BUNDLE_ID</string>
  <key>CFBundleExecutable</key><string>shim</string>
  <key>CFBundleIconFile</key><string>Shim</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

# Ad-hoc signature. Not notarised, but enough that macOS keeps a stable identity
# for the app rather than re-prompting for permissions on every rebuild.
codesign --force --deep --sign - "$APP" 2>/dev/null || echo "[bundle] codesign skipped"

echo "[bundle] built $APP (v$VERSION, $BUNDLE_ID)"
du -sh "$APP" | awk '{print "[bundle] size " $1}'
