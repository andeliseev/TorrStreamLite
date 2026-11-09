# To do / ideas for later

Nothing here is committed to; it's a list of things noticed so far. Rough priority order within each section.

## Not verified yet

- [ ] The Keenetic build on a router: kernel compatibility, the real 4K bitrate, seeks, peak memory, CPU, a long run alongside normal router and VPN traffic
- [ ] The local network prompt the first time other devices are allowed

## Performance

- [ ] At most four pieces download at once, so at most four peers send data at the same time. With many slow peers that caps the speed (4K from peers giving 1 MB/s each, for example). Raising it toward the readers' window needs a test on a real swarm
- [ ] With 16 KiB pieces each new piece still costs a few passes over all 6,144 slots (about 0.3 ms). Keeping the window between pieces, or a bitmap instead of a hashed set, would cut that further if the router needs it
- [ ] `/api/status`, polled every 1.5 s, carries the whole file list (up to 4,096 paths) and the engine's full stats, though the page only needs the files once per torrent. A separate request for the files would shrink it
- [ ] The memory figure is the resident size; Activity Monitor shows the physical footprint (`phys_footprint`), so the two can differ

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
