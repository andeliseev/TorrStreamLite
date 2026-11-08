#!/bin/sh
set -eu
PROJECT_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$PROJECT_ROOT"
if [ -d "$PROJECT_ROOT/.build/cross/bin" ]; then
    export PATH="$PROJECT_ROOT/.build/cross/bin:$PATH"
    export CARGO_ZIGBUILD_PYTHON_PATH="$PROJECT_ROOT/.build/cross/bin/python"
fi
export CARGO_ZIGBUILD_CACHE_DIR="$PROJECT_ROOT/.build/zigbuild"
export ZIG_GLOBAL_CACHE_DIR="$PROJECT_ROOT/.build/zig-cache"
mkdir -p "$CARGO_ZIGBUILD_CACHE_DIR" "$ZIG_GLOBAL_CACHE_DIR" dist
sh scripts/cargo.sh zigbuild --release --locked --target aarch64-unknown-linux-musl -j 4
cp target/aarch64-unknown-linux-musl/release/torrstream-lite dist/torrstream-lite-linux-arm64
printf 'Built %s\n' "$PROJECT_ROOT/dist/torrstream-lite-linux-arm64"
