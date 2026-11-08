# TorrStream Lite

A lightweight torrent streaming server written in Rust. Stream to a player on your Mac or to Lampa on your TV, with video buffered entirely in RAM.

An independent project inspired by [TorrServer](https://github.com/YouROK/TorrServer), with a compatible playback API.

## Use on Mac

macOS downloads are published on the [Releases page](https://github.com/andeliseev/TorrStreamLite/releases).

1. Open **TorrStream Lite** and add a magnet link, torrent URL, or `.torrent` file.
2. Open the file in VLC or IINA, or copy its stream URL into your player.
3. Use **Stop & release RAM** when finished or before adding another torrent.

Click **Download** for live peer details. **Settings** contains the TV port, cache diagnostics, and app information.

## Watch on TV

1. Connect the Mac and TV to the same home network and keep the Mac awake.
2. Enable **TV access** and enter the address shown at the top of the app into Lampa's **Settings → TorrServer**.
3. Select a torrent in Lampa and play it. Samsung Tizen playback is supported.

TV access is off by default. While enabled, devices on your home network can control playback without a password. No port forwarding is needed.

## Project status

Supports one active torrent and direct playback, without transcoding. The default video cache is **96 MiB**; total server memory is higher. Keenetic router support is experimental and still needs testing on hardware.

[Development, builds, and releases](docs/dev.md) · [Keenetic setup](docs/router.md)

## License

App code: [MIT](LICENSE), © 2026 Andrei Eliseev. The modified `librqbit` engine retains Apache-2.0. See [third-party notices](THIRD_PARTY_NOTICES.txt).
