# To do / ideas for later

Nothing here is committed to; it's a list of things noticed so far. Rough priority order within each section.

## Not verified yet

- [ ] Long playback from public swarms (the automated tests use a local peer only)
- [ ] Lampa on a real TV, including Samsung Tizen: the widget, the decoder, seeking
- [ ] The Keenetic build on a router: kernel compatibility, the real 4K bitrate, seeks, peak memory, CPU, a long run alongside normal router and VPN traffic
- [ ] First launch of a released DMG on another Mac (Gatekeeper, the local network prompt)
- [ ] A measured comparison with TorrServer

## Performance

- [ ] For small-piece torrents the RAM scheduler is quadratic in the number of cache slots. `ram_wanted_pieces` (vendor `torrent_state/streaming.rs`) removes duplicates with `Vec::contains`, and `RamCache::reserve` checks every slot against the protected list with `contains`, under the engine lock, for each reservation. With 96 MiB and 64 KiB pieces that's 1,536 slots. A set or a bitmap, or a capped window, makes it linear. This matters most on the router's CPU
- [ ] `/api/status`, polled every 1.5 s, carries the whole file list (up to 4,096 paths) and the engine's full stats, though the page only needs the files once per torrent. A separate request for the files would shrink it
- [ ] The memory figure is the resident size; Activity Monitor shows the physical footprint (`phys_footprint`), so the two can differ

## App and UI

- [x] Redesign in Unified Mail's style: a sidebar with the state, the files in the main pane, one surface under a transparent titlebar, drag and drop and paste, Appearance in Settings
- [ ] An optional password for home network access (**Allow other devices**). Lampa supports TorrServer's login and password, and it would also keep web pages open on other devices away from the TV address
- [ ] Cancel while a magnet is resolving. The server holds its add/stop lock for up to 90 s, so Stop can't interrupt it
- [x] An app icon
- [x] Remember the window's size and position; the standard Window menu, Close Window (⌘W), Minimize (⌘M) and Hide (⌘H)
- [ ] Check the transparent titlebar by hand, on macOS 26 and an older version: the window buttons over the sidebar, dragging by the top band, double-click to zoom
- [ ] Open .torrent files and magnet links from Finder and browsers (document types and a URL scheme in `Info.plist`)
- [ ] Choose the player; today VLC is tried first, then IINA
- [ ] A universal build for Intel Macs
- [ ] Offer to restart the server from the alert when it stops
- [x] A themed startup screen

## Distribution

- [x] One-click Release workflow with release notes from `CHANGELOG.md`
- [ ] Code signing and notarization
- [x] Screenshots in the README, light and dark, as in Unified Mail's
- [x] A website: home page, privacy policy and support page
- [ ] A Windows version. The server is mostly portable (`network.rs` and `memory.rs` need Windows code). The window could move from Swift to Tauri 2, as in Unified Mail, which would also bring Windows installers, auto-update and a universal Mac build

## Code quality

- [ ] Unit tests for the compatibility routes in `server.rs` (`origin_allowed`, the Host check and the cross-site rule have them)
- [x] `api()` in `ui/app.js` shows plain-text errors (such as a refused origin) instead of a JSON parser message
- [ ] An engine error banner stays after the torrent recovers, until it's dismissed
- [ ] `stream_file` wraps its channel receiver with the `async-stream` crate; `futures::stream::poll_fn` would do it without the extra dependency
- [ ] Rename the bundle identifier `local.torrstream-lite.mac` to a reverse-DNS name before the first notarized release (it resets the saved TV settings once)
