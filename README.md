# TorrStream Lite

A Rust, RAM-only torrent streaming prototype for **one 4K direct-play session** on a 512 MB Keenetic KN-1811, with a macOS app for testing first.

The streaming server and cache are Rust. The Mac window is a small Swift/WebKit wrapper. BitTorrent protocol support comes from a pinned, modified copy of `librqbit`; this is a new focused implementation, not a feature-complete port of TorrServer.

This is an independent project inspired by TorrServer, with a compatible API subset; it is not an official TorrServer release.

## Try the Mac app

Build locally and open **`dist/TorrStream Lite.app`** in Finder, or download a macOS DMG from [Releases](https://github.com/andeliseev/TorrStreamLite/releases) when one is published. This build is for Apple Silicon. Add a magnet link, HTTP(S) torrent URL, or `.torrent` file. Choose **Open in player** to use VLC, or IINA if VLC is not installed. Alternatively, copy the stream URL into your player.

The app starts its own server on an available loopback port, so an existing TorrServer on port 8090 can keep running. The Mac window, player, and decoder use memory separately from the server reading shown in the app. The app bundle is locally signed, not notarized for distribution.

If the app was open while rebuilding, quit and reopen it to use the new binary.

## Watch on a TV

1. Open **TorrStream Lite.app** and turn on **Allow TV access** in the **Watch on TV** panel.
2. Copy the displayed address, for example `http://192.168.1.35:8091`, into your TV app's server settings. Use the address actually displayed on your Mac.
3. Keep the Mac awake and connect the TV to the same home network. If macOS requests local-network or incoming-connection access, allow it for this app.

The Mac app remembers the switch and TV port. TV access is off on first launch. Port 8091 is separate from the Mac's private control port. If another program uses that port, choose another one under **Connection options**. Switching TV access on does not reload the torrent; switching it off closes TV connections while preserving Mac playback and the torrent cache.

The server binds only to private IPv4 LAN addresses, filters common VPN/tunnel interfaces, and refreshes its listeners when network addresses change. If both Wi-Fi and Ethernet are connected, it may show more than one address. Choose the one on your TV's network. A stable DHCP reservation for the Mac keeps the address consistent between reconnects.

In **Lampa**, enter that address in **Settings → TorrServer**. Version 0.2.1 accepts Lampa's JSON request format and the `lampa.mx`, `lampa.app`, and opaque (`null`) origins used by packaged TV widgets, including Samsung Tizen. It supports adding a torrent, selecting a file, preloading, playing, and seeking. Playback positions are kept only while that torrent remains active. Stop the current torrent in the Mac app before choosing a different torrent; this server supports one at a time.

TV access has no password: devices on that home network can control playback while it is enabled. No router port forwarding is needed. Lampa's HTTP sequence is covered by automated tests; actual TV firmware/player behavior still needs device testing. This version does not provide automatic TV discovery or DLNA. Custom Lampa web origins need the server's `--allow-origin` option.

## Direct torrent downloads with Happ on macOS

Configure VPN exceptions in Happ itself. The server executable is inside the Mac app at `TorrStream Lite.app/Contents/Resources/torrstream-lite`.

In Happ Desktop's per-app proxy settings, add the running **`torrstream-lite`** process (or its executable path), select the mode where selected applications bypass the VPN / connect directly, then reconnect Happ. Excluding only VLC or IINA does not exclude the server that downloads torrent data. Keep local-network access allowed as well so the TV can reach the Mac.

Happ documents desktop per-app bypass for its **DMG build**, not its App Store Mac Catalyst build: [official Happ documentation](https://github.com/HappDev/happ_su/blob/main/dev-docs/app-management.md). TorrStream Lite does not rewrite your VPN configuration, toggle the VPN, or report that bypass is active. An actual Happ routing test is still needed after configuring the exception.

When the server runs on Keenetic, a VPN running only on the TV or Mac does not set the router server's outgoing route. If the router itself uses a VPN, its own torrent downloads still follow the router's routing policy; the TV-to-router connection is a separate local connection.

## Project layout

| Folder | Contents |
|---|---|
| `src/` | Rust streaming server, bounded RAM cache, and TV access |
| `ui/` | Control page HTML, CSS, and JavaScript; embedded in the server |
| `macos/` | Native Mac window and app metadata |
| `scripts/` | Mac build, optional router build, and Rust toolchain helper |
| `tests/` | Local torrent, streaming, memory, and network checks |
| `vendor/` | Pinned Rust torrent engine, modifications, and license |
| `docs/` | Development notes and router setup |
| `.github/workflows/` | Automatic checks and manually started releases |
| `dist/` | Generated app for testing; currently only `TorrStream Lite.app` |

## Build the Mac app

```sh
sh scripts/build-macos.sh
```

This builds only the Mac app. It does not create a ZIP or Linux binary. DMG packaging and the router build are separate, optional commands.

## GitHub releases

Open [Actions → Release](https://github.com/andeliseev/TorrStreamLite/actions/workflows/release.yml), choose **Run workflow** on `main`, then select a version bump and **macOS**, **Keenetic**, or **macOS and Keenetic**. An optional exact version overrides the bump. The first release uses the source version (currently 0.2.1) unless overridden.

The workflow runs the checks, commits any version update, creates a draft, and builds an Apple Silicon DMG and/or a compressed static Linux ARM64 executable with SHA-256 checksum files. It publishes the release only after all selected builds succeed. It uses the repository's built-in `GITHUB_TOKEN`; no extra release secret is required. Repository rules must allow Actions to write `main` for automatic version updates. A failed build leaves a draft; start a new run for the same unpublished version to replace that draft and rebuild its assets.

Pushes to `main` and pull requests run **Checks** without publishing a release. The Mac app is ad-hoc signed and is not notarized; Keenetic builds are not a substitute for a real router playback test.

See [development and testing](docs/development.md) for build requirements, memory behavior, validation results, and the API subset. See [router setup](docs/router.md) when ready to test Keenetic.

`.build/` and `target/` are local toolchain/build caches. `dist/`, `release-assets/`, and `test-results/` are generated outputs. These folders and Python/macOS cache files are excluded from Git; they are not source files to upload to GitHub.

## Source provenance

See [engine provenance and changes](vendor/UPSTREAM.md). The project is inspired by [TorrServer](https://github.com/YouROK/TorrServer) and uses a modified Rust `librqbit` engine. No Go server is bundled or launched. The project and vendored engine use Apache-2.0; dependency licenses remain their own.
