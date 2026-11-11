# Changelog

What changed in each version of TorrStream Lite. The newest is at the top; each release's notes on GitHub show the same list.

## Unreleased

## 0.2.10 (2026-11-11)

- Keenetic build: it now starts on a router that has no root certificates installed. There it checks HTTPS torrent links and trackers against a list built into it (Mozilla's); before, it didn't start on such a router. A router that has certificates keeps using its own.
- Keenetic build: it also starts on a router without IPv6, which on a Keenetic is a component that may not be installed. It then uses IPv4 only.
- Keenetic build: the download now includes a script that starts the server when the router boots, and stops, restarts and reports on it.
- Keenetic build: it has now run on a real router, a Keenetic Ultra (KN-1811), and played a 4K film there. The guide (`docs/router.md`) was rewritten after that install. It now covers adding Entware to the router, installing and trying the server, checking that it works, switching it off, updating, uninstalling, and what to do when something fails, and says which of its steps haven't been tried on a router yet.
- Connections that sit idle, or never send a request, are closed after 30 seconds, and each address serves at most 64 at once. A TV that was switched off, or a device that misbehaves, can no longer leave connections open for good. Players and TV apps aren't affected: a paused film waits as long as before.

## 0.2.9 (2026-11-09)

- Magnet links load faster, and some that timed out now load: the app asks 16 peers for a torrent's details at once instead of 4. Most peers in a swarm never answer, and waiting for them four at a time could use up all the time allowed.
- When a magnet link doesn't load, the window also says what the torrent's tracker answered. If the tracker no longer has the torrent ("Torrent not registered"), it was removed there or replaced with a newer one, and the window says to look for it again. It no longer points at a VPN when peers simply don't answer.
- Some trackers could give the app fewer peers for a magnet link: until the torrent's details arrived, the app told them it had nothing left to download, which looks like a seeder, and many trackers don't send seeders to seeders. It now reports an unfinished download from the start.

## 0.2.8 (2026-11-09)

- While a magnet link loads, the window counts the peers found and those that didn't answer. If it doesn't load, the window says why: no peers were found, or none answered. That includes torrents opened in Lampa, which shows only its own timeout. The app now gives up after 85 seconds instead of 90, before Lampa does.

## 0.2.7 (2026-11-09)

- Fixed seeks that took a long time or never started after a pause, or once the cache had filled. The app disconnected peers that had nothing to send yet, and a seek then waited, sometimes minutes, for them to be tried again.

## 0.2.6 (2026-11-09)

- Fixed Lampa's HASH error the first time you opened a torrent. Opening another torrent on the TV now replaces the one that's loaded, and Lampa no longer gives up after 30 seconds on a magnet link that's slow to load.

## 0.2.5 (2026-11-09)

- Steadier playback from public torrents: peers that have the whole torrent no longer disconnect between a player's requests, and trackers now see the app as downloading, so they send it those peers too.
- Lampa can play a torrent again after the app released it for sitting idle, or after Lampa dropped it.
- Closed ways for web pages, and for other devices on the same network, to reach the app's controls.
- The streaming server now quits with the app, even after a crash or Force Quit.
- Right-clicking the window no longer offers Back and Reload, which could leave it blank or bring back an old home network setting.
- **Settings → Third-party notices** lists every library built into the app, with its license and copyright notices, instead of only the torrent engine's license.

## 0.2.4 (2026-11-09)

- **Settings → Cache:** choose 96, 192 or 256 MiB. More cache keeps more of the video around where you're watching, for smoother playback on slow torrents and quicker seeks back. A change applies to the next torrent you add.
- Torrents made of many small pieces stream smoothly instead of slowing down and keeping a processor core busy.
- Downloads from all connected peers at once. Before, at most four peers sent data at a time, which could hold back torrents with many slow peers.
- **Memory** in the sidebar shows the same figure as Activity Monitor.
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
