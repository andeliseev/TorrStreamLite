# Changelog

What changed in each version of TorrStream Lite. The newest is at the top; each release's notes on GitHub show the same list.

## Unreleased

- **Settings → Cache:** choose 96, 192 or 256 MiB. More cache keeps more of the video around where you're watching, for smoother playback on slow torrents and quicker seeks back. A change applies to the next torrent you add.
- Torrents made of many small pieces stream smoothly instead of slowing down and keeping a processor core busy.
- Fixed playback that could stop and send Lampa back to the file list when a peer stopped sending its part of the video. Another peer now takes that part over within seconds, and a torrent that's just slow gets five minutes instead of 45 seconds before the stream gives up.
- The window no longer leaves a web cache on disk, and the one earlier versions left is deleted.

## 0.2.3 (2026-11-09)

- A new look: the sidebar shows the status, download speed, cache, memory and the home network switch, and the main area shows the torrent's files. The window resizes cleanly and remembers its size and position.
- Drop a .torrent file anywhere on the window, paste a magnet link anywhere outside a text field, or use **File → Open Torrent File** (⌘O).
- **TV access** is now **Home network → Allow other devices**, and it uses port 8090 by default, the port TorrServer apps expect. A port you chose before stays.
- **Settings → Appearance:** follow the system, or always light or dark.
- An app icon, and the standard Mac menus: About, Hide (⌘H), Close Window (⌘W) and Minimize (⌘M).
- Fixed a stall of 10 seconds or more that could follow a seek or opening another file: the app no longer drops its peers between requests.
- Web pages open in your browser can no longer control TorrStream Lite or see what's playing, unless **Allow other devices** is on: then the TV address still works without a password for everything on your home network.

## 0.2.2 (2026-11-08)

- Click **Download** to see every connected peer, its client and its current speed. Peer details are only collected while the panel is open.
- **Settings** (⌘,) now holds the TV port, cache details and information about the app, so the main window is more compact.
- Settings shows the app's MIT license and the license of the bundled torrent engine.

## 0.2.1 (2026-11-08)

- First release. Add a torrent from a magnet link, a torrent URL or a .torrent file, and stream its files with the video kept in RAM only (a 96 MiB cache, nothing written to disk).
- Open a file in VLC or IINA, or copy its stream URL into another player. Seeking works, and only the parts you watch are downloaded.
- **TV access:** Lampa on a TV in the same home network can use your Mac as its TorrServer, including on Samsung (Tizen) TVs.
- An experimental build for Keenetic routers (static ARM64 Linux binary), not yet tested on a router.
