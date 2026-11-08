# Keenetic router build

Running the server on a Keenetic router, without a Mac, is experimental: the build has **not been run on a router yet**. Check the router's kernel and firmware compatibility before installing it. [Back to the docs](README.md).

## Get the binary

Download `TorrStream-Lite-<version>-linux-arm64.tar.gz` from the [Releases page](https://github.com/andeliseev/TorrStreamLite/releases) of a release built with **Keenetic** (see [development.md](development.md#releases-github-actions)). It holds `torrstream-lite-linux-arm64`, a statically linked ARM64 Linux executable, and the licenses.

Or cross-build it on a Mac with a project-local `cargo-zigbuild` and Zig:

```sh
python3 -m venv .build/cross
.build/cross/bin/pip install cargo-zigbuild==0.24.1 ziglang==0.17.1
# Add aarch64-unknown-linux-musl to the Rust toolchain that scripts/cargo.sh uses.
# For the project-local toolchain:
CARGO_HOME="$PWD/.build/cargo" RUSTUP_HOME="$PWD/.build/rustup" .build/cargo/bin/rustup target add aarch64-unknown-linux-musl
sh scripts/build-router.sh
```

The build writes `dist/torrstream-lite-linux-arm64`.

## Run it

Copy the binary to Entware storage, make it executable, and start it on the router's **LAN address**, for example:

```sh
/opt/bin/torrstream-lite --listen 192.168.1.1:8090 --cache-mib 96 --peers 20
```

Replace the address with your router's own LAN address, and enter that address in Lampa. If you'd rather enter a name, such as `my.keenetic.net`, start the server with `--allow-host my.keenetic.net` too: other names are refused, to stop DNS rebinding. The server has no password: bind it to loopback or a trusted LAN, and keep it off the Internet. Downloading torrents over HTTPS needs the router's CA certificates; point `SSL_CERT_FILE` or `SSL_CERT_DIR` at Entware's when needed.

## What to check on the router

The router this was planned for uses about 173 of its 512 MB of RAM at baseline. Before relying on it, check the real 4K bitrate, seeks, a long playback run, peak memory (RSS), CPU use, and the effect on routing and VPN traffic. Success on a Mac says nothing about the router's throughput.
