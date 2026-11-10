# <img src="app-icon.svg" width="36" height="36" align="top" alt=""> TorrStream Lite

Stream torrents to your Mac or your TV without saving anything to disk. TorrStream Lite keeps the video in a small RAM cache and frees it as soon as you stop.

- **Paste a magnet link or a torrent URL, or drop a .torrent file on the window,** and play any file right away in VLC or IINA, or in another player through its stream address
- **Light on memory, nothing on disk:** the video goes through a 96 MiB cache, whatever the size of the torrent
- **Seek anywhere:** only the parts you watch are downloaded
- **Watch on your TV** with Lampa or another app made for [TorrServer](https://github.com/YouROK/TorrServer). TorrStream Lite answers them like TorrServer does, on the same port
- **Or without a Mac:** an experimental build runs [on a Keenetic router](#on-a-keenetic-router) and serves the TV by itself
- **See the download** as it happens: speed, cache use and every connected peer
- Light and dark appearance

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/images/screenshot-dark.png">
  <img src="docs/images/screenshot-light.png" alt="TorrStream Lite playing an episode of a series, with the download speed, cache, memory and home network address in the sidebar">
</picture>

Website: [andeliseev.github.io/torrstream-lite](https://andeliseev.github.io/torrstream-lite/)

## Download

Get the latest version from the **[Releases page](https://github.com/andeliseev/TorrStreamLite/releases/latest)**. What changed in each version is in the [changelog](CHANGELOG.md).

| System | File |
| --- | --- |
| macOS 12 or later, Apple Silicon | `TorrStream-Lite-<version>-macos-arm64.dmg` |
| Keenetic router (ARM64 with Entware), experimental | `TorrStream-Lite-<version>-linux-arm64.tar.gz` |

Each file has a `.sha256` checksum next to it. Not every release has the router's file: take it from the newest release that lists it. The Mac's steps are below; the router's are in [On a Keenetic router](#on-a-keenetic-router).

### First launch

TorrStream Lite isn't notarized by Apple yet (that takes a paid Apple developer membership), so the first time you open it macOS says *"Apple could not verify 'TorrStream Lite' is free of malware that may harm your Mac or compromise your privacy."* To open it anyway:

1. Drag **TorrStream Lite** from the disk image to **Applications**, and open it from there.
2. When the message appears, click **Done**.
3. Open **System Settings → Privacy & Security** and scroll down. Next to *"TorrStream Lite" was blocked to protect your Mac*, click **Open Anyway**. The button shows for about an hour after you tried to open the app.
4. Click **Open Anyway** again and confirm with your password or Touch ID.

macOS remembers this for the copy you allowed; a new version asks again. If you use Terminal, this command does the same:

```sh
xattr -dr com.apple.quarantine "/Applications/TorrStream Lite.app"
```

The app is open source, and you can check that a download is the one GitHub built: `shasum -a 256` on the file must print the same value as its `.sha256` file.

If macOS asks whether TorrStream Lite may find devices on your local network, click **Allow**. Without it, your TV can't connect.

## Watching on your Mac

1. Open **TorrStream Lite**, paste a magnet link or a torrent URL and click **Add**. Or drop a .torrent file on the window, or press ⌘O.
2. Click **Play** next to a file. It opens in VLC or IINA. For another player, click the copy button next to it and open that address in the player.
3. When you're done, click **Stop & release RAM**. One torrent plays at a time, so stop it before you add the next one.

**Download** in the sidebar shows the connected peers and their speeds. **Settings** (⌘,) has the home network port, the cache size, the appearance (System, Light or Dark), cache details and information about the app.

## Watching on your TV

1. Keep the Mac awake and on the same home network as the TV.
2. In the sidebar, under **Home network**, turn on **Allow other devices**. The app shows the address to use, for example `http://192.168.1.35:8090`.
3. In Lampa, open **Settings → TorrServer** and enter that address. Other TorrServer apps have a similar setting.
4. Choose a torrent in Lampa and play it.

**Allow other devices** is off by default. While it's on, anything on your home network can add, play and stop torrents without a password, including web pages open on its devices, so turn it off when you're done watching. Nothing is reachable from the Internet, and no port forwarding is needed.

The address uses port 8090, like TorrServer. If TorrServer itself runs on the same Mac, choose another port in **Settings**.

## On a Keenetic router

The server also runs on a Keenetic router by itself, so the TV plays through Lampa with no Mac switched on. This is experimental: it has run on one model so far, a Keenetic Ultra (KN-1811), where a 4K film took about 112 MiB of the router's memory and a tenth of one processor core.

It needs a Keenetic with an ARM64 processor (many models have MIPS processors and can't run it) and Entware, the package system Keenetic supports for extra programs. In short:

1. Add Entware to the router. Keenetic's component for it updates the router's system and restarts it.
2. Copy the server, one file of about 10 MB, to the router over SSH, and start it by hand to see that it works.
3. Copy its start script, so that it starts whenever the router boots.
4. In Lampa, open **Settings → TorrServer** and enter the router's address, such as `192.168.1.1:8090`.

**[The router guide](docs/router.md)** has every step with its commands: [installing](docs/router.md#install), [checking that it works](docs/router.md#check-that-it-works), [switching it off and on](docs/router.md#switch-it-off-and-on), [updating](docs/router.md#update), [uninstalling](docs/router.md#uninstall), and [what to do when something fails](docs/router.md#if-it-doesnt-work). It also says [what the install changes on the router](docs/router.md#what-it-changes-on-the-router): two files in Entware, and nothing in the router's own system.

The page at the router's address, such as `http://192.168.1.1:8090`, is the Mac app's window without its Mac-only settings, for any computer or phone in the home network. There's no password, so anything in the home network can add, play and stop torrents; nothing is reachable from the Internet.

## If something doesn't work

| What you see | What to do |
| --- | --- |
| macOS says it can't verify the app | Follow [First launch](#first-launch) |
| **Play** says to install VLC or IINA | Install one of them, or click the copy button next to the file and open that address in your player |
| A magnet link doesn't load | The window says what it found: how many peers, how many of them answered, and what the torrent's tracker said. If the tracker no longer has the torrent, or no peer answers, look for another release of it |
| The TV app can't connect | **Allow other devices** must be on, and the address in the TV app the one the sidebar shows. The Mac must be awake and in the same network as the TV, and macOS must allow the app to find devices on the local network (**System Settings → Privacy & Security → Local Network**) |
| **Home network** says it cannot open the address | Something else on the Mac uses port 8090, such as TorrServer. Choose another port in **Settings**, and enter the same in the TV app |
| Something on the router | See [If it doesn't work](docs/router.md#if-it-doesnt-work) in the router guide |

For anything else, [open an issue](https://github.com/andeliseev/TorrStreamLite/issues).

## Uninstalling

**On a Mac:** quit TorrStream Lite and move it from **Applications** to the Trash. It leaves nothing on the disk but its settings, and this command in Terminal removes those:

```sh
defaults delete com.andeliseev.torrstream-lite
```

**On a router:** stop the server and delete its two files, as [Uninstall](docs/router.md#uninstall) in the router guide shows. That section also removes Entware, if you added it only for this.

## Memory

TorrStream Lite started as a streaming server small enough to run on a home router, and the Mac app keeps that design:

- **One cache, always the same size.** The video goes through a single block of RAM, 96 MiB unless you choose 192 or 256 MiB in **Settings → Cache**, whether the torrent is 1 GB or 100 GB. It's taken when you add a torrent and given back when you stop it, quit, or after five minutes with nothing playing.
- **More cache, if you like.** A bigger cache keeps more of the video around where you're watching: smoother playback on slow torrents and quicker seeks back. A change applies to the next torrent you add.
- **Only what you watch.** The app downloads the part your player asks for and a little ahead of it, never the whole file. Seeking back further than the cache holds downloads that part again.
- **Measured:** in our tests the whole server used about 110 MiB while streaming and about 15 MiB after Stop. The window and your video player use memory of their own. On a Keenetic router it used 112 MiB with a 4K film and 6 MiB before one was added.
- **No disk at all:** no video files, no cache files, no history.

## Privacy

TorrStream Lite runs on your computer, or on your router if you install it there. It has no account, no analytics and no servers of its own.

- **Torrent traffic:** like any torrent app, it connects to the trackers and peers of the torrent you add, and to the DHT network that helps find peers. They see your IP address.
- **Torrent URLs:** when you add a torrent by its URL, the app downloads the .torrent file from that address.
- **Nothing is kept:** the video is released when you stop the torrent or quit. Watched positions from TV apps last only until the torrent is stopped. When nothing has played for five minutes, or a TV app drops the torrent, the app keeps only its link in memory, so the TV app can play it again; Stop, another torrent or quitting forgets it.
- **Settings:** the Mac app saves only the home network switch and port, the cache size, the appearance, and the window's size and position.
- **On a router:** the server saves nothing. Its start script hands what it prints to the router's system log, and a warning there can name a torrent's hash and the addresses of peers. It runs all the time, so it's part of the DHT network all the time, also with no torrent loaded.

Read the full [privacy policy](https://andeliseev.github.io/torrstream-lite/privacy.html).

## What it doesn't do

- **Play several torrents at once.** One torrent runs at a time, with the whole cache to itself. Opening another torrent in Lampa replaces the one that's loaded.
- **Convert video.** Files play as they are, so your player must support their format. VLC and IINA play almost anything.
- **Upload to other peers (seed).** It only downloads, so it doesn't help others finish the torrent, and it doesn't suit private trackers that require you to upload.
- **Search for torrents.** Bring a magnet link or a .torrent file, or search in Lampa.
- **Work as a DLNA media server.** A TV's own media player can't browse it; use Lampa, another TorrServer app, or a player with the stream address.
- **Play a few rare torrents:** ones whose pieces don't fit three times in the cache (larger than 32 MiB with the default 96 MiB cache, 64 MiB with a bigger one), and ones made only in the newer BitTorrent v2 format. Hybrid v1 and v2 torrents play.
- **Run on Intel Macs,** or on Keenetic routers with MIPS processors. The router build has run on one model so far, a Keenetic Ultra (KN-1811).

## Building from source and contributing

The server is written in Rust on a modified [librqbit](https://github.com/ikatson/rqbit) torrent engine, and the Mac app is a small Swift window around it. TorrStream Lite is an independent project inspired by TorrServer; it doesn't contain TorrServer's code. To build it yourself, run the tests or publish a release, see [docs/development.md](docs/development.md). How it works inside is in [docs/](docs/README.md), and the Keenetic setup in [docs/router.md](docs/router.md).

Bug reports and pull requests are welcome. To report a security problem, see [SECURITY.md](SECURITY.md).

## License

[MIT](LICENSE), © 2026 Andrei Eliseev. The modified librqbit engine stays under Apache-2.0, and the other libraries the app is built with keep their own licenses; see the [third-party notices](THIRD_PARTY_NOTICES.txt).
