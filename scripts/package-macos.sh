#!/bin/sh
# Explicit distribution packaging; ordinary builds leave only the .app.
set -eu
PROJECT_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$PROJECT_ROOT"
VERSION=$(python3 -c 'import tomllib; print(tomllib.load(open("Cargo.toml", "rb"))["package"]["version"])')
APP="$PROJECT_ROOT/dist/TorrStream Lite.app"
test -d "$APP"
STAGE=$(mktemp -d "${TMPDIR:-/tmp}/torrstream-dmg.XXXXXX")
trap 'rm -rf "$STAGE"' EXIT HUP INT TERM
ditto "$APP" "$STAGE/TorrStream Lite.app"
ln -s /Applications "$STAGE/Applications"
mkdir -p release-assets
hdiutil create -volname "TorrStream Lite" -srcfolder "$STAGE" -ov -format UDZO \
    "release-assets/TorrStream-Lite-$VERSION-macos-arm64.dmg"
