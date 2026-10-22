#!/bin/bash
# Builds Squawk in release and assembles a signed target/Squawk.app.
#
# The bundle ID is com.plyhdrn.squawk. macOS ties the Accessibility and Microphone grants to it,
# so changing it means granting them again.
# Signing uses the hardened runtime with the audio-input entitlement, which macOS requires
# before it will show the microphone prompt to a hardened app.
#
# Squawk isn't distributed prebuilt: you build and sign it on the Mac that runs it. A
# self-signed certificate is enough for that (see "Signing" in the README), and macOS keeps the
# app's permissions across rebuilds as long as the certificate stays the same.
#
# Usage: scripts/bundle-macos.sh
# Env:   SQUAWK_SIGN_IDENTITY  codesign identity (default: "Squawk Self-Signed")
set -euo pipefail

cd "$(dirname "$0")/.."
ROOT=$(pwd)
IDENTITY=${SQUAWK_SIGN_IDENTITY:-"Squawk Self-Signed"}
BUNDLE_ID=com.plyhdrn.squawk
APP="$ROOT/target/Squawk.app"

# Without -v, so a self-signed certificate counts even though no root trusts it.
if ! security find-identity -p codesigning | grep -qF "\"$IDENTITY"; then
    echo "No code-signing certificate named \"$IDENTITY\" in your Keychain." >&2
    echo "Create one as described under \"Signing\" in the README, or set SQUAWK_SIGN_IDENTITY" >&2
    echo "to one of these:" >&2
    security find-identity -p codesigning | sed -n 's/.*"\(.*\)".*/  \1/p' | sort -u >&2
    exit 1
fi

VERSION=$(sed -n '/^\[workspace.package\]/,/^\[/s/^version *= *"\(.*\)"/\1/p' Cargo.toml)
# Build number: commits on this branch, so every bundle from a new commit sorts after the last.
BUILD=$(git rev-list --count HEAD 2>/dev/null || echo 1)

echo "Building squawk $VERSION ($BUILD) in release…"
cargo build --release -p squawk

echo "Assembling ${APP}…"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/squawkctl "$APP/Contents/MacOS/Squawk"
# sherpa-onnx and ONNX Runtime are shared libraries on macOS (see squawk-stt's Cargo.toml); the
# binary finds them through its @executable_path/../Frameworks rpath.
mkdir -p "$APP/Contents/Frameworks"
cp target/release/libsherpa-onnx-c-api.dylib target/release/libonnxruntime.dylib "$APP/Contents/Frameworks/"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleDevelopmentRegion</key>
	<string>en</string>
	<key>CFBundleDisplayName</key>
	<string>Squawk</string>
	<key>CFBundleExecutable</key>
	<string>Squawk</string>
	<key>CFBundleIdentifier</key>
	<string>$BUNDLE_ID</string>
	<key>CFBundleInfoDictionaryVersion</key>
	<string>6.0</string>
	<key>CFBundleName</key>
	<string>Squawk</string>
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
	<string>Squawk listens while you hold the dictation key, and only then.</string>
</dict>
</plist>
PLIST

# The hardened runtime only loads libraries signed by the app's own Team ID, and a self-signed
# certificate has none, so dyld would refuse the bundled sherpa-onnx dylibs ("different Team
# IDs"). disable-library-validation lifts that; the libraries are still signed and sealed in.
ENTITLEMENTS="$ROOT/target/Squawk.entitlements"
cat > "$ENTITLEMENTS" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>com.apple.security.device.audio-input</key>
	<true/>
	<key>com.apple.security.cs.disable-library-validation</key>
	<true/>
</dict>
</plist>
PLIST

echo "Signing as \"$IDENTITY\"…"
# Inside out: the libraries first, then the app that seals them.
codesign --force --options runtime --timestamp=none --sign "$IDENTITY" "$APP"/Contents/Frameworks/*.dylib
codesign --force --options runtime --timestamp=none \
    --entitlements "$ENTITLEMENTS" \
    --identifier "$BUNDLE_ID" \
    --sign "$IDENTITY" \
    "$APP"
codesign --verify --strict --verbose=2 "$APP"

echo "Done: $APP"
