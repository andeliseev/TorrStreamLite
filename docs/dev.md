# Development and testing

Run commands below from the repository root. [Back to the app guide](../README.md).

## Build the Mac app

Requires a recent Rust toolchain (tested with Rust 1.99.1). `scripts/cargo.sh` uses the project-local toolchain under `.build/` when present, otherwise the system toolchain. Rust dependency versions are in `Cargo.lock`.

The Mac build also requires Xcode's command-line tools and targets the Mac's native architecture.

```sh
sh scripts/build-macos.sh
```

Open **`dist/TorrStream Lite.app`** in Finder. Quit and reopen the app after rebuilding to load the new binary. This command builds only the app; DMG packaging and the router build are separate optional steps. The app is ad-hoc signed, not notarized.

The native window uses Swift/WebKit and launches the bundled Rust server on an available loopback port. The window, player, and decoder use memory separately from the server reading shown in the app.

## Test

```sh
sh scripts/cargo.sh test --locked
sh scripts/cargo.sh clippy --locked --all-targets -- -D warnings
python3 tests/network.py
python3 tests/e2e.py --cache-mib 6 --size-mib 64
python3 tests/e2e.py --cache-mib 96 --size-mib 256 --rate-mbps 80 --report test-results/router-profile.json
python3 tests/e2e.py --cache-mib 6 --size-mib 64 --corrupt-first --report test-results/corruption-recovery.json
```

To generate a real 4K test video with FFmpeg and offer it to the app through a local torrent peer:

```sh
ffmpeg -f lavfi -i 'testsrc2=size=3840x2160:rate=24' -t 12 -c:v libx264 -preset ultrafast -crf 22 -pix_fmt yuv420p -movflags +faststart -an /tmp/torrstream-lite-4k-test.mp4
python3 tests/serve_fixture.py --video /tmp/torrstream-lite-4k-test.mp4
```

Paste the printed torrent URL into the app. The test peer reads the generated video from disk; **TorrStream Lite itself still stores all received video in RAM**.

## Project layout

| Folder | Contents |
|---|---|
| `src/` | Rust streaming server, bounded RAM cache, and TV access |
| `ui/` | Control page HTML, CSS, and JavaScript; embedded in the server |
| `macos/` | Native Mac window and app metadata |
| `scripts/` | Mac build, optional router build, and Rust toolchain helper |
| `tests/` | Local torrent, streaming, memory, and network checks |
| `vendor/` | Pinned Rust torrent engine, modifications, and license |
| `docs/` | Development notes and router setup |
| `.github/workflows/` | Automatic checks and manually started releases |

`.build/` and `target/` are local toolchain/build caches. `dist/`, `release-assets/`, and `test-results/` contain generated outputs. These folders and Python/macOS cache files are excluded from Git.

## Publishing releases

Open [Actions → Release](https://github.com/andeliseev/TorrStreamLite/actions/workflows/release.yml), choose **Run workflow** on `main`, then select a version bump and **macOS**, **Keenetic**, or **macOS and Keenetic**. An optional exact version overrides the bump. The first release uses the source version unless overridden.

The workflow runs the checks, commits any version update, creates a draft, and builds an Apple Silicon DMG and/or a compressed static Linux ARM64 executable with SHA-256 checksum files. It publishes the release only after all selected builds succeed. It uses the repository's built-in `GITHUB_TOKEN`; no extra release secret is required. Repository rules must allow Actions to write `main` for automatic version updates. A failed build leaves a draft; start a new run for the same unpublished version to replace that draft and rebuild its assets.

Pushes to `main` and pull requests run **Checks** without publishing a release. The Mac app is ad-hoc signed and is not notarized; Keenetic builds still need testing on hardware.

## Memory behavior

- One shared, anonymous RAM mapping, **96 MiB by default**. No video files, disk cache, session database, or DHT persistence.
- Reserve a reusable piece slot **before issuing network requests**. At most four pieces download/verify concurrently. No payload allocation grows with torrent size.
- Download only the windows requested by active HTTP readers. No background whole-file download.
- Keep readers' windows and pieces undergoing verification protected. Eviction clears the engine's HAVE and per-file counters, so backward seeks re-download correctly.
- Cancel obsolete requests after seeks; discard late replies safely. Reuse slots after writers finish. Reads and eviction coordinate through the same engine lock.
- At most 20 connected/connecting torrent peers by default, 16 outstanding block requests per peer, four metadata connections, and 256 discovered peer candidates.
- Up to four simultaneous HTTP requests for one playback session, reduced when pieces are large. Each response has one 64 KiB queued chunk and a bounded producer buffer.
- Stop cancels slow readers and unmaps cache pages. An idle torrent is removed after five minutes (`--idle-seconds 0` disables this).

The cache limit is **not a hard cap on total RSS**. Torrent metadata, network buffers, DHT, TLS, runtime threads, and the OS still need memory. The initial server target is under 192 MiB; confirm this with real swarms and the router's normal traffic.

Pieces must be at least 16 KiB, and the cache must fit at least three torrent pieces. With 96 MiB, pieces up to 32 MiB fit. Large-piece torrents get a clear error instead of exceeding the budget. Metadata is limited to 2 MiB, 131072 pieces, 4096 files, and 512 GiB total torrent content.

## Verified locally

The automated local peer generates deterministic data and speaks the real BitTorrent protocol. It does not contact public swarms. Tests cover:

- Whole files larger than cache, with every byte checked.
- Backward/forward seeks, suffix ranges, overlapping head/tail requests, and cross-file pieces.
- Magnet metadata exchange, corrupted-piece rejection/recovery, and bounded allocation.
- Single-torrent admission, invalid ranges, and HEAD requests without downloads.
- Stop during a slow HTTP response, cache teardown, and no server files written.
- Decoding a generated 3840×2160 H.264 clip through the Mac server: all 288 frame hashes and a seek match the original.

Machine-readable measurements are in `test-results/`. These are Mac/local-peer measurements, not router benchmarks or guarantees for Internet swarms. Native Mac window interaction requires manual verification; the server can be tested independently.

Version 0.2 adds LAN integration coverage: a reachable LAN listener, range streaming and LAN playlist links, rejection of remote settings changes, cancellation of TV streams without stopping Mac playback, invalid-port handling, and recovery from port conflicts. The displayed TV switch and address were also checked in the browser. The earlier Mac player handoff was confirmed by the user; native preference persistence and actual TV behavior remain manual checks.

Measurements from November 8, 2026 on this Apple Silicon Mac:

| Test | Result |
|---|---|
| 256 MiB transfer at 80 Mbps, 96 MiB cache | 110.81 MiB peak server RSS |
| Same test after stopping the torrent | 14.59 MiB server RSS; cache released |
| Generated 4K clip through the final bundled server | 288 matching decoded frames; seek matched |
| Control page | Add torrent and Stop tested; Stop reduced RSS from 98 to 16 MiB |

The automated peer tests use DHT disabled. The control-page test used the default DHT-enabled profile. Public-swarm behavior and router performance still need testing. The user confirmed native player launch in the earlier Mac build. There is no measured comparison against the original Go server yet.

For the first real playback test, quit and reopen the rebuilt Mac app, add one torrent, and choose **Open in player**. Watch the server-memory reading during playback and after seeking, then use **Stop & release RAM**. Test your intended 4K bitrate before moving to the router.

## API and compatibility scope

Native API:

| Request | Behavior |
|---|---|
| `GET /api/status` | RSS, cache accounting, torrent/files, engine stats |
| `GET /api/peers` | Connected peer counters, torrent session ID, monotonic sample time; no stored history |
| `GET /licenses/app`, `GET /licenses/third-party` | Offline MIT and engine license/attribution text |
| `GET /api/network` | TV addresses, port, and local-control availability |
| `POST /api/network` | Localhost only: JSON `{"enabled":true,"port":8091}`; no torrent restart |
| `POST /api/torrents` | JSON `{"link":"magnet:?..."}` or torrent URL |
| `POST /api/torrents/file` | Raw `.torrent` bytes |
| `DELETE /api/torrents` | Cancel playback and release the active torrent |
| `GET/HEAD /api/stream/{file}` | Stream a **1-based** file index, with one HTTP byte range |

Implemented TorrServer-style subset: `/echo`, `POST /torrents` actions `add/list/get/set/rem/drop`, `POST /settings` action `get`, `POST /cache` action `get` (summary; empty per-piece/reader details), `POST /viewed` actions `list/set/rem` (current torrent, RAM only), and `/stream?link=<hash-or-magnet>&index=1&play`. `/stream` also supports `stat`, `m3u`, and asynchronous `preload`. Preload warms up to 8 MiB or one quarter of the cache, whichever is smaller, using the existing slab and one 64 KiB read buffer. Playback or stop cancels an unfinished preload.

Unsupported: full TorrServer UI/settings API, persistent library/view history (`save_to_db` is not persisted), `torrs://`, DLNA, search providers, transcoding, full seeding, and multiple active torrents. BitTorrent v1/`btih` is the intended protocol; pure v2 torrents are outside this prototype. The current connection profile uses outgoing TCP peers.

Lampa's jQuery requests contain JSON but can label it as form data. Compatibility routes accept this format while retaining the 2 MiB request limit; native API routes keep strict JSON handling. Only compatibility routes allow the built-in `http(s)://lampa.mx`, `http(s)://lampa.app`, and `null` origins. Other browser origins require `--allow-origin <origin>`. Native network settings remain localhost-only. Approved preflights include the private-network response header; browser/OS permission prompts still apply.

Run the Lampa and release-helper regressions with:

```sh
python3 tests/lampa.py
python3 tests/release.py
node tests/peer-rates.cjs
```

The Lampa test uses a local BitTorrent peer and reproduces ping, form-labelled JSON, opaque TV origins, torrent URL add, file selection, delayed preload progress, cache statistics, playback/seek, viewed positions, and teardown. It also checks that Lampa cannot change native network settings and that unrelated browser origins are rejected. This does not test a physical TV's widget or decoder.

Version 0.2.1 validation: the bundled Mac server passed the Lampa sequence; the standalone server passed LAN access and the corrupted-piece/seek/teardown regression. The 32 MiB transfer with a 6 MiB cache peaked at 19.02 MiB server RSS and never exceeded the cache budget. Both Mac executables now declare macOS 12 as their minimum deployment target. The DMG packaging step was also exercised locally.

## Code formatting

Rust uses `cargo fmt`, and Swift uses Xcode's `swift-format`. The control page is formatted with Prettier 3.6.2. No Node dependencies or JavaScript build step are required to build or run the app.

```sh
sh scripts/cargo.sh fmt --all
xcrun swift-format format --in-place macos/App.swift
```

The files in `ui/` are embedded into the Rust binary at compile time, so rebuild the Mac app after editing them. `macos/Info.plist` contains the native app metadata.

The peer details endpoint returns only connected peers, bounded by the configured peer limit. The browser calculates rates from received-byte deltas and the server's monotonic sample time. New peers, reconnections, counter resets, torrent changes, and gaps longer than 10 seconds require a fresh baseline. The peer panel polls every 1.5 seconds while open and visible, cancels pending requests when closed/hidden, and keeps only two bounded snapshots. The server adds no periodic sampler or peer history.
