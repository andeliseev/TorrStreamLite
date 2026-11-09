# To do / ideas for later

Nothing here is committed to; it's a list of things noticed so far. Rough priority order within each section.

## Not verified yet

- [ ] The Keenetic build on a router: kernel compatibility, the real 4K bitrate, seeks, peak memory, CPU, a long run alongside normal router and VPN traffic
- [ ] The local network prompt the first time other devices are allowed
- [ ] Downloading from all peers at once on a real swarm: the speed, and that the piece a player waits for still arrives first

## Performance

- [ ] Reserving a slot zeroes the whole piece (up to 32 MiB) under the engine's lock, and each eviction walks every file to update per-file counters
- [ ] Eviction still looks at every cached piece to find the oldest one it may drop: 6,144 of them with 16 KiB pieces. It's no longer the limit (143 MiB/s, with 3.5 s of server CPU for 384 MiB on an Apple Silicon Mac), so only the router might need better

## Security

- [ ] Header and idle timeouts and a connection limit per listener: `axum::serve` sets no timer, so slow or idle connections stay open (matters with **Allow other devices** on, and on the router)
- [ ] Torrent URLs can point at services on the home network or the router itself, and the error says whether they answered. Refusing private addresses would break a local Jackett, so at least make those errors generic

## App and UI

- [ ] An optional password for home network access (**Allow other devices**). Lampa supports TorrServer's login and password, and it would also keep web pages open on other devices away from the TV address
- [ ] Cancel while a magnet is resolving. The server holds its add/stop lock for up to 90 s, so Stop can't interrupt it
- [ ] Check the transparent titlebar by hand, on macOS 26 and an older version: the window buttons over the sidebar, dragging by the top band, double-click to zoom
- [ ] Open .torrent files and magnet links from Finder and browsers (document types and a URL scheme in `Info.plist`)
- [ ] Choose the player; today VLC is tried first, then IINA
- [ ] A universal build for Intel Macs
- [ ] Offer to restart the server from the alert when it stops
- [ ] Play is a link, which Tab skips in WKWebView (`tabFocusesLinks` is off); the license and peer lists can't be scrolled by keyboard; focus falls back to the page when Add or Stop hides the focused control
- [ ] Some light-theme colors are under 4.5:1 contrast (the banner, muted labels and hints), and the switch's off state is hard to see
- [ ] Stop shows "Adding torrent…" while the add/stop lock is held; report adding and stopping separately
- [ ] ⌘O or a drop while an add runs gets a raw 409 that stays in the banner; dropping a file that isn't a .torrent gives confusing errors
- [ ] The "server stopped" alert shows raw, colored log text, and failed requests show "Load failed"
- [ ] The error banner and network messages are rewritten on every poll, which screen readers may repeat
- [ ] macOS 12.0–12.2 (Safari 15.0–15.3) lacks `overflow-wrap: anywhere`, `:focus-visible` and `inert`; add fallbacks
- [ ] No way to zoom the page in the app (⌘+ and ⌘−)
- [ ] Closing the window quits the app and stops TV playback without asking

## Distribution

- [ ] Code signing and notarization
- [ ] A Windows version. The server is mostly portable (`network.rs` and `memory.rs` need Windows code). The window could move from Swift to Tauri 2, as in Unified Mail, which would also bring Windows installers, auto-update and a universal Mac build

## Code quality

- [ ] Unit tests for the compatibility routes in `server.rs` (`origin_allowed`, the Host check and the cross-site rule have them)
- [ ] An engine error banner stays after the torrent recovers, until it's dismissed
- [ ] `stream_file` wraps its channel receiver with the `async-stream` crate; `futures::stream::poll_fn` would do it without the extra dependency

## Release and tests

- [ ] Run the engine's own tests in the Release workflow; its vendored lockfile is out of date, so they can't run with `--locked`
- [ ] The streaming tests run a debug build, where a panic only ends one task; release builds abort the whole server. Test a release build and fail on `panicked` in the logs
- [ ] The release commit is pushed before the Swift app and the macOS-only Rust are compiled; compile them in the Test job, or commit in Publish
- [ ] Harden the workflow: a read-only token in Build, actions pinned by SHA, hashed `pip install`, build attestations
- [ ] License notices: the app and the router tarball include two license texts for about 285 crates; generate full notices (`cargo about`)
- [ ] The router build doesn't start without CA certificates (Entware's `ca-certificates`); bundle root certificates or document it, and smoke-run the binary
- [ ] Some test checks can hardly fail: no memory bound at small caches, "cache budget respected", and "no files written" looks only at the working directory
- [ ] `docs/router.md` runs a file name the tarball doesn't have, and `tests/video_smoke.py` isn't documented
- [ ] Pin the Rust toolchain, and drop engine features the app doesn't use (uTP, UPnP, LSD, SOCKS) to shrink the binary
- [ ] A re-taken piece can lose its requests if the peer-side cancel runs after the same peer took it again; the stall rule moves it 5 s later
