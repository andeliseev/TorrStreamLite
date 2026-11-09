# To do / ideas for later

Nothing here is committed to; it's a list of things noticed so far. Rough priority order within each section.

## Not verified yet

- [ ] The Keenetic build on a router: kernel compatibility, the real 4K bitrate, seeks, peak memory, CPU, a long run alongside normal router and VPN traffic
- [ ] The local network prompt the first time other devices are allowed
- [ ] Downloading from all peers at once on a real swarm: the speed, and that the piece a player waits for still arrives first

## Performance

- [ ] Eviction still looks at every cached piece to find the oldest one it may drop: 6,144 of them with 16 KiB pieces. It's no longer the limit (143 MiB/s, with 3.5 s of server CPU for 384 MiB on an Apple Silicon Mac), so only the router might need better

## App and UI

- [ ] An optional password for home network access (**Allow other devices**). Lampa supports TorrServer's login and password, and it would also keep web pages open on other devices away from the TV address
- [ ] Cancel while a magnet is resolving. The server holds its add/stop lock for up to 90 s, so Stop can't interrupt it
- [ ] Check the transparent titlebar by hand, on macOS 26 and an older version: the window buttons over the sidebar, dragging by the top band, double-click to zoom
- [ ] Open .torrent files and magnet links from Finder and browsers (document types and a URL scheme in `Info.plist`)
- [ ] Choose the player; today VLC is tried first, then IINA
- [ ] A universal build for Intel Macs
- [ ] Offer to restart the server from the alert when it stops

## Distribution

- [ ] Code signing and notarization
- [ ] A Windows version. The server is mostly portable (`network.rs` and `memory.rs` need Windows code). The window could move from Swift to Tauri 2, as in Unified Mail, which would also bring Windows installers, auto-update and a universal Mac build

## Code quality

- [ ] Unit tests for the compatibility routes in `server.rs` (`origin_allowed`, the Host check and the cross-site rule have them)
- [ ] An engine error banner stays after the torrent recovers, until it's dismissed
- [ ] `stream_file` wraps its channel receiver with the `async-stream` crate; `futures::stream::poll_fn` would do it without the extra dependency
