#!/bin/sh
set -eu
PROJECT_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$PROJECT_ROOT"
export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-12.0}"
sh scripts/cargo.sh build --release --locked
APP="$PROJECT_ROOT/dist/TorrStream Lite.app"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" "$PROJECT_ROOT/.build/swift-cache"
cp target/release/torrstream-lite "$APP/Contents/Resources/torrstream-lite"
xcrun swiftc -O -target "$(uname -m)-apple-macosx$MACOSX_DEPLOYMENT_TARGET" -module-cache-path "$PROJECT_ROOT/.build/swift-cache" macos/App.swift -o "$APP/Contents/MacOS/TorrStreamLite" -framework Cocoa -framework WebKit
cp macos/Info.plist "$APP/Contents/Info.plist"
cp macos/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"
cp LICENSE THIRD_PARTY_NOTICES.txt "$APP/Contents/Resources/"
# The server inside the app is signed as <bundle identifier>.server.
BUNDLE_ID=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' macos/Info.plist)
codesign --force --sign - --identifier "$BUNDLE_ID.server" "$APP/Contents/Resources/torrstream-lite"
codesign --force --sign - "$APP"
# Finder and the Dock cache an app's icon by the bundle's date, which copying files
# into it doesn't change. Bump it and re-register, so a rebuild shows the current icon.
touch "$APP"
LSREGISTER=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
if [ -x "$LSREGISTER" ]; then "$LSREGISTER" -f "$APP"; fi
printf 'Built %s\n' "$APP"
