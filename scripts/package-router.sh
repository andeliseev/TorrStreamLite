#!/bin/sh
# The router download: the binary that scripts/build-router.sh built, its Entware start
# script and the licenses, in one folder, as
# release-assets/TorrStream-Lite-<version>-linux-arm64.tar.gz.
set -eu
PROJECT_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$PROJECT_ROOT"
VERSION=$(python3 -c 'import tomllib; print(tomllib.load(open("Cargo.toml", "rb"))["package"]["version"])')
NAME="TorrStream-Lite-$VERSION-linux-arm64"
BINARY=dist/torrstream-lite-linux-arm64
test -f "$BINARY"
STAGE=$(mktemp -d "${TMPDIR:-/tmp}/torrstream-router.XXXXXX")
trap 'rm -rf "$STAGE"' EXIT HUP INT TERM
mkdir "$STAGE/$NAME"
cp LICENSE THIRD_PARTY_NOTICES.txt "$BINARY" scripts/router/S99torrstream-lite "$STAGE/$NAME/"
chmod 755 "$STAGE/$NAME/torrstream-lite-linux-arm64" "$STAGE/$NAME/S99torrstream-lite"
mkdir -p release-assets
# COPYFILE_DISABLE keeps a Mac's tar from adding its "._" attribute files.
COPYFILE_DISABLE=1 tar -czf "release-assets/$NAME.tar.gz" -C "$STAGE" "$NAME"
printf 'Packed %s\n' "$PROJECT_ROOT/release-assets/$NAME.tar.gz"
