# Architecture

TorrStream Lite is one Rust process: an HTTP server ([axum](https://github.com/tokio-rs/axum)) in front of a modified [librqbit](https://github.com/ikatson/rqbit) torrent engine whose only storage is a fixed RAM cache. The Mac app is a Swift window with a WKWebView. It starts the server on a free loopback port and shows its control page, which is static HTML/CSS/JS compiled into the server binary.

```mermaid
flowchart LR
  subgraph Mac["Mac app (macos/App.swift)"]
    Page["WKWebView<br/>control page (ui/)"]
  end
  subgraph Server["torrstream-lite (src/)"]
    HTTP["server.rs<br/>native and TorrServer API"]
    TV["network.rs<br/>TV access listeners"]
    Cache["cache.rs<br/>RAM cache"]
    Engine["librqbit<br/>(vendor/rqbit)"]
  end
  Page -- "HTTP, 127.0.0.1" --> HTTP
  Player["VLC / IINA"] -- "range requests" --> HTTP
  Lampa["Lampa on a TV"] -- "HTTP, LAN :8090" --> TV
  TV --> HTTP
  HTTP --> Engine
  Engine --> Cache
  Engine --> Swarm["trackers, DHT, peers"]
```

## Rust modules

| Module | Responsibility |
| --- | --- |
| `main.rs` | Command-line options, the engine session (upload off, DHT without persistence, connection limits), server start and graceful shutdown |
| `server.rs` | Routes and browser-origin checks, adding and stopping the one active torrent, range streaming, preload, the TorrServer-compatible routes, releasing an idle torrent |
| `cache.rs` | `RamFactory` and `RamStorage`: one anonymous memory mapping split into piece slots, reserved before network requests, least-recently-used eviction |
| `network.rs` | TV access (in the app: **Home network → Allow other devices**): finds the Mac's private LAN addresses and runs one listener per address on the TV port, checked again every 3 s |
| `range.rs` | Parses a single HTTP byte range |
| `memory.rs` | The server's memory for the app: the physical footprint on macOS, as Activity Monitor shows it, and the resident size |

Options: `--listen` (default `127.0.0.1:8090`; the Mac app passes port 0), `--cache-mib` (96; Settings → Cache changes it for the next torrent, see `POST /api/cache`), `--peers` (20), `--idle-seconds` (300, 0 turns it off), `--tv-port` (8090, the port TorrServer apps expect), `--allow-origin` and `--allow-host` (both repeatable, see [Security](#security)), `--no-dht`, and `--exit-on-stdin-close` (the Mac app's: exit when standard input closes). The server prints `TORRSTREAM_LISTEN=http://…` once it listens; the Mac app reads that line.

The engine's local changes are listed in [vendor/UPSTREAM.md](../vendor/UPSTREAM.md). They only apply when the storage is the RAM cache.

## Memory

- One shared, anonymous RAM mapping, **96 MiB by default**; the Mac app offers 192 and 256 MiB too. No video files, disk cache, session database or DHT persistence.
- The readers' window is one range of pieces per reader, not a list, and the next piece to download is found by skipping downloaded ones a word of bits at a time. Nothing builds a set for each new piece; eviction still looks at each cached piece once. This matters with small pieces: a 96 MiB cache of 16 KiB pieces has 6,144 slots, and it all runs under the engine's lock for every piece.
- A reusable piece slot is reserved **before any network request**. Every connected peer can download a wanted piece at the same time; the readers' window bounds how many. No payload allocation grows with torrent size.
- Only the windows that active HTTP readers request are downloaded. There's no background whole-file download.
- Readers' windows and pieces being verified are protected from eviction. Eviction clears the engine's HAVE and per-file counters, so backward seeks download again correctly.
- After a seek, obsolete requests are cancelled and late replies are discarded safely. Slots are reused after their writers finish. Reads and eviction coordinate through the same engine lock.
- A piece readers want moves to the next peer that asks when its peer has sent nothing at all for 5 s since taking it (it choked us or stopped; a slow peer's new piece waits behind its earlier requests, so any chunk counts), or has had it 10 times longer than the asking peer's average piece time (3 times for the piece a reader is at, or when there's nothing new to download). The chunks already received stay, and a piece taken over doesn't count toward the new peer's average. Peers often choke a client that doesn't upload, and without this one peer could hold back the piece a player is waiting for.
- At most 20 connected or connecting peers by default, 16 outstanding block requests per peer, four metadata connections and 256 discovered peer candidates. Peers stay connected while the torrent is loaded, also between a player's requests, so a seek doesn't wait for reconnections: RAM storage selects no files, so the engine must never treat it as finished, and it tells trackers the torrent's size less what's cached as what's left, so they don't take it for a seeder (many send seeders no other seeders).
- Once a reader's window is cached, or the player pauses, peers have nothing to send, and seeders send a keep-alive only every couple of minutes. The 15 s read timeout applies only while a peer owes us chunks; one that has sent its bitfield, has unchoked us and owes nothing may stay quiet (one that keeps us choked still goes, so it doesn't hold a peer slot). Before, every seeder was dropped this way, and the reconnect backoff (10 s, then a minute, then six) left a seek with no one to download from. Our keep-alives go out every 5 s, so peers don't drop us either.
- Up to four simultaneous HTTP requests for one playback session, fewer when pieces are large. Each response has one 64 KiB queued chunk and a bounded producer buffer.
- A response starts at the requested position (a stream created at 0 and then moved would first ask peers for the start of the file) and waits for a slow torrent, ending only after five minutes with no data, so the player decides how long to wait: ending it early sends TV apps back to their menu. A preload waits as long, and playback clears it.
- Stop cancels slow readers and unmaps the cache. An idle torrent is released after five minutes.

The cache limit is **not a cap on total memory**. Torrent metadata, network buffers, DHT, TLS, runtime threads and the OS need memory too. The target for the whole server is under 192 MiB; confirm it with real swarms and on the router.

Pieces must be at least 16 KiB, and the cache must hold at least three pieces: with 96 MiB, pieces up to 32 MiB fit. A torrent with larger pieces gets a clear error instead of going over the budget. Metadata is limited to 2 MiB, 131,072 pieces, 4,096 files and 512 GiB of content.

## HTTP API

Native API, used by the control page:

| Request | Behavior |
| --- | --- |
| `GET /api/status` | Memory (`memory_bytes`, the footprint on macOS, and `resident_bytes`), cache accounting and the torrent's progress, without its files |
| `GET /api/files` | The torrent's files; the page reads them once per torrent |
| `GET /api/peers` | Connected peers' counters, the torrent session ID and a monotonic sample time; no stored history |
| `GET /api/network` | TV access: addresses, port, and whether the caller may change it |
| `POST /api/network` | Loopback only: `{"enabled":true,"port":8090}`; the torrent keeps playing |
| `POST /api/cache` | Loopback only: `{"mib":192}` (4 to 256), the size of the next torrent's cache; a loaded torrent keeps its own. `/api/status` reports it as `cache_setting_mib` |
| `POST /api/torrents` | `{"link":"magnet:?..."}` or a torrent URL |
| `POST /api/torrents/file` | Raw `.torrent` bytes |
| `DELETE /api/torrents` | Cancels playback and releases the torrent |
| `GET/HEAD /api/stream/{file}` | Streams a **1-based** file index, with one HTTP byte range |
| `GET /licenses/app`, `GET /licenses/third-party` | The MIT license, and `THIRD_PARTY_NOTICES.txt`: the licenses and copyright notices of the crates compiled into the server |

TorrServer-compatible subset, used by Lampa: `/echo`; `POST /torrents` with the actions `add`, `list`, `get`, `set`, `rem` and `drop`; `POST /settings` action `get`; `POST /cache` action `get` (a summary, with empty per-piece and reader details); `POST /viewed` actions `list`, `set` and `rem` (for the current torrent, in RAM only); and `/stream?link=<hash-or-magnet>&index=1&play`. Lampa gives `add` 30 s and then asks `get` every 2 s until `file_stats` is there, because TorrServer answers at once. So `add` with a magnet link answers right away with the hash and `stat` 1 ("Torrent getting info", no `file_stats`), and a task resolves the link in the background, holding the add/stop lock; `get` and `list` show it meanwhile, and `drop`, `rem` and Stop cancel it. Lampa also adds the next torrent before it drops the last, so its `add` replaces the loaded torrent, while the app's own add still asks for Stop first. `drop` releases the torrent but keeps its link in memory, as the idle release does, so a later `/stream?link=<hash>` adds it again the way it was added; `rem` and Stop forget it, and any other hash becomes a magnet link for DHT. `/stream` also supports `stat`, `m3u` and an asynchronous `preload`. Preload warms up to 8 MiB or a quarter of the cache, whichever is smaller, using the existing cache and one 64 KiB read buffer. Playback or Stop cancels an unfinished preload.

Not supported: the full TorrServer UI and settings API, a persistent library or view history (`save_to_db` isn't stored), `torrs://`, DLNA, search providers, transcoding, seeding and more than one active torrent. BitTorrent v1 (`btih`) is the supported protocol; pure v2 torrents aren't. Peers are reached with outgoing TCP connections.

Lampa's jQuery requests carry JSON but can label it as form data. The compatibility routes accept that, within the same 2 MiB request limit; native routes require real JSON.

## Security

- The Mac app binds the server to `127.0.0.1` on a random port. The standalone server's default is `127.0.0.1:8090`.
- **Host names.** A request whose `Host` isn't an IP address, `localhost` or a name passed with `--allow-host` is refused, and so is a malformed one (a port that isn't a number, or a Host longer than any real one). `.local` names work only on listeners that face the home network: any device there can answer for one and point it at the Mac's loopback server. A web page that rebinds its own domain to the server's address (DNS rebinding) still sends that domain, so it can't get through.
- **Browser origins.** A request with an `Origin` header is refused unless it's the same origin, an origin passed with `--allow-origin`, or, on the TorrServer-compatible routes only, Lampa's web origins: `https://lampa.mx` and `https://lampa.app` anywhere, and their plain `http://` forms only on listeners that face the home network, since plain HTTP can be faked on the way.
- **The `null` origin** of packaged TV apps is accepted on the compatibility routes only by listeners that face the home network: the TV access listeners, and `--listen` on a non-loopback address (the router build). Any web page can send `null` from a sandboxed frame, so the Mac app's loopback server refuses it. While TV access is on, a web page open on a device in the home network can therefore still reach the TV address, like the TV itself.
- **Adding torrents.** `GET /stream?link=` adds a torrent that isn't loaded yet, as in TorrServer. A browser request marked `Sec-Fetch-Site: cross-site` or `same-site` (an `<img>` or `<video>` tag on another site sends no `Origin`) may only stream the loaded torrent, never add one. Players and TV apps' native code send no such header.
- **`/api/*` from other sites.** Requests marked `Sec-Fetch-Site: cross-site` or `same-site` are refused there, so another site's `<video>` or `<img>` can't take a reader slot or learn what's playing.
- **Frames.** Every response says `X-Frame-Options: DENY` and `Content-Security-Policy: frame-ancestors 'none'` (and `X-Content-Type-Options: nosniff`), so no site can show the control page in a frame and steer a click onto its switches.
- Approved preflights get the private-network header; browser and OS permission prompts still apply.
- `POST /api/network`, which turns TV access on or off, accepts loopback connections only.
- TV access listens only on private IPv4 addresses, skipping loopback, VPN, tunnel, container, Internet Sharing and virtual machine interfaces. By design it has no password.
- The Mac app lets the page navigate only to the server's own origin, opens only the server's own stream addresses in a player, opens external links only from a fixed list of GitHub pages, and accepts native messages only from its own main frame (see [ui.md](ui.md#the-mac-window)). It starts the server with `--exit-on-stdin-close` and a pipe, so the server exits with the app even after a crash or Force Quit.

## Design decisions

- **One active torrent.** All playback shares one RAM budget, so adding another torrent requires stopping the current one.
- **No disk at all.** No files, no database and no DHT state. Quitting or stopping always returns the memory.
- **Bounded everything.** Every queue, buffer and table has a fixed limit, so a large or hostile torrent can't grow memory.
- **The cache is a single `mmap`.** Unmapping returns the pages to the OS at once instead of leaving them in an allocator's arenas.
- **Peer speeds are computed in the browser** from byte counters and a monotonic timestamp. The server keeps no sampler and no history.
- **No frontend framework or bundler.** `ui/` is served as it is, from inside the binary.
