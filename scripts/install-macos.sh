#!/bin/bash
# Builds and signs Squawk with bundle-macos.sh, moves it to /Applications, and relaunches it.
#
# Installed there, the app runs from the same path every time and survives `cargo clean`, which
# deletes target/ and the bundle in it. It goes to ~/Applications instead if it's already
# installed there, or if /Applications isn't writable (no admin rights). The Microphone and
# Accessibility permissions carry over: macOS ties them to the bundle ID and certificate, not the
# path.
#
# Usage: scripts/install-macos.sh
# Env:   SQUAWK_SIGN_IDENTITY  passed on to bundle-macos.sh
set -euo pipefail

cd "$(dirname "$0")/.."
APP=target/Squawk.app
BUNDLE_ID=com.plyhdrn.squawk

if [[ -d ~/Applications/Squawk.app ]]; then
    DEST=~/Applications
elif [[ -d /Applications/Squawk.app || -w /Applications ]]; then
    DEST=/Applications
else
    DEST=~/Applications
fi
INSTALLED="$DEST/Squawk.app"

# Checked before building, so a clash fails fast. Another app called Squawk is left alone.
if [[ -d $INSTALLED ]]; then
    id=$(/usr/libexec/PlistBuddy -c "Print :CFBundleIdentifier" "$INSTALLED/Contents/Info.plist" 2>/dev/null || true)
    if [[ $id != "$BUNDLE_ID" ]]; then
        echo "$INSTALLED is a different app (${id:-no bundle ID}); move it aside first." >&2
        exit 1
    fi
fi

scripts/bundle-macos.sh

# A running Squawk, installed or from target/, is the old build. It saves settings as they
# change, so quitting it with SIGTERM loses nothing. Launch Services notices it's gone a moment
# after the process is, and opening it again before then fails with error -600.
if pkill -x Squawk; then
    for _ in $(seq 50); do
        [[ -z $(lsappinfo find bundleid="$BUNDLE_ID") ]] && break
        sleep 0.1
    done
fi

echo "Moving it to ${INSTALLED}…"
mkdir -p "$DEST"
rm -rf "$INSTALLED"
# Moved rather than copied, so Spotlight and Launch Services find only the installed copy.
mv "$APP" "$INSTALLED"
open "$INSTALLED"
echo "Installed $INSTALLED"
