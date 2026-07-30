#!/bin/bash
# Renders site/og.html to site/og.png at 1200x630 using headless Chrome, so the
# social card carries the site's real typography rather than a hand-drawn
# approximation. Re-run after editing og.html, then redeploy.
set -euo pipefail
cd "$(dirname "$0")/.."
CHROME="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
[ -x "$CHROME" ] || { echo "Chrome not found at $CHROME"; exit 1; }
"$CHROME" --headless=new --disable-gpu --hide-scrollbars \
  --force-device-scale-factor=1 --window-size=1200,630 \
  --virtual-time-budget=6000 \
  --screenshot="$PWD/site/og.png" "file://$PWD/site/og.html" >/dev/null 2>&1
[ -f site/og.png ] || { echo "screenshot failed"; exit 1; }
echo "[og] wrote site/og.png ($(stat -f%z site/og.png) bytes)"
