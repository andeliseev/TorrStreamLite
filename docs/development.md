# Development

Run the commands below from the repository root.

## Requirements

- Rust: tested with 1.99.1; `Cargo.toml` asks for 1.94 or later. `scripts/cargo.sh` uses a project-local toolchain under `.build/` when there is one, and the system's otherwise. Dependency versions are pinned in `Cargo.lock`.
- macOS with the Xcode Command Line Tools, for the Mac app. It's built for the Mac's own architecture and runs on macOS 12 or later.
- Python 3.11 or later, for the tests and the release helper. Node.js, only for `tests/peer-rates.cjs`.
- FFmpeg, only for the optional 4K video test.

## Build the Mac app

```sh
sh scripts/build-macos.sh
```

Open **`dist/TorrStream Lite.app`**. Quit and reopen the app after rebuilding, to load the new server. The build also updates the app's date and registers it with macOS, which keep Finder and the Dock from showing an old, cached icon. If the Dock still shows one, restart it with `killall Dock`. The files in `ui/` are compiled into the server, so rebuild after editing them too. The app is ad-hoc signed, not notarized. The window, the player and the video decoder use memory of their own, apart from the server memory the app shows.

Packaging a DMG (`sh scripts/package-macos.sh`, into `release-assets/`) and the Keenetic build ([router.md](router.md)) are separate steps.

To run only the server, for example to work on the control page in a browser:

```sh
sh scripts/cargo.sh run -- --listen 127.0.0.1:8090
```

Then open <http://127.0.0.1:8090>.

## Tests

Before you push:

```sh
sh scripts/cargo.sh fmt --all
sh scripts/cargo.sh clippy --locked --all-targets -- -D warnings
sh scripts/cargo.sh test --locked
python3 tests/release.py
node tests/peer-rates.cjs
python3 scripts/notices.py --check
```

The engine's own unit tests, in `vendor/rqbit`, aren't part of `cargo test`. They run in the engine's workspace, with its own `Cargo.lock` and the features the app uses. They cover librqbit, where all the local changes are, and stay on the local machine:

```sh
sh scripts/cargo.sh test --locked --manifest-path vendor/rqbit/Cargo.toml -p librqbit --no-default-features --features rust-tls,disable-upload --lib --target-dir target/rqbit-tests
```

When the engine's manifests change, update its lockfile with the same command without `--locked`, check that the change to `vendor/rqbit/Cargo.lock` is only what the manifests ask for, and commit it.

The streaming tests start a real server against a local BitTorrent peer that serves generated data. They use `target/release/torrstream-lite`, which `scripts/build-macos.sh` builds; after a debug build (`sh scripts/cargo.sh build --locked`), add `--binary target/debug/torrstream-lite`.

```sh
python3 tests/e2e.py --cache-mib 6 --size-mib 64
python3 tests/e2e.py --cache-mib 6 --size-mib 64 --corrupt-first --report test-results/corruption-recovery.json
python3 tests/e2e.py --cache-mib 6 --size-mib 64 --stalling-peer --report test-results/stalling-peer.json
python3 tests/e2e.py --cache-mib 16 --size-mib 48 --peers 12 --peer-mbps 10 --report test-results/many-peers.json
python3 tests/e2e.py --cache-mib 96 --size-mib 256 --rate-mbps 80 --report test-results/router-profile.json
python3 tests/lampa.py
python3 tests/network.py
```

To try a real 4K video, generate one with FFmpeg and offer it to the app through a local peer:

```sh
ffmpeg -f lavfi -i 'testsrc2=size=3840x2160:rate=24' -t 12 -c:v libx264 -preset ultrafast -crf 22 -pix_fmt yuv420p -movflags +faststart -an /tmp/torrstream-lite-4k-test.mp4
python3 tests/serve_fixture.py --video /tmp/torrstream-lite-4k-test.mp4
```

Paste the printed torrent URL into the app. The test peer reads the video from disk; **TorrStream Lite itself still keeps everything it receives in RAM**.

### What the tests cover

The local peer speaks the real BitTorrent protocol and never contacts public swarms. The engine's DHT is turned off in these tests.

- Whole files larger than the cache, with every byte checked
- Backward and forward seeks, suffix ranges, overlapping head and tail requests, and pieces that span files
- Magnet metadata exchange, rejecting and recovering from a corrupted piece, and bounded allocation
- A second peer that unchokes the engine and then answers nothing, as a peer that chokes it mid-piece does: the other peer takes over its pieces, and no read waits 15 s or more
- Twelve peers limited to 10 Mbps each: they all send at once, faster than four could
- One torrent at a time, invalid ranges, and HEAD requests that download nothing
- Stop during a slow HTTP response, cache teardown, and no files written by the server
- TV access: a reachable LAN listener, range streaming and LAN playlist links, refusing settings changes from the network, the opaque `null` origin of TV apps (accepted there, refused on the loopback server), stopping TV streams without stopping Mac playback, invalid ports and port conflicts
- Lampa's sequence: ping, form-labelled JSON from Lampa's web origin, adding by torrent URL, file selection, delayed preload progress, cache statistics, playback and seeking, watched positions and teardown; and that Lampa can't change TV access and other browser origins are refused
- Requests addressed to unknown host names (DNS rebinding) are refused, and a cross-site stream URL can't add a torrent
- Decoding a generated 3840×2160 H.264 clip through the Mac server: all 288 frame hashes and a seek match the original

Machine-readable results go to `test-results/` (not committed). The native window, a physical TV and the router still need manual checks.

### Measurements

On an Apple Silicon Mac, November 8, 2026, with the local peer:

| Test | Result |
| --- | --- |
| 256 MiB at 80 Mbps, 96 MiB cache | 110.81 MiB peak server memory |
| The same, after Stop | 14.59 MiB; cache released |
| 32 MiB with a 6 MiB cache | 19.02 MiB peak; never over the cache budget |
| Generated 4K clip through the bundled server | 288 matching decoded frames; the seek matched |
| Control page: add a torrent, then Stop | Server memory from 98 to 16 MiB |

These aren't router benchmarks or a guarantee for Internet swarms, and there's no measured comparison with TorrServer yet.

## Third-party notices

[`THIRD_PARTY_NOTICES.txt`](../THIRD_PARTY_NOTICES.txt) holds the license texts and copyright notices of every crate compiled into the server, for the Mac app (`aarch64-apple-darwin`) and the Keenetic build (`aarch64-unknown-linux-musl`). The server shows it in **Settings → Third-party notices** (`/licenses/third-party`), and the Mac app and the router tarball include it. It's generated; don't edit it by hand. After changing dependencies, write it again and commit it with the change:

```sh
python3 scripts/notices.py
```

[`scripts/notices.py`](../scripts/notices.py) runs [cargo-about](https://github.com/EmbarkStudios/cargo-about) 0.9.3 with [`about.toml`](../about.toml) to find each crate's license files, and `cargo tree` for both targets to keep only the crates linked into the server: not the procedural macros and build scripts that only run during the build. It writes each license text once, under the crates it covers. It needs network access for the crates' sources, and cargo-about, once:

```sh
sh scripts/cargo.sh install cargo-about --locked --version 0.9.3 --features cli
```

The Release workflow runs `python3 scripts/notices.py --check`, which fails when the file isn't what the script would write. The script also stops when a linked crate has no license text of its own, which would leave out its copyright notice:

- **A license file the scanner misreads** (cargo-about falls back to the license's standard text): add a clarification for that file to `about.toml`, with the SHA-256 of the text it uses (`shasum -a 256 <file>` for a whole file; the part between `start` and `end` when it uses one). The checksum makes the clarification stop applying when a new version of the crate changes the file; check the file again then.
- **A crate that ships no license file:** take it from the crate's repository, at the commit in the published crate's `.cargo_vcs_info.json`, put it in `licenses/` and add it to `MISSING_LICENSES` in the script. The vendored engine's crates use upstream's notice, `vendor/rqbit/LICENSE`, and the Apache-2.0 text in `licenses/Apache-2.0.txt`.
- **A license that isn't in `accepted`** in `about.toml`: decide whether the app can include that crate. The list is in order of preference, so a crate that offers several licenses is listed under the first one it offers.

## Project layout

| Folder | Contents |
| --- | --- |
| `src/` | Rust server: HTTP API, RAM cache, TV access |
| `ui/` | Control page; compiled into the server |
| `macos/` | Native Mac window and `Info.plist` |
| `scripts/` | Mac build and DMG, Keenetic build, Rust toolchain helper, release helper, app icon, screenshots |
| `tests/` | Local torrent peer, streaming, memory, network, Lampa and release tests |
| `vendor/` | The pinned librqbit engine with local changes ([UPSTREAM.md](../vendor/UPSTREAM.md)) |
| `licenses/` | License texts that crates don't ship themselves, for the [third-party notices](#third-party-notices) |
| `docs/` | These notes |
| `.github/workflows/` | The Release workflow |

`.build/` and `target/` hold the local toolchain and build caches. `dist/`, `release-assets/` and `test-results/` hold generated files. All of them are ignored by Git.

## App icon

The icon is drawn in [`app-icon.svg`](../app-icon.svg), built like Unified Mail's: a white play mark with three motion lines on a green tile with a wide rim. The same drawing is the sidebar logo in `ui/index.html`; change both together. To regenerate `macos/AppIcon.icns` (needs macOS 14 or later, which reads SVG):

```sh
xcrun swift scripts/make-icon.swift
```

## Screenshots

The README and the website show [`docs/images/screenshot-light.png`](images/screenshot-light.png) and [`screenshot-dark.png`](images/screenshot-dark.png). They're the real `ui/` files rendered in WebKit, like in the Mac app, with the sample data in `scripts/screenshots/mock.js`. After a visible change to the UI, render them again and copy them to the website:

```sh
python3 scripts/screenshots/make.py
```

With Pillow installed (`pip install pillow`) they're reduced to 256 colors, about a third of the size.

## Website

The home page, privacy policy and support page are at <https://andeliseev.github.io/torrstream-lite/>. Their source is in the [unified-mail](https://github.com/andeliseev/unified-mail) repository, in `site/torrstream-lite/`, next to Unified Mail's pages: its `site/` folder is the whole of andeliseev.github.io. After changing them there, commit on its own (the message becomes public in the site's history) and publish from unified-mail:

```sh
git subtree push --prefix=site https://github.com/andeliseev/andeliseev.github.io.git main
```

If what the app connects to or keeps changes, update `privacy.html` and its effective date.

## Code formatting

Rust uses `cargo fmt`, Swift uses Xcode's `swift-format`, and the control page uses Prettier 3.6.2. Prettier is only a formatter: nothing in the build needs Node.

```sh
sh scripts/cargo.sh fmt --all
xcrun swift-format format --in-place macos/App.swift
```

## Releases (GitHub Actions)

[`.github/workflows/release.yml`](../.github/workflows/release.yml) tests, builds and publishes a release in one click. It needs no secrets: it uses the repository's built-in `GITHUB_TOKEN`. Repository rules must let GitHub Actions push to `main`, for the release commit.

**To release:** **Actions → Release → Run workflow**, choose what to raise and what to build, and run it:

| Choice | From 0.2.2 | Use for |
| --- | --- | --- |
| `patch` (default) | 0.2.3 | Fixes |
| `minor` | 0.3.0 | New features |
| `major` | 1.0.0 | Big changes |

Or type an exact version (e.g. `0.3.0`) in the second box; it wins over the choice. Versions are plain `X.Y.Z`.

The third box picks the downloads: **macOS** (default), **macOS and Keenetic**, or **Keenetic**. The README's Download link points to the latest release, so after a one-platform release the other platform's file isn't there until a release includes it again. Older releases still have it.

To see what a run would release:

```sh
python3 scripts/release.py next --bump patch
```

It prints the last release and the next version. The last release is the highest `vX.Y.Z` tag on GitHub (`git ls-remote --tags origin` lists them). The run is named "Release (patch, macOS)" or "Release (0.3.0, macOS)" in the Actions list, and its summary page names the version.

The workflow then:

1. **Tests:** formatting, Clippy, unit tests, the release helper and peer speed tests, the [third-party notices](#third-party-notices) and the engine's own tests, then the streaming, Lampa and TV access tests against a local peer, on Linux. If anything fails, nothing is committed or built.
2. Works out the version with `scripts/release.py next`. It stops if that isn't newer than the last release, or is lower than the version already in `Cargo.toml`.
3. Sets it in `Cargo.toml`, `Cargo.lock` and `macos/Info.plist` (`scripts/release.py set`). Each job does this in its own checkout; nothing is committed yet.
4. Moves the **Unreleased** lines of [`CHANGELOG.md`](../CHANGELOG.md) under `## X.Y.Z (date)` (`scripts/release.py changelog`), leaving an empty Unreleased section for the next changes. With nothing under Unreleased, the run shows a warning and the release has no "What's new".
5. Creates a draft release `vX.Y.Z` whose text starts with **What's new** (those changelog lines), replacing a draft left by a failed run.
6. Builds the chosen platforms in parallel, with the new version, and uploads them to the draft, each with a `.sha256` file:
   - **macOS:** builds the app on an Apple Silicon runner, verifies its signature, runs the Lampa test against the server inside the app, and packages `TorrStream-Lite-X.Y.Z-macos-arm64.dmg`.
   - **Keenetic:** cross-compiles a static ARM64 Linux binary with `cargo-zigbuild`, checks that it's static ARM64, and packs `TorrStream-Lite-X.Y.Z-linux-arm64.tar.gz` with `LICENSE` and `THIRD_PARTY_NOTICES.txt`.
7. Only if every build succeeded: commits the version and changelog changes as "Release vX.Y.Z" to `main`, authored by whoever ran the workflow (with their GitHub noreply address, so no bot appears among the contributors), and publishes the release with its `vX.Y.Z` tag on that commit. Run `git pull` afterwards, before you commit locally.

**If a run fails,** fix the cause and run it again with the same choice. A failed test or build leaves `main` as it was, and the tag doesn't exist yet, so the same version comes out and its draft is replaced. If something was pushed to `main` while the release ran, the last step stops rather than release a commit that wasn't built; run it again. If the release commit was pushed but the release wasn't published, the next run reuses that commit and the changelog's notes.

**Before releasing,** check the Unreleased lines in `CHANGELOG.md`: they become the release notes as written. Don't set the version locally: the workflow does it.

**Notes:**

- There's no separate workflow for every push. The Release workflow runs the same tests before it builds anything, so run the [tests](#tests) locally before you push, and a broken `main` is caught at the latest when you release. The Swift app and the macOS-only Rust code are first compiled in the macOS build, which still runs before anything is committed.
- On a private repository, Actions minutes count against the account's monthly allowance, and macOS minutes count ten times. Each macOS release builds the app once. On a public repository the minutes are free.
- The Mac app is ad-hoc signed and not notarized. The release notes explain **Open Anyway**.

## Manual test checklist

- [ ] First launch from the DMG on a Mac that never had the app: Gatekeeper's Open Anyway, the window opens, Idle, server memory around 15 MiB
- [ ] Add a magnet link with reachable peers: the files appear within a minute
- [ ] **Play**: VLC (or IINA) starts; seek forward and back; speed and cache update, and the status says Streaming
- [ ] **Download**: peers appear with speeds; after closing the panel, `/api/peers` isn't requested any more
- [ ] **Stop & release RAM**: server memory drops back near idle
- [ ] Add a second torrent while one is loaded: a clear message to stop the first
- [ ] Turn on **Allow other devices**: the address appears with port 8090; Lampa on the TV connects, lists the files, plays and seeks
- [ ] Turn it off while the TV plays: the TV stream stops, Mac playback continues
- [ ] Change the port in Settings, quit and reopen: the switch and the port are kept
- [ ] Settings → Appearance: System, Light and Dark switch the page and the titlebar; the choice is kept after reopening
- [ ] Drag the window by its top edge; double-click there to zoom. Resize it: the sidebar keeps its width and the file list takes the rest; quit and reopen: size and position are kept
- [ ] ⌘O adds a .torrent file; so does dropping one on the window, and pasting a magnet link outside the text field
- [ ] Menus: About shows the icon and version; ⌘W, ⌘M and ⌘H work
- [ ] Quit while a magnet is still resolving: no `torrstream-lite` process is left (Activity Monitor)
