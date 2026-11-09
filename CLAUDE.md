# TorrStream Lite

[README.md](README.md) is for people using the app. Building, testing and releasing are in [docs/development.md](docs/development.md). Implementation notes are in [docs/](docs/README.md): architecture, UI, the Keenetic build, and [todo.md](docs/todo.md) for known gaps and ideas.

## Commands

- `sh scripts/cargo.sh test --locked`, `sh scripts/cargo.sh clippy --locked --all-targets -- -D warnings`, `sh scripts/cargo.sh fmt --all`: unit tests, lints and formatting
- `sh scripts/build-macos.sh`: the Mac app in `dist/`, and the release server binary the Python tests use
- `python3 tests/e2e.py --size-mib 32 --corrupt-first`, `python3 tests/e2e.py --size-mib 32 --stalling-peer`, `python3 tests/lampa.py`, `python3 tests/network.py`: streaming, Lampa and TV access tests against a local BitTorrent peer (add `--binary target/debug/torrstream-lite` after a debug build)
- `python3 tests/release.py`, `node tests/peer-rates.cjs`: release helper and peer speed tests
- `npx prettier@3.6.2 --write ui/` and `xcrun swift-format format --in-place macos/App.swift`: formatting for the page and the Mac app
- `xcrun swift scripts/make-icon.swift`: regenerate `macos/AppIcon.icns` from `app-icon.svg`
- `python3 scripts/screenshots/make.py`: render the README and website screenshots from `ui/`

## Working rules

- **RAM only.** Never write torrent data or metadata to disk. Keep every allocation bounded by the cache budget and the documented limits, never by torrent size (see [docs/architecture.md](docs/architecture.md#memory)).
- **Tests never use public swarms.** Use the local peer in `tests/`.
- `ui/` is plain HTML/CSS/JS compiled into the server with `include_str!`. Don't add a framework or build step, and rebuild after editing it. The oldest engine it runs in is WKWebView on macOS 12 (Safari 15): no container queries, no `color-mix()`.
- Set server and torrent text (file names, errors, peer clients) with `textContent`, never `innerHTML`.
- The design follows Unified Mail's (tokens, type scale, outlined buttons, card dialogs); see [docs/ui.md](docs/ui.md). The UI calls TV access **Home network → Allow other devices**; code, `UserDefaults` keys, `/api/network` and `--tv-port` keep the old name. `app-icon.svg` and the sidebar logo in `ui/index.html` are the same drawing; change both.
- Lampa's origins are allowed only on the TorrServer-compatible routes, and the `null` origin only there and only on listeners that face the home network. Keep the Host check and the cross-site rule for adding torrents (see [docs/architecture.md](docs/architecture.md#security)). Changing TV access (`POST /api/network`) stays loopback-only.
- Changes to the engine go in `vendor/rqbit` and get a line in [vendor/UPSTREAM.md](vendor/UPSTREAM.md).
- Errors returned to the UI are shown as they are; write them for people.
- Commits use the author set in this repo's git config. Never add `Co-Authored-By` lines or any other Claude/AI attribution to commit messages, pull requests or release notes.
- `README.md` stays user-facing (download, first launch, use, privacy). Developer details go in `docs/`.
- The website's pages live in the unified-mail repo, `site/torrstream-lite/` ([docs/development.md](docs/development.md#website)). After a visible UI change, render the screenshots again and copy them there; when what the app connects to or keeps changes, update its privacy policy.
- When behavior changes, update the matching file in `docs/`. Add things you notice but don't do to `docs/todo.md`.
- Every change people using the app would notice gets a line under **Unreleased** in [CHANGELOG.md](CHANGELOG.md), written for them (what changed for them, not how), in the same commit. The Release workflow turns those lines into the release's notes. Changes only to docs, tests or the release tooling don't go there.
