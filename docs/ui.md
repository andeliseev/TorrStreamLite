# UI

The control page is plain HTML/CSS/JS in `ui/`, compiled into the server with `include_str!` and served at `/`. There's no framework, no build step and no Node dependency. Browsers on the home network get the same page at the TV address, without the Mac-only parts. The design follows [Unified Mail](https://github.com/andeliseev/unified-mail): the same tokens, type scale, controls and dialogs.

| File | Contents |
| --- | --- |
| `index.html` | The sidebar (logo, status, statistics, home network, Settings), the main pane (torrent title, error banner, add card, file list), the Settings, Download and license dialogs, the drop target and the toast |
| `styles.css` | Theme tokens on `:root` (light), a dark palette under `prefers-color-scheme: dark` and `data-theme="dark"`, layout |
| `app.js` | Polling, adding and stopping torrents, home network access, dialogs, appearance |
| `peer-rates.js` | Per-peer download speeds from two counter snapshots |

The files are formatted with Prettier 3.6.2 (`npx prettier@3.6.2 --write ui/`, settings in `.prettierrc.json`). Rebuild the server or the Mac app after editing them. The oldest engine they run in is WKWebView on macOS 12 (Safari 15): no container queries and no `color-mix()`.

## Layout

`#app` is a two-column grid: a 236 px sidebar and the main pane, whose single row is `minmax(0, 1fr)` so the file list scrolls inside the window. All extra width and height go to the file list. File rows have fixed action columns, so the buttons line up from row to row; subtitles and other files get only Copy. Below 820 px the Play buttons drop their label, and below 680 px (phones, TV browsers) the sidebar folds into a bar above the files.

In the Mac app (`.desktop` on `<html>`) the page runs under a transparent titlebar. The app passes the titlebar's height as `--titlebar`, and the page keeps controls out of that band.

## State and feedback

While the page is visible it reads `/api/status` every 1.5 s and `/api/network` every 3 s. Each poll is skipped while the previous one is still running, and nothing is polled while the page is hidden. The status leaves out the file list: the page reads it from `/api/files` and rebuilds it only when the torrent's hash changes. Server text (file names, errors, peer clients) is always set with `textContent`.

The status in the sidebar is a dot with a word:

| Status | Dot | When |
| --- | --- | --- |
| Idle | gray | No torrent |
| Adding torrent… | amber, pulsing | This page or another client is adding one |
| Buffering… | amber, pulsing | A preload is running |
| Streaming | green, with a ring | An HTTP reader is active |
| Ready to play | green | A torrent is loaded |
| Torrent error, Server unavailable | red | The engine reports an error, or the server doesn't answer |

Errors show in one banner under the torrent's title; a dismissed engine error stays hidden until it changes. While a magnet link's details are on the way, the line under the title counts the peers found and those that didn't answer; if a torrent Lampa added doesn't load, the banner says why, since Lampa shows only its own timeout: whether peers were found and answered, and what the torrent's tracker said (a torrent the tracker no longer has was removed or replaced). Confirmations ("Stream URL copied") show in a toast. While a torrent is loaded the add card is hidden, because only one torrent can run.

A .torrent file dropped anywhere on the window is added, and so is a magnet link or torrent URL pasted outside a text field.

**Cache** (Settings) is 96, 192 or 256 MiB. The page sends it to `POST /api/cache`, which sizes the next torrent's cache, and the Mac app saves it and starts the server with it next time. Like the port, it shows only on the Mac itself.

**Appearance** (Settings) is System, Light or Dark. It sets `data-theme` on `<html>`; the Mac app also stores it and sets the window's appearance, and browsers keep it in `localStorage`.

## Peer speeds

`/api/peers` returns only the connected peers, up to the peer limit. The page calculates speeds from the received-byte differences and the server's monotonic sample time. New peers, reconnections, counter resets, a different torrent and gaps longer than 10 s start a fresh baseline. The dialog polls every 1.5 s only while it's open and visible, cancels pending requests when it closes, and keeps two snapshots at most. The server adds no sampler and keeps no history.

## The Mac window

`macos/App.swift` is the whole native app:

- It starts `Contents/Resources/torrstream-lite --listen 127.0.0.1:0 --cache-mib <size>` with the saved cache size (96 by default), waits for the `TORRSTREAM_LISTEN=` line and loads that address. Until then a startup screen in the window's colors shows.
- The window has a transparent titlebar over the page (`fullSizeContentView`). A transparent view covers the titlebar band, so dragging there moves the window and a double-click zooms or minimizes it, as set in System Settings. The window and the web view's background match the page, so resizing doesn't flash. The size and position are remembered; the minimum is 720 × 520.
- Navigation is allowed only to the server's origin, and never back or forward; the context menu has no Back, Forward or Reload. `torrstream-player://play?url=…` links open the stream in VLC, or in IINA when VLC isn't installed, and only when the URL is the server's own stream address. If WebKit's page process ends, the page loads again.
- It starts the server with `--exit-on-stdin-close` and keeps a pipe on its standard input: when the app ends, however it ends, the server exits too.
- Menus: About (the standard panel), Settings… (⌘,), Hide (⌘H), Quit; File → Open Torrent File… (⌘O) and Close Window (⌘W); Edit; Window → Minimize (⌘M) and Zoom.
- The web view's data store is non-persistent, so the page's cache and storage stay in memory.
- Settings are saved in `UserDefaults` under the bundle identifier `com.andeliseev.torrstream-lite`, and the server inside the app is signed as `com.andeliseev.torrstream-lite.server`. Versions up to 0.2.3 used `local.torrstream-lite.mac`; at its first launch the app moves those settings over and deletes what WebKit kept on disk for that version.

At document start it sets these globals for the page, from the saved settings. The script is rebuilt whenever a setting is saved, so a page that loads again never gets an old value:

| Global | Contents |
| --- | --- |
| `window.torrstreamDesktop` | `true` |
| `window.torrstreamTV` | The saved home network switch and port, which the page applies to the server |
| `window.torrstreamTheme` | The saved appearance |
| `--titlebar` (CSS variable) | The titlebar's height |

It calls `window.torrstreamOpenSettings()` for Settings… and `window.torrstreamAddTorrent(base64)` with the file chosen by ⌘O, so progress and errors show in the page.

The page sends messages to `window.webkit.messageHandlers.desktop`. They're accepted only from the app's own main frame and server origin:

| Action | Fields | Effect |
| --- | --- | --- |
| `openExternal` | `url` | Opens the URL in the browser if it's one of the project's GitHub pages (a fixed list) |
| `saveTVSettings` | `enabled`, `port` | Saves the home network switch and port in `UserDefaults`, for the next launch |
| `saveCacheSize` | `mib` | Saves the cache size (4 to 256) in `UserDefaults`; the server starts with it next time |
| `setTheme` | `theme` | Saves the appearance and sets `NSApp.appearance` |
| `copy` | `text` | Copies up to 8,192 characters to the clipboard |

The UI calls TV access **Home network → Allow other devices**; the code, the `UserDefaults` keys, `/api/network` and `--tv-port` keep the old name.
