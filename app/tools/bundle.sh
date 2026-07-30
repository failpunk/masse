#!/bin/bash
# Packages the release binary as Masse.app.
#
# The bundle identifier matters more than it looks: WKWebView derives its
# persistent data store from app identity, so changing CFBundleIdentifier makes
# the app forget every Google login. Do not edit it casually.
set -euo pipefail
cd "$(dirname "$0")/.."

BUNDLE_ID="com.failpunk.masse"
APP="target/Masse.app"
VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)

# Quit a running copy politely first. WebKit flushes its cookie jar on clean
# shutdown and drops it on a hard kill, so `pkill` here would silently sign the
# user out of every Google account on every rebuild.
if pgrep -f "$APP/Contents/MacOS" >/dev/null 2>&1 || pgrep -x masse >/dev/null 2>&1; then
  osascript -e 'tell application "Masse" to quit' >/dev/null 2>&1 || true
  for _ in 1 2 3 4 5 6 7 8 9 10; do
    pgrep -x masse >/dev/null 2>&1 || break
    sleep 1
  done
  pgrep -x masse >/dev/null 2>&1 && echo "[bundle] WARNING: Masse would not quit; not force-killing (cookies would be lost)"
fi

# Refuse to rebuild a version that has already been released. Relying on
# remembering to bump is how a build ships under someone else's version number.
if git rev-parse "v$VERSION" >/dev/null 2>&1; then
  echo "[bundle] ERROR: v$VERSION is already tagged. Bump the version in Cargo.toml." >&2
  exit 1
fi

cargo build --release
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/masse "$APP/Contents/MacOS/masse"

# Icon: reuse the PNG generator, upscaled to the sizes iconutil wants.
ICONSET=$(mktemp -d)/Masse.iconset
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
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/Masse.icns"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Masse</string>
  <key>CFBundleDisplayName</key><string>Masse</string>
  <key>CFBundleIdentifier</key><string>$BUNDLE_ID</string>
  <key>CFBundleExecutable</key><string>masse</string>
  <key>CFBundleIconFile</key><string>Masse</string>
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

# Install to /Applications. Without this the only copy lives under target/, which
# any rebuild or `cargo clean` deletes, so the Dock icon and Spotlight entry break.
# Same bundle id, so the session and cookie jar carry over.
INSTALLED="/Applications/Masse.app"
if rsync -a --delete "$APP/" "$INSTALLED/" 2>/dev/null; then
  codesign --force --deep --sign - "$INSTALLED" 2>/dev/null || true
  echo "[bundle] installed $INSTALLED"
else
  echo "[bundle] WARNING: could not install to $INSTALLED; run it from $APP"
fi

# Refresh the landing page's download so it can never be an older build than the
# app. ditto, not zip: it preserves the bundle layout and the code signature.
if [ -d ../site ]; then
  ditto -c -k --keepParent "$INSTALLED" ../site/Masse.zip 2>/dev/null \
    && echo "[bundle] refreshed ../site/Masse.zip"
  # Stamp the version wherever the page states it, so the page can never claim a
  # different build than the zip beside it. Scoped to data-version lines only.
  if [ -f ../site/index.html ]; then
    sed -i '' -E "/data-version/s/[0-9]+\.[0-9]+\.[0-9]+/$VERSION/" ../site/index.html \
      && echo "[bundle] stamped v$VERSION into ../site/index.html"
  fi
fi

echo "[bundle] built $APP (v$VERSION, $BUNDLE_ID)"
du -sh "$APP" | awk '{print "[bundle] size " $1}'
