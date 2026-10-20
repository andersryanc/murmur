#!/bin/zsh
# Installs or updates Murmur from the latest GitHub Release, for a Mac that doesn't build it
# (see scripts/release.sh). Needs no Xcode or admin rights; run it again to update:
#
#   curl -fsSL https://raw.githubusercontent.com/andersryanc/murmur/main/scripts/get.sh | zsh
#
# Downloads with curl, which doesn't quarantine the app, so Gatekeeper doesn't block it for not
# being notarized.
set -euo pipefail

url=https://github.com/andersryanc/murmur/releases/latest/download/Murmur.zip

if [[ $(uname -m) != arm64 ]]; then
  echo "Murmur needs an Apple Silicon Mac." >&2
  exit 1
fi
if (( ${$(sw_vers -productVersion)%%.*} < 26 )); then
  echo "Murmur needs macOS 26 or later." >&2
  exit 1
fi

# Update in place; otherwise /Applications if you can write to it, else ~/Applications.
if [[ -d ~/Applications/Murmur.app ]]; then
  dest=~/Applications
elif [[ -d /Applications/Murmur.app || -w /Applications ]]; then
  dest=/Applications
else
  dest=~/Applications
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
echo "Downloading the latest release…"
curl -fL --progress-bar -o "$tmp/Murmur.zip" "$url"
ditto -x -k "$tmp/Murmur.zip" "$tmp"
codesign --verify --strict "$tmp/Murmur.app"
xattr -dr com.apple.quarantine "$tmp/Murmur.app" 2>/dev/null || true

pkill -x Murmur 2>/dev/null && sleep 0.5 || true
mkdir -p "$dest"
rm -rf "$dest/Murmur.app"
ditto "$tmp/Murmur.app" "$dest/Murmur.app"
open "$dest/Murmur.app"

info="$dest/Murmur.app/Contents/Info.plist"
version=$(defaults read "$info" CFBundleShortVersionString)
build=$(defaults read "$info" CFBundleVersion)
echo "Installed Murmur $version ($build) to $dest/Murmur.app"

if ! security find-generic-password -s com.plyhdrn.murmur -a anthropic-api-key >/dev/null 2>&1; then
  echo
  echo "For Claude cleanup, store your Anthropic API key in the Keychain:"
  echo "  security add-generic-password -s com.plyhdrn.murmur -a anthropic-api-key -w"
fi
