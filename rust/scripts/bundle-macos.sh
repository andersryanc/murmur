#!/bin/bash
# Builds the Rust Murmur in release and assembles a signed rust/target/Murmur.app.
#
# The bundle ID is com.plyhdrn.murmur-rs, deliberately not the Swift app's, so the two get
# separate Accessibility and Microphone permissions and can't overwrite each other's grants.
# Signing uses the hardened runtime with the audio-input entitlement, which macOS requires
# before it will show the microphone prompt to a hardened app.
#
# Usage: rust/scripts/bundle-macos.sh
# Env:   MURMUR_SIGN_IDENTITY  codesign identity (default: the Apple Development certificate)
set -euo pipefail

cd "$(dirname "$0")/.."
ROOT=$(pwd)
IDENTITY=${MURMUR_SIGN_IDENTITY:-"Apple Development: Ryan Anderson (N6KL49FYD2)"}
BUNDLE_ID=com.plyhdrn.murmur-rs
APP="$ROOT/target/Murmur.app"

VERSION=$(sed -n '/^\[workspace.package\]/,/^\[/s/^version *= *"\(.*\)"/\1/p' Cargo.toml)
# Build number: commits on this branch, so every bundle from a new commit sorts after the last.
BUILD=$(git rev-list --count HEAD 2>/dev/null || echo 1)

echo "Building murmur $VERSION ($BUILD) in release…"
cargo build --release -p murmur

echo "Assembling $APP…"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/murmur "$APP/Contents/MacOS/Murmur"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleDevelopmentRegion</key>
	<string>en</string>
	<key>CFBundleDisplayName</key>
	<string>Murmur</string>
	<key>CFBundleExecutable</key>
	<string>Murmur</string>
	<key>CFBundleIdentifier</key>
	<string>$BUNDLE_ID</string>
	<key>CFBundleInfoDictionaryVersion</key>
	<string>6.0</string>
	<key>CFBundleName</key>
	<string>Murmur</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
	<key>CFBundleShortVersionString</key>
	<string>$VERSION</string>
	<key>CFBundleVersion</key>
	<string>$BUILD</string>
	<key>LSApplicationCategoryType</key>
	<string>public.app-category.productivity</string>
	<key>LSMinimumSystemVersion</key>
	<string>13.0</string>
	<key>LSUIElement</key>
	<true/>
	<key>NSMicrophoneUsageDescription</key>
	<string>Murmur listens while you hold the dictation key, and only then.</string>
</dict>
</plist>
PLIST

ENTITLEMENTS="$ROOT/target/Murmur.entitlements"
cat > "$ENTITLEMENTS" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>com.apple.security.device.audio-input</key>
	<true/>
</dict>
</plist>
PLIST

echo "Signing as \"$IDENTITY\"…"
codesign --force --options runtime --timestamp=none \
    --entitlements "$ENTITLEMENTS" \
    --identifier "$BUNDLE_ID" \
    --sign "$IDENTITY" \
    "$APP"
codesign --verify --strict --verbose=2 "$APP"

echo "Done: $APP"
