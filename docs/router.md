# Keenetic router build

This is an optional later step. The Mac test app does not need a Linux binary. [Back to the app guide](../README.md).

The build command creates `dist/torrstream-lite-linux-arm64`, a statically linked ARM64 Linux executable. Cross-compilation was verified earlier, and the generated binary was removed during Mac-first cleanup. It has **not been run on your router**. Check the router's kernel and firmware compatibility before installation.

Cross-building from the Mac uses project-local `cargo-zigbuild` and Zig:

```sh
python3 -m venv .build/cross
.build/cross/bin/pip install cargo-zigbuild==0.24.1 ziglang==0.17.1
# Add aarch64-unknown-linux-musl to the Rust toolchain used by scripts/cargo.sh.
# For the project-local toolchain installed in this workspace:
CARGO_HOME="$PWD/.build/cargo" RUSTUP_HOME="$PWD/.build/rustup" .build/cargo/bin/rustup target add aarch64-unknown-linux-musl
sh scripts/build-router.sh
```

For an eventual manual router test, copy the binary to Entware storage, make it executable, and start it on the router's **LAN address**, for example:

```sh
/opt/bin/torrstream-lite --listen 192.168.1.1:8091 --cache-mib 96 --peers 20
```

Replace the address with your router's actual LAN address. The current prototype has no authentication: bind to loopback or a trusted LAN and keep it off the Internet. HTTPS tracker/torrent downloads need the router's CA certificates; Entware paths can be supplied using `SSL_CERT_FILE`/`SSL_CERT_DIR` when necessary.

Your reported baseline is 173/512 MB. Router acceptance should include actual 4K bitrate, seeks, a long playback run, peak RSS, CPU use, and routing/VPN load. A Mac success does not establish the router's throughput.
