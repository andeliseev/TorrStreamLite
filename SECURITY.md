# Security

TorrStream Lite runs a server on your computer, and on your home network while other devices are allowed, so security reports are very welcome.

## Reporting a problem

Please **don't open a public issue** for a security problem. Instead, report it privately:

- With GitHub's **Report a vulnerability** button on the repository's **Security** tab, or
- By email to **andeliseev@gmail.com**

Include what you found, how to reproduce it, and what an attacker could do with it. You'll get a reply as soon as possible, and credit in the release notes if you'd like.

## What's in scope

For example:
- A web page, or a device on the network, controlling the server when it shouldn't: adding or stopping torrents, reading what's playing, or turning on access for other devices, which only the Mac app itself may do
- The server reachable from outside the Mac while other devices aren't allowed, or from outside the home network while it's on
- Torrent data written to disk, or memory use beyond the documented limits, caused by a crafted torrent, link or request
- The Mac app opening links, or starting apps, other than the project's GitHub pages and VLC or IINA

How the server checks browser origins and network access is described in [docs/architecture.md](docs/architecture.md#security).
