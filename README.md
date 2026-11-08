# <img src="app-icon.svg" width="36" height="36" align="top" alt=""> TorrStream Lite

Stream torrents on your Mac or your TV without saving them to disk. A small torrent streaming server written in Rust that keeps the video in RAM only.

- **Paste a magnet link or a torrent URL, or drop a .torrent file on the window,** and play any file right away in VLC or IINA, or copy its stream URL into another player
- **Nothing is written to disk.** The video goes through a 96 MiB RAM cache, and **Stop & release RAM** frees it at once
- **Seek anywhere:** only the parts you watch are downloaded
- **Watch on your TV:** allow other devices on your home network, and Lampa or another TorrServer app on your TV plays from your Mac, Samsung (Tizen) TVs included. It uses TorrServer's port, 8090
- **Download details:** speed, cache use and every connected peer with its own speed
- Light or dark look, following the system or your choice in Settings
- An experimental build for Keenetic routers runs the same server without a Mac

An independent project inspired by [TorrServer](https://github.com/YouROK/TorrServer), with a compatible playback API for Lampa.

## Download

Get the latest version from the **[Releases page](https://github.com/andeliseev/TorrStreamLite/releases/latest)**. What changed in each version is in the [changelog](CHANGELOG.md).

| System | File |
| --- | --- |
| macOS 12 or later, Apple Silicon | `TorrStream-Lite-<version>-macos-arm64.dmg` |
| Keenetic router (ARM64 with Entware), experimental | `TorrStream-Lite-<version>-linux-arm64.tar.gz` |

Each file has a `.sha256` checksum next to it.

### First launch

The app isn't notarized by Apple yet, so macOS asks once before opening it: open the app, then go to **System Settings → Privacy & Security** and click **Open Anyway**.

If macOS asks whether TorrStream Lite may find devices on your local network, click **Allow**. Without it, your TV can't connect.

## Watching on your Mac

1. Open **TorrStream Lite**, paste a magnet link or a torrent URL and click **Add**. Or drop a .torrent file anywhere on the window, or press ⌘O.
2. Click **Play** next to a file. It opens in VLC or IINA. For another player, click the copy button next to it and open that address in the player.
3. When you're done, click **Stop & release RAM**. One torrent plays at a time, so stop it before you add the next one.

Click **Download** in the sidebar to see the connected peers and their speeds. **Settings** (⌘,) has the home network port, the appearance (System, Light or Dark), cache details and information about the app.

## Watching on your TV

1. Keep the Mac awake and on the same home network as the TV.
2. In the sidebar, under **Home network**, turn on **Allow other devices**. The app shows the address to use, for example `http://192.168.1.35:8090`.
3. In Lampa on the TV, open **Settings → TorrServer** and enter that address. Other TorrServer apps have a similar setting.
4. Choose a torrent in Lampa and play it.

**Allow other devices** is off by default. While it's on, anything on your home network can add, play and stop torrents without a password, including web pages open on its devices, so turn it off when you're done watching on the TV. It listens only on your home network's private addresses, and no port forwarding is needed.

The address uses port 8090, the one TorrServer apps expect. If TorrServer itself runs on the same Mac, choose another port in **Settings**.

## Privacy

TorrStream Lite runs on your computer. It has no account, no analytics and no servers of its own.

- **Torrent traffic:** like any torrent client, it connects to the trackers and peers of the torrent you add, and to the DHT network. They see your IP address. Uploading to other peers is turned off.
- **Torrent URLs:** when you add a torrent by its URL, the app downloads the .torrent file from that address.
- **Nothing is kept:** the video stays in RAM and is released when you stop the torrent or quit. There's no history and no library. Lampa's watched positions last only until the torrent is stopped.
- **Settings:** the Mac app saves only the home network switch and port, the appearance, and the window's size and position.

## Current limits

- One torrent at a time, played as it is: there's no transcoding, so your player must support the file's format.
- The RAM cache must hold at least three of the torrent's pieces. With the default 96 MiB cache, pieces can be up to 32 MiB; a torrent with larger pieces shows an error.
- The Mac app is for Apple Silicon Macs only.
- The Keenetic build hasn't been tested on a router yet.
- BitTorrent v1 torrents only. No DLNA, search or seeding.

## Building from source and contributing

The server is written in Rust on a modified [librqbit](https://github.com/ikatson/rqbit) torrent engine, and the Mac app is a small Swift window around it. To build it yourself, run the tests or publish a release, see [docs/development.md](docs/development.md). How it works inside is in [docs/](docs/README.md), and the Keenetic setup in [docs/router.md](docs/router.md).

Bug reports and pull requests are welcome. To report a security problem, see [SECURITY.md](SECURITY.md).

## License

[MIT](LICENSE), © 2026 Andrei Eliseev. The modified librqbit engine stays under Apache-2.0; see the [third-party notices](THIRD_PARTY_NOTICES.txt).
