# To do / ideas for later

Nothing here is committed to; it's a list of things noticed so far. Rough priority order within each section.

## Not verified yet

- [ ] The Keenetic build has run on one router, a KN-1811, for one 13-minute 4K film ([measurements](development.md#measurements)). Still open: seek times by the clock, the router's own free memory and load while it plays, a long run alongside normal router and VPN traffic, the release after five idle minutes (closing the film in Lampa released it at once), and other models. What to run and read off is in [router.md](router.md#first-run-checklist); the release only runs the binary on an emulated processor
- [ ] The router's start script works on a KN-1811: `start`, `stop`, `restart`, `status`, the server outliving the SSH session that started it, its lines in the system log, and Entware starting it when the router boots (running again about a minute into a restart). Not seen yet: that `chmod -x` keeps it from starting, and the five tries when the home network's address comes up late
- [ ] The three lines the server prints when it starts at boot weren't found in the KN-1811's system log, while those of a later `restart` were. The router's clock isn't set that early, so they may be there under an old date, or the log may not take lines yet. Look at the start of the log after a restart of the router
- [ ] On a router the server is in the DHT network around the clock, also with no torrent loaded, because the start script runs it from boot. Join the DHT with the first torrent and leave it some time after the last, or make `--no-dht` the script's default
- [ ] The built-in root certificates on a real router: the KN-1811 with a fresh Entware had certificates of its own (where from wasn't checked), so the server used those
- [ ] The IPv4-only mode on a system that really has no IPv6 (a Keenetic without that component): the tests only ask for it with `--ipv4-only`, and the DHT stays off in them
- [ ] The local network prompt the first time other devices are allowed
- [ ] Downloading from all peers at once on a real swarm: the speed, and that the piece a player waits for still arrives first

## Performance

- [ ] Swarms whose peers accept no connections can't be used: the app only connects out, over TCP. Measured on a real swarm of 217 addresses (direct connection, no VPN): 49 accepted TCP, 17 answered the handshake (8%), 28 were junk that reset at once, and uTP reached only 2 more. A small torrent's 5–9 addresses can all be dead. What other clients add is incoming connections: a listening port announced to trackers and the DHT, opened with UPnP. That's a listener facing the internet, to be designed with the security rules
- [ ] A magnet link's lookup asks each peer once. When all have failed and no new ones come, ask them again instead of waiting out the 85 s
- [ ] Every add announces to the trackers again, also inside their minimum interval: Lampa's retries make that every 90 s. Remember a tracker's last answer for the same torrent for its interval
- [ ] Reserving a slot zeroes the whole piece (up to 32 MiB) under the engine's lock, and each eviction walks every file to update per-file counters
- [ ] Eviction still looks at every cached piece to find the oldest one it may drop: 6,144 of them with 16 KiB pieces. It's no longer the limit (143 MiB/s, with 3.5 s of server CPU for 384 MiB on an Apple Silicon Mac), so only the router might need better
- [ ] The piece a player waits for downloads from one peer at a time, and nothing plays until all of it is checked. With the 8 MiB pieces common for films and peers at 1–2 MB/s, a seek waits 4–8 s. Fetch it from several peers at once, or read its chunks before the piece is checked, as TorrServer does
- [ ] A peer that drops mid-piece loses what it sent: `release_pieces_owned_by` clears the piece's chunks, even in RAM mode where the slot and its data stay. Keep them, as for a piece taken over
- [ ] 16 outstanding requests per peer cap each peer at 256 KiB per round trip: about 2.5 MB/s at 100 ms, as through a VPN. Size the window from each peer's rate
- [ ] A seek doesn't retry peers that are waiting out their reconnect backoff (10 s, then a minute, then six) after a real disconnect

## Security

- [ ] A request's body may arrive as slowly as its sender likes, and a response nobody reads keeps its connection, finished or not: only a request's headers and idle connections have a time limit. A TV switched off in the middle of a film holds its reader, and so the cache, until the system gives up on the connection, which takes many minutes; TCP keep-alives on accepted connections would notice sooner
- [ ] The router's start script runs the server as root, as Entware does with its services; it needs no privileges
- [ ] Stopping a torrent can log `manage_peer finished with error: chunk tracker empty, torrent was paused` at error level, with a peer's address: the engine reports every task that ends with an error that way. On the router those lines go to the system log
- [ ] Torrent URLs can point at services on the home network or the router itself, and the error says whether they answered. Refusing private addresses would break a local Jackett, so at least make those errors generic

## App and UI

- [ ] While a magnet link loads, show what its tracker answered (`lookup.tracker_error` in `/api/status`), not only after it gave up
- [ ] An optional password for home network access (**Allow other devices**). Lampa supports TorrServer's login and password, and it would also keep web pages open on other devices away from the TV address
- [ ] Cancel while a magnet link added in the window is resolving. Lampa's adds resolve in the background and Stop cancels them, but the window's own add holds the add/stop lock for up to 85 s, so Stop can't interrupt it
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
- [ ] A build for the many Keenetic models with MIPS processors; only ARM64 ones can run today's
- [ ] The router's start script holds its settings (address, cache, peers), so a newer script replaces them; read them from a file of their own
- [ ] On a router the server can only be switched off and on over SSH, with the start script; nothing in the page or the router's web interface does it
- [ ] `scripts/package-router.sh` packs the Keenetic archive with the owner names of the account that ran it (`tar -tv` shows them); give the entries a fixed owner
- [ ] The router guide's Entware steps come from one install: a KN-1811, Entware in the router's own storage, KeeneticOS as it was in November 2026. A USB drive, other models and later versions follow Keenetic's own instructions, untried here. Nobody has run its commands for uninstalling Entware
- [ ] The router guide's commands are for the shell of a Mac or of Linux; copying a file with `ssh … < file` doesn't work in Windows PowerShell
- [ ] The README and the router guide are in English only, while Keenetic routers and Lampa are mostly used by Russian speakers
- [ ] The router's default port, 8090, is also the default of Keenetic's own torrent client (Download Station); the server says the port is taken, but not by what
- [ ] A Windows version. The server is mostly portable (`network.rs` and `memory.rs` need Windows code). The window could move from Swift to Tauri 2, as in Unified Mail, which would also bring Windows installers, auto-update and a universal Mac build

## Code quality

- [ ] Unit tests for the compatibility routes in `server.rs` (`origin_allowed`, the Host check and the cross-site rule have them)
- [ ] An engine error banner stays after the torrent recovers, until it's dismissed
- [ ] `stream_file` wraps its channel receiver with the `async-stream` crate; `futures::stream::poll_fn` would do it without the extra dependency
- [ ] `cargo clippy -- -D warnings` in `vendor/rqbit` stops on four casts in `piece_tracker.rs` and `torrent_state/streaming.rs`; CI doesn't lint the engine

## Release and tests

- [ ] The streaming tests run a debug build, where a panic only ends one task; release builds abort the whole server. Test a release build and fail on `panicked` in the logs
- [ ] Harden the workflow: a read-only token in Build, actions pinned by SHA, hashed `pip install`, build attestations
- [ ] Some test checks can hardly fail: no memory bound at small caches, "cache budget respected", and "no files written" looks only at the working directory
- [ ] `tests/video_smoke.py` isn't documented
- [ ] The test tracker in `tests/e2e.py` prints a `BrokenPipeError` traceback now and then, when the engine drops an announce it no longer needs (seen in `tests/lampa.py`, with 0.2.9 too). The test still passes, but it reads like a failure
- [ ] Pin the Rust toolchain, and drop engine features the app doesn't use (uTP, UPnP, LSD, SOCKS) to shrink the binary
- [ ] `THIRD_PARTY_NOTICES.txt` adds 246 KB to the server, half of it the MIT text repeated for each crate's copyright lines; one copy of the text under all of those lines would take a fraction of that
- [ ] A re-taken piece can lose its requests if the peer-side cancel runs after the same peer took it again; the stall rule moves it 5 s later
