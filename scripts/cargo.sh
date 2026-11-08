#!/bin/sh
set -eu
PROJECT_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
if [ -x "$PROJECT_ROOT/.build/cargo/bin/cargo" ]; then
    export CARGO_HOME="$PROJECT_ROOT/.build/cargo"
    export RUSTUP_HOME="$PROJECT_ROOT/.build/rustup"
    export PATH="$CARGO_HOME/bin:$PATH"
fi
cd "$PROJECT_ROOT"
exec cargo "$@"
