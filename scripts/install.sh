#!/bin/zsh
# Builds Murmur (Release), installs it, and relaunches it. Installs to /Applications, or to
# ~/Applications if you can't write there (no admin rights) or it's already installed there.
set -euo pipefail
cd "$(dirname "$0")/.."

if [[ -d ~/Applications/Murmur.app ]]; then
  dest=~/Applications
elif [[ -d /Applications/Murmur.app || -w /Applications ]]; then
  dest=/Applications
else
  dest=~/Applications
fi

APP=$(scripts/build.sh)
pkill -x Murmur 2>/dev/null && sleep 0.5 || true
mkdir -p "$dest"
rm -rf "$dest/Murmur.app"
ditto "$APP" "$dest/Murmur.app"
open "$dest/Murmur.app"
echo "Installed $dest/Murmur.app"
