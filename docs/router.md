# TorrStream Lite on a Keenetic router

The server can run on a Keenetic router by itself, so a TV plays through Lampa with no Mac switched on. This is experimental: it has run on **one router so far**, a Keenetic Ultra (KN-1811), where it played a 4K film to a TV ([what was measured](development.md#measurements)). Before a release, its binary is run on an emulated router processor, where it must start, answer and stream from a test peer ([development.md](development.md#releases-github-actions)). That says nothing about another router's kernel, memory or speed; the [first-run checklist](#first-run-checklist) below is for finding that out. [Back to the docs](README.md).

On this page:

- [What you need](#what-you-need), and [what it changes on the router](#what-it-changes-on-the-router)
- [Install](#install): Entware, the server, the first start, starting when the router boots
- [Use it](#use-it), [check that it works](#check-that-it-works), [switch it off and on](#switch-it-off-and-on)
- [Update](#update) and [uninstall](#uninstall)
- [If it doesn't work](#if-it-doesnt-work)
- Details: [the home network](#keep-it-inside-the-home-network), [root certificates](#root-certificates), [IPv6](#ipv6), [memory and processor](#memory-and-processor), the [first-run checklist](#first-run-checklist), [building it yourself](#build-it-yourself)

The commands use `192.168.1.1` for the router's address in the home network; use your own. "On the Mac" means its Terminal (Linux works the same), and "on the router" means the router's shell, which you reach over SSH.

## What you need

- **A Keenetic with an ARM64 processor.** Many Keenetic models have MIPS processors, and this build can't run on them. [Keenetic's Entware instructions](https://help.keenetic.com/hc/en-us/articles/360021214160) name the installer for each model: if yours takes `aarch64-installer.tar.gz`, it's ARM64, and `uname -m` in its shell will print `aarch64`.
- **Entware** on the router, in its own storage or on a USB drive. [Adding it](#add-entware-to-the-router) is the first step below.
- **A computer in the home network** with `ssh`: the Terminal of a Mac or of Linux.
- **About 10 MB** of Entware's storage for the server, and **about 200 MB of free RAM**: with the default 96 MiB cache, the server takes about 112 MiB while a film plays.

## What it changes on the router

- **Entware is the bigger step.** Adding Keenetic's **Open Package support** component, which Entware needs, updates the router's system to its latest version and restarts it, as any change of components does.
- **Two files in Entware and nothing else:** the server and its start script. It changes nothing in the router's own system or settings, doesn't ask the router to open a port (no UPnP) and takes no connections from peers. Its page and streams are served only at the address `--listen` gives.
- **It writes nothing itself.** The video is kept in RAM. The start script hands what the server prints to the router's system log; a warning there can name a torrent's hash and the addresses of peers.
- **What it takes is RAM and processor time while something plays:** the cache and a little more (the [checklist](#first-run-checklist) measures how much), and no cache once the torrent is released. If the router runs out of memory, its system ends processes or restarts; use a smaller cache.
- **It talks to the Internet while idle too.** From its start the server is part of the BitTorrent DHT network, which is how magnet links find peers: it exchanges small messages with other nodes, which see your address, also with no torrent loaded. With the start script that is all the time. `--no-dht` (in the script: `EXTRA="--no-dht"`) switches it off; magnet links then depend on their trackers.
- **Nothing restarts it while the router runs.** A server started by hand is gone after a restart of the router. With the start script it is started once when the router boots, idle, and not again if it stops.
- **There's a way out without SSH:** with Entware's storage taken away, nothing of Entware starts. See [the router misbehaves and SSH doesn't answer](#the-router-misbehaves-and-ssh-doesnt-answer).

## Install

About a quarter of an hour, most of it for Entware. The router restarts once, when Keenetic's component is added.

### Add Entware to the router

Entware gives the router a place for extra programs (`/opt`) and an SSH shell. Keenetic documents it for [a USB drive](https://help.keenetic.com/hc/en-us/articles/360021214160) and, on the models that can, for [the router's own storage](https://help.keenetic.com/hc/en-us/articles/360021888880). Those pages are the reference, and Keenetic's menus and commands change between versions. The steps below worked on a Keenetic Ultra (KN-1811) in November 2026, with Entware in the router's own storage, which needs no drive.

1. **Save the router's settings**, in case you want them back: in the router's web interface, on **Management → General System Settings**, under **System files**, save `startup-config` to the computer.
2. **Add the component.** On the same page, click **Component options**, tick **Open Package support** and click **Update KeeneticOS**. This updates the router's system to its latest version and restarts it. The router must be online, and must not be switched off meanwhile.
3. **Choose where Entware lives.** On the **OPKG** page, under **Management**, set **Drive** to **Internal storage**, see that **Access** is on for your account, and save.
4. **Start the install** from the router's command line. In the address of the web interface, replace `dashboard` with `a` (`http://192.168.1.1/a`), and send these three commands, one at a time:

   ```text
   no opkg disk
   opkg disk storage:/ https://bin.entware.net/aarch64-k3.10/installer/aarch64-installer.tar.gz
   system configuration save
   ```

   The second must answer `Disk is set to: storage:/`. The first is there because step 3 has already set the disk, and the command installs only when the disk changes: without it the answer is `disk is unchanged`, and nothing is installed.

5. **Watch it install** on **Management → Diagnostics → System log**. Within a few minutes there are lines from `[1/5]` to `[5/5]`, and the last says that Entware is installed.
6. **Log in and change the password.** On the Mac:

   ```sh
   ssh root@192.168.1.1
   ```

   Answer `yes` to the question about the router's key. The password is the one in Keenetic's instructions, `keenetic`, the same on every router, so type `passwd` right away and set your own: this shell controls the whole router. Every `ssh` command below asks for it.

If the router's own **SSH server** component is installed, Entware's SSH is on port 222 instead of 22: add `-p 222` to every `ssh` command on this page.

For a USB drive, format it as ext4, put the installer into a folder named `install` on it, and choose the drive on the OPKG page, as Keenetic's instructions say. Other guides suggest adding an `opkg chroot` line to the router's configuration; don't: on KeeneticOS 5.1 it [stops the install](https://forum.keenetic.ru/topic/30216-kn-1811-%D0%BF%D0%B5%D1%80%D0%B5%D1%81%D1%82%D0%B0%D0%BB-%D1%83%D1%81%D1%82%D0%B0%D0%BD%D0%B0%D0%B2%D0%BB%D0%B8%D0%B2%D0%B0%D1%82%D1%8C%D1%81%D1%8F-opkg/).

### Check the router

On the router, in that SSH shell:

```sh
uname -m && uname -r
head -3 /proc/meminfo
df -h /opt
```

| Line | What it must show |
| --- | --- |
| The first, from `uname -m` | `aarch64`. With anything else, this build can't run on the router |
| `MemAvailable` | 200000 kB or more for the default cache. With less, use a smaller cache: `--cache-mib 48`, and `CACHE_MIB=48` in the start script. A torrent's piece must fit three times in the cache, so with 48 MiB the few torrents with pieces over 16 MiB don't load |
| `Available`, in the line for `/opt` | 15M or more: the server is 10 MB |

A KN-1811 showed kernel `4.9-ndm-5`, 320 MB available, and 91 MB free in its own storage. Type `exit` to leave the shell.

### Get the files

Download `TorrStream-Lite-<version>-linux-arm64.tar.gz` from the [Releases page](https://github.com/andeliseev/TorrStreamLite/releases). Not every release has the router's file: take it from the newest release that lists it. It must be newer than 0.2.9: the builds up to that one don't start on a router without root certificates, and have no start script. `shasum -a 256` on the download must print the value in the `.sha256` file next to it. The archive unpacks into a folder with:

| File | What it is |
| --- | --- |
| `torrstream-lite-linux-arm64` | The server: one statically linked ARM64 Linux program, with nothing else to install |
| `S99torrstream-lite` | The Entware script that starts it when the router boots |
| `LICENSE`, `THIRD_PARTY_NOTICES.txt` | The licenses |

Or [build the archive yourself](#build-it-yourself).

### Copy the server

On the Mac, in Terminal, go to the folder with the download, unpack it and copy the server to the router. Plain `ssh` is enough, so it doesn't matter whether the router has `scp`. If the browser has already unpacked the archive, leave out the `tar` line.

```sh
cd ~/Downloads
tar -xzf TorrStream-Lite-*-linux-arm64.tar.gz
cd TorrStream-Lite-*-linux-arm64
ssh root@192.168.1.1 'cat > /opt/bin/torrstream-lite && chmod 755 /opt/bin/torrstream-lite' < torrstream-lite-linux-arm64
```

Then see that it arrived whole and runs:

```sh
shasum -a 256 torrstream-lite-linux-arm64
ssh root@192.168.1.1 'sha256sum /opt/bin/torrstream-lite; /opt/bin/torrstream-lite --version'
```

The two checksums must be the same, and the last line must be `torrstream-lite` with the version. That is the program running on this router for the first time; no server is started yet.

The start script comes [later](#start-it-when-the-router-boots), once the server has run by hand: until that script is there, nothing starts the server when the router boots, so a restart of the router undoes a first start that went wrong.

### The first start, by hand

Start it once where you can see what it says. On the Mac:

```sh
ssh -t root@192.168.1.1 '/opt/bin/torrstream-lite --listen 192.168.1.1:8090 --cache-mib 96 --peers 20'
```

The server prints a few lines and keeps running. The warning is expected: TorrStream Lite never uploads.

```text
… WARN librqbit::session: uploading disabled
TORRSTREAM_LISTEN=http://192.168.1.1:8090
… INFO torrstream_lite: TorrStream Lite ready addr=192.168.1.1:8090 cache_mib=96 peers=20
```

Open <http://192.168.1.1:8090> in a browser, and play something ([Use it](#use-it)). The server runs for as long as that Terminal window stays open; Ctrl-C stops it.

### Start it when the router boots

Once the first start has worked, stop that server with Ctrl-C and copy the start script from the same folder on the Mac. If the router's address in your home network isn't `192.168.1.1`, change the `LISTEN=` line at the top of `S99torrstream-lite` first. The cache size (`CACHE_MIB`), the number of peers (`PEERS`) and other options (`EXTRA`) are set there too.

```sh
ssh root@192.168.1.1 'cat > /opt/etc/init.d/S99torrstream-lite && chmod 755 /opt/etc/init.d/S99torrstream-lite' < S99torrstream-lite
ssh root@192.168.1.1 '/opt/etc/init.d/S99torrstream-lite start; /opt/etc/init.d/S99torrstream-lite status'
```

The second command answers like this, and the server keeps running after the Terminal window is closed:

```text
torrstream-lite started: http://192.168.1.1:8090
torrstream-lite is running, process 9898
VmHWM:	    6456 kB
VmRSS:	    6456 kB
```

Entware runs every executable `S…` script in `/opt/etc/init.d` with `start` when the router boots, so the server is meant to start then; the script tries five times, a few seconds apart, in case the home network isn't up yet. Where the router has the `logger` command, the script hands what the server prints to the router's system log (**Management → Diagnostics → System log**), marked `torrstream-lite`; otherwise those lines are dropped.

In the system log, the server's lines look like this (its own times are UTC):

```text
torrstream-lite  2026-11-10T23:37:30.119266Z WARN librqbit::session: uploading disabled
torrstream-lite  TORRSTREAM_LISTEN=http://192.168.1.1:8090
torrstream-lite  2026-11-10T23:37:30.143120Z INFO torrstream_lite: TorrStream Lite ready addr=192.168.1.1:8090 cache_mib=96 peers=20
```

All of this has been seen on a KN-1811: `start`, `stop`, `restart` and `status`, the server outliving the SSH session that started it, its lines in the system log, and the start at boot, where the server was running again about a minute into the router's restart. One thing was missing there: the three lines of that start at boot weren't found in the log, while those of a later `restart` were. A router sets its clock only some time after it starts, so they may be at the beginning of the log under an old date.

## Use it

- **On the TV:** in Lampa, open **Settings → TorrServer** and enter the router's address, `192.168.1.1:8090`. Other TorrServer apps have a similar setting. Then choose a torrent and play it, as with the Mac app.
- **From a computer or a phone** in the home network: open <http://192.168.1.1:8090>. It's the Mac app's page, without the home network switch and the cache setting, which the router sets when it starts the server. Paste a magnet link or a torrent URL and click **Add**. **Play** opens a file in VLC, through a `vlc://` link, where VLC is installed. For IINA or another player, click the copy button next to it and open that address in the player. **Stop & release RAM** frees the cache.

One torrent plays at a time. Its cache is released when Lampa closes the film, on Stop, and after five minutes with nothing playing. What the server doesn't do is the same as for the Mac app: see the [project README](../README.md#what-it-doesnt-do).

## Check that it works

| Where | Do | A working server shows |
| --- | --- | --- |
| A browser in the home network | Open `http://192.168.1.1:8090` | The control page |
| The same | Open `http://192.168.1.1:8090/echo` | `TorrStreamLite.` and the version. TV apps ask this address to see whether the server is there |
| A terminal in the home network | `curl http://192.168.1.1:8090/api/status` | The version, `"mode":"RAM only"`, and `"torrent":null` while nothing is loaded |
| The router | `/opt/etc/init.d/S99torrstream-lite status` | `torrstream-lite is running` and its memory: about 6,500 kB after a start, about 15,000 kB once a film has been released, about 115,000 kB with a film in the default cache |
| The router's web interface | **Management → Diagnostics → System log** | Lines marked `torrstream-lite`, with the `ready` line after each `start` or `restart`. Those of a start at boot may be missing, or under an old date, so don't take their absence for a stopped server |
| While a film plays | The page's sidebar | The download speed, the peers, and the cache filled to its size |

[Memory and processor](#memory-and-processor) has more ways to watch it.

## Switch it off and on

On the router, or from the Mac as `ssh root@192.168.1.1 '…'` with the command between the quotes:

| To | Command |
| --- | --- |
| Stop the server | `/opt/etc/init.d/S99torrstream-lite stop` |
| Start it | `/opt/etc/init.d/S99torrstream-lite start` |
| Stop and start it | `/opt/etc/init.d/S99torrstream-lite restart` |
| See whether it runs, and its memory | `/opt/etc/init.d/S99torrstream-lite status` |
| Not start it when the router boots | `chmod -x /opt/etc/init.d/S99torrstream-lite` |
| Start it at boot again | `chmod +x /opt/etc/init.d/S99torrstream-lite` |

A stopped server uses no memory and makes no connections. After `chmod -x`, the commands above need `sh` in front: `sh /opt/etc/init.d/S99torrstream-lite stop`.

The settings are the four lines at the top of the script: `LISTEN`, `CACHE_MIB`, `PEERS` and `EXTRA`. Change them on the router with `vi /opt/etc/init.d/S99torrstream-lite`, or in the file on the Mac and copy it again, and then `restart`.

## Update

Download the new version and unpack it, as in [Copy the server](#copy-the-server). On the Mac, in its folder:

```sh
ssh root@192.168.1.1 'sh /opt/etc/init.d/S99torrstream-lite stop'
ssh root@192.168.1.1 'cat > /opt/bin/torrstream-lite && chmod 755 /opt/bin/torrstream-lite' < torrstream-lite-linux-arm64
ssh root@192.168.1.1 'sh /opt/etc/init.d/S99torrstream-lite start'
```

`http://192.168.1.1:8090/echo` then shows the new version. A server started by hand is stopped with Ctrl-C instead, and started again by hand. The start script changes rarely. A newer one has the default settings, so put yours into it before copying it.

## Uninstall

**The server.** On the router:

```sh
sh /opt/etc/init.d/S99torrstream-lite stop && rm /opt/etc/init.d/S99torrstream-lite /opt/bin/torrstream-lite
```

That is all of it: it kept nothing else on the router. If the answer is that the server could not be stopped, nothing was deleted; run the line again. In Lampa, set TorrServer back to what it was.

**Entware too**, if you added it only for this. This erases the router's whole own storage, not only Entware: anything else kept there is lost. The commands are from [Keenetic's instructions](https://help.keenetic.com/hc/en-us/articles/360021888880) for Entware in the router's own storage, and haven't been run for this guide. In the router's command line (`http://192.168.1.1/a`):

```text
no opkg disk
no system mount storage:
erase storage:
```

Then untick **Open Package support** under **Component options**; like adding it, that updates and restarts the router. For Entware on a USB drive, the first command is enough before removing the component, and the drive can be unplugged once the router is switched off.

## Keep it inside the home network

Always give `--listen` the router's **address in the home network**, never `0.0.0.0`: that would also listen on the Internet side. The server has no password, so anything in the home network can add, play and stop torrents, including web pages open on its devices. Don't forward its port.

To use a name instead of the address, such as `my.keenetic.net`, add `--allow-host my.keenetic.net` (in the script: `EXTRA`); other names are refused, to stop DNS rebinding.

## If it doesn't work

Start with what the server itself says. Started by hand ([the first start](#the-first-start-by-hand)), it prints to the Terminal. Started by the script, its lines are meant to be in the router's system log; if they aren't, stop it and start it by hand.

### Installing Entware

| What you see | What to do |
| --- | --- |
| `disk is unchanged` after the `opkg disk` command | The disk was already set, so nothing was installed. Send `no opkg disk` first, then the command again |
| `Disk is set to: storage:/`, but no `[1/5]` lines in the system log | Install from a folder, as Keenetic's instructions describe: download `aarch64-installer.tar.gz` from the address in step 4, and on **Applications** open **Internal storage**, make a folder named `install` and put the file into it. Then send `no opkg disk` and `opkg disk storage:/`. If the router's configuration has an `opkg chroot` line, remove it |
| The log stops at `[2/5]` | The installer can't download Entware's packages: `bin.entware.net` doesn't answer the router. Try again later; Entware lists its [mirrors](https://entware.net/mirrors.html) |
| `ssh: connect to host … port 22: Connection refused` | Entware's SSH server isn't running: the install didn't reach `[5/5]`. Or it is on port 222, because the router's own SSH server component has port 22: `ssh -p 222 …` |
| `Permission denied` after the password | Until you've run `passwd`, the password is Entware's from Keenetic's instructions; after that, yours. The router's administrator password doesn't work here |
| `REMOTE HOST IDENTIFICATION HAS CHANGED` | Entware was installed again and has new keys. On the Mac: `ssh-keygen -R 192.168.1.1` (with port 222: `ssh-keygen -R '[192.168.1.1]:222'`), then log in again |

### Starting the server

| What you see | What it means |
| --- | --- |
| `syntax error`, `Exec format error` or `not found` when starting the server | The router's processor isn't ARM64 (`uname -m`), or the file didn't arrive whole: compare the checksums as in [Copy the server](#copy-the-server) |
| `Illegal instruction` | The processor lacks an instruction the build uses. Please report it with the model and `cat /proc/cpuinfo` |
| `Cannot open HTTP port; another server may be running` | The server is already running (`status`). Or the address after `--listen` isn't one of the router's (`ip addr` or `ifconfig` lists them). Or something else uses port 8090: Keenetic's own torrent client (Download Station) has its page on that port when it's switched on. Then choose another port, such as 8091, and enter the same in Lampa |
| `Killed`, or the server is gone and the system log mentions memory | The router ran out of memory. Use a smaller cache, such as `CACHE_MIB=48` |
| `Text file busy` when copying a new version | The server is running; `stop` it first |
| `torrstream-lite did not start` from the script | It names the command to run by hand, which prints the reason |
| It doesn't start at boot, but `start` works by hand | Look in the system log for what Entware ran at boot and what the script answered. `ls -l /opt/etc/init.d/S99torrstream-lite` must show `x` in its permissions, and `LISTEN=` in it must be the router's address |

### Using it

| What you see | What it means |
| --- | --- |
| The server says `ready`, but the page doesn't open | `wget -qO- http://192.168.1.1:8090/echo` on the router tells whether the server answers there. If it does, the router's firewall rules for the home network are in the way, or the computer is in another segment, such as the guest network |
| Lampa can't connect | The address in Lampa must be `192.168.1.1:8090`, with `http` and not `https` if it asks. The TV must be in the home network, not the guest one. `http://192.168.1.1:8090/echo` must open in a browser on another device |
| The page opens, but torrents don't load | The page says what the lookup found. `nslookup github.com` on the router tells whether its DNS answers programs running on it |
| `invalid peer certificate` when adding an HTTPS torrent URL | The router's clock is wrong, or its certificate store is out of date. `date` shows the clock. To try the built-in list instead, stop the server and start it by hand with `SSL_CERT_FILE=/dev/null` in front of the command |
| The film stops to load | **Download** on the page shows the speed and every peer. A speed below the film's bitrate means the torrent's peers don't send fast enough: try another release of the film. If the speed is high and it still stops, look at `top` and `head -3 /proc/meminfo` on the router while it plays, and please report it |
| The router or the Internet is slow while a film plays | Lower `PEERS` or `CACHE_MIB` in the script and `restart`. `stop` gives everything back at once |

### The router misbehaves and SSH doesn't answer

Restart the router. If only a server started by hand was running, that is the end of it: nothing starts it again.

With the start script installed, the server is started with the router. To keep all of Entware from starting, take its storage away:

- **Entware on a USB drive:** switch the router off, unplug the drive, and switch it on.
- **Entware in the router's own storage:** send `no opkg disk` and then `system configuration save` in the router's command line (`http://192.168.1.1/a`), and restart the router.

Nothing is deleted by that: Entware and the server stay in the storage, switched off. Choosing the storage again on the OPKG page is meant to switch them back on; that hasn't been tried here. Step 4's command, with the installer's address, would install Entware again instead.

### Report it

Open an issue at <https://github.com/andeliseev/TorrStreamLite/issues> with the router's model and KeeneticOS version, `uname -r`, the lines the server printed, and what `http://192.168.1.1:8090/api/status` shows. The status names the torrent that is loaded; leave that part out if you prefer.

## Root certificates

Torrent URLs and trackers that use HTTPS are checked against the system's root certificates: the file `SSL_CERT_FILE` names or the folder `SSL_CERT_DIR` names, or else the usual places of a Linux system. Under Entware that is the file `/opt/etc/ssl/certs/ca-certificates.crt`, which its `ca-bundle` package installs. A router that has none uses the list built into the server (Mozilla's, as it was when that version was built), and says so when it starts:

```text
… INFO torrstream_lite::tls: This system has no root certificates, so HTTPS uses the built-in list reason=…
```

A KN-1811 with a fresh Entware had certificates of its own, so it didn't print this line. Certificates are only valid at the right date, so HTTPS fails until the router has set its clock.

## IPv6

On a Keenetic, IPv6 is a component of the system that may not be installed, and the kernel then has no IPv6 at all. The server notices that it can't open an IPv6 socket, uses IPv4 only, and says so when it starts:

```text
… INFO torrstream_lite: This system has no IPv6, so the server uses IPv4 only
```

`--ipv4-only` asks for the same where there is IPv6.

## Memory and processor

- **In a browser:** the control page shows the server's memory. `http://192.168.1.1:8090/api/status` has the numbers: `resident_bytes` (memory now), `peak_resident_bytes` (the most since it started) and `cpu_seconds` (processor time used since it started).
- **On the router:** `/opt/etc/init.d/S99torrstream-lite status` prints `VmRSS` (memory now) and `VmHWM` (the most since it started). `top` shows the processor: BusyBox's `top` counts all cores together, so on a two-core router 50% is one core fully busy. `head -3 /proc/meminfo` shows the router's own free memory.
- **Over time, from the Mac:** `python3 tests/watch.py http://192.168.1.1:8090`, run in a copy of this repository (it needs Python 3 and isn't in the download), prints a line every five seconds with the download speed, peers, cache, memory and processor load, here as a share of one core. Ctrl-C ends it with a summary: the peak memory, the average and highest download speed, and the average and highest load while something played. It only reads. Add `| tee -i router-run.txt` to keep the lines; the `-i` lets the summary through when you press Ctrl-C.

## First-run checklist

What isn't known about a router until it has run there, and how to find out. Keep `python3 tests/watch.py http://192.168.1.1:8090 | tee -i router-run.txt` running on the Mac, in a second Terminal window, from the second row on.

| To find out | Do | Read off |
| --- | --- | --- |
| **Does it run on this router's kernel?** | The [first start by hand](#the-first-start-by-hand), then `uname -r` | The `ready` line, and the page opening on the Mac. The kernel version. Whether it said it uses the built-in root certificates, or IPv4 only. Memory while idle (about 15 MiB on a Mac, 6 MiB on a KN-1811) |
| **Can it find peers?** | Add a magnet link on the page, or play something from Lampa | The files appear within a minute. If not, the page says why: no peers found points at the router's DNS or at the DHT, the network that finds peers for magnet links; peers that don't answer points at the torrent's peers |
| **A 4K film's bitrate** | Play a 4K film from Lampa for at least ten minutes, with few pauses: a paused film counts as playing. Its bitrate in Mbps is its size in GB × 133 ÷ its length in minutes | Whether it played without stopping to load. In the summary, the average download speed while playing, which should be about the film's bitrate, and the highest, which is what the router and the peers had to spare |
| **Seeks** | In that film, jump ten minutes ahead, five back, close to the end, and back to the start | The seconds until the picture is back, each time. With a film's usual 8 MiB pieces and peers that send 1 to 2 MB/s, it's 4 to 8 s on a Mac |
| **Peak memory** with `--cache-mib 96` | After the film and the seeks: `status` on the router, or the summary | `VmHWM` (`peak_resident_bytes`). The target for the whole server is under 192 MiB; a Mac measured 111 MiB, a KN-1811 112 MiB. And `MemAvailable` in `/proc/meminfo` while it plays: what's left for the router |
| **Processor** | `top` on the router while the film plays | The server's share and the idle share. In the summary, the average and highest load while playing |
| **A long run next to everything else** | Play a whole film while the home network and the VPN are in use as usual. Run an Internet speed test before and during the film, directly and through the VPN | Stops in playback. Speed test results during the film against before. `no answer` lines in `router-run.txt`: there should be none while the server runs. Memory at the film's end against its start: it shouldn't keep growing. Whether the router's web interface stays quick |
| **Does it let go?** | Stop playback and leave it for six minutes | The state goes from `loaded` to `idle` (the torrent is released after five minutes with nothing playing, or at once when Lampa closes the film), and memory drops to about 15 MiB. A KN-1811 had 6.4 MiB before its first film and 14.6 MiB after it |
| **Do stop and restart work?** | `restart` on the router, then `status` | `torrstream-lite stopped`, then `torrstream-lite started`, and a new process number |
| **Does it come back after a reboot?** | Reboot the router | `status` says it runs; the system log has its `ready` line |

Worth writing down with the results: the router's model and KeeneticOS version, `uname -r`, and `router-run.txt`. They go into [development.md](development.md#measurements), and what stays open is in the first lines of [todo.md](todo.md).

## Build it yourself

The archive can be built on a Mac that has Python 3.11 or later, with a project-local `cargo-zigbuild` and Zig. The third line adds the ARM64 Linux target to the project-local Rust toolchain, the one `scripts/cargo.sh` uses; with another toolchain, add `aarch64-unknown-linux-musl` to that one instead.

```sh
python3 -m venv .build/cross
.build/cross/bin/pip install cargo-zigbuild==0.24.1 ziglang==0.17.1
CARGO_HOME="$PWD/.build/cargo" RUSTUP_HOME="$PWD/.build/rustup" .build/cargo/bin/rustup target add aarch64-unknown-linux-musl
sh scripts/build-router.sh
sh scripts/package-router.sh
```

The build writes `dist/torrstream-lite-linux-arm64`, and the last script packs it into `release-assets/TorrStream-Lite-<version>-linux-arm64.tar.gz`. A Mac can't run the result, so a local build hasn't been run anywhere until a router runs it.
