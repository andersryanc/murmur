#!/bin/zsh
# Builds Murmur (Release), installs it to /Applications, and relaunches it.
set -euo pipefail
cd "$(dirname "$0")/.."

APP=$(scripts/build.sh)
pkill -x Murmur 2>/dev/null && sleep 0.5 || true
rm -rf /Applications/Murmur.app
ditto "$APP" /Applications/Murmur.app
open /Applications/Murmur.app
echo "Installed /Applications/Murmur.app"
