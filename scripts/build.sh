#!/bin/zsh
# Builds Murmur (Release) and prints the path to Murmur.app. Extra arguments go to xcodebuild,
# e.g. CURRENT_PROJECT_VERSION=42.
set -euo pipefail
cd "$(dirname "$0")/.."

(cd App && xcodegen generate --quiet)

xcodebuild \
  -project App/Murmur.xcodeproj \
  -scheme Murmur \
  -configuration Release \
  -derivedDataPath build/DerivedData \
  -quiet \
  build "$@" >&2

echo "$PWD/build/DerivedData/Build/Products/Release/Murmur.app"
