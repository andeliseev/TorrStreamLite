# Vendored Rust torrent engine

Source: https://github.com/ikatson/rqbit

Commit: `f23bacdddcc5318caa813418b8eb1b1d7307faaa` (workspace version 9.1.0).

License: Apache-2.0, retained at `rqbit/LICENSE`. Source authorship and package metadata are retained in each crate. The upstream desktop app and web build assets are not included. The upstream workspace's desktop/binary default members are removed from its root manifest; this project builds the library only. `rqbit/Cargo.lock` drops the packages only those members used, with no other version changed, so the engine's tests run with `--locked`.

Local changes supporting bounded RAM streaming:

- `storage/mod.rs`: opt-in cache-slot, pre-request reservation (with a check for protected pieces, not a list), and release hooks, including forwarding through boxed storage.
- `torrent_state/initializing.rs`: ephemeral storage starts with an empty bitfield without scanning nonexistent files.
- `piece_tracker.rs`: demand-only acquisition with no cap on pieces in flight (the readers' window bounds them), explicit verifying state, safe reservation/cancellation, no whole-file queue in RAM mode; the readers' window as one range per reader, with the next missing piece found in the HAVE bits; a wanted piece moves to another peer when its peer sent none of it for 5 s or is much slower (3x for the piece a reader is at), like upstream's steals but by readers' urgency; a stall is judged by the owner's last chunk of any piece, and a piece taken over doesn't count toward the new owner's average.
- `chunk_tracker.rs`: eviction clears HAVE/chunk state and subtracts per-file and total cached-byte counters; a per-chunk received check.
- `torrent_state/live/mod.rs`: never treat RAM storage as finished after a piece completes (that disconnected every seeder between a player's requests), release the pieces of a peer that dies while not needed, record which peer delivered each chunk for the stall rule, integrate reservations before peer requests, announce RAM pieces taken over from another peer like steals and request only their missing chunks, cancel obsolete pieces, protect concurrent writers and hash checks, cap discovered candidates, wake downloaders after reader changes, and never treat RAM storage as finished, so idle peers stay connected between readers instead of being disconnected and backed off for 10 s or more.
- `torrent_state/streaming.rs`: `stream_from` starts a stream at a position instead of at 0, cache-sized interleaved reader windows as ranges, EOF handling, wakeups, and atomic HAVE/read access relative to eviction.
- `session.rs`: in RAM mode, trackers hear the torrent's size less what's cached as what's left, never 0, so they don't take us for a seeder. Before a magnet link's details arrive, or the torrent is live, the size isn't known and they hear 16 KiB left, as from libtorrent; upstream says 0 left on that first announce, the one whose answer has to do for the next half hour.
- `dht_utils.rs`, `peer_info_reader/mod.rs`: metadata connection concurrency 16 (upstream: 128), at most 4 of them holding a metadata buffer, and candidate limit 256. With 4 connections, a swarm of mostly dead peers used up the whole lookup time.
- `peer_info_reader/mod.rs`: metadata size limit 2 MiB and zero-size rejection.
- `peer_connection.rs`, `read_buf.rs`: a handler can let a quiet peer stay connected: between messages, a read that times out is tried again while `may_stay_quiet()` holds (a message cut short still times out). In RAM mode that's a peer that has sent its bitfield, has unchoked us and owes us no chunks. The read timeout dropped every seeder once a reader's window was cached, and the reconnect backoff left a seek with no one to download from.
- `dht_utils.rs`, `session.rs`, `lib.rs`, `tracker_comms/`: `AddTorrentOptions::metadata_lookup` counts the peers a magnet link's lookup found, asked and lost, so the app can say why the torrent's details didn't come. It also remembers each tracker's best answer (up to 64 trackers, 120 characters of a reason): `tracker_comms` calls the new `TorrentStatsProvider::on_announce` with the peers a tracker returned, its failure reason (HTTP `failure reason`, UDP error) or that it couldn't be reached.
- `session.rs`, `torrent_state/mod.rs`, `torrent_state/live/mod.rs`: peer connections get the session's keep-alive interval. Upstream leaves it out of the per-torrent options, so peers got the 120 s default.

A test fix: in `tests/fake_peer.rs`, `test_fake_peer_downloads_a_block_from_a_seeder` waits for the seeder's upload counter. The seeder updates it when its write returns, which can be after the peer has read the block, so the test failed now and then on Linux.

The last two limits apply to this vendored build generally. The storage/scheduler changes opt in only for RAM storage. Metadata and cache limits are intentional product constraints and are documented in the root README.

TorrServer's source at commit `0c48c9ac78de2864f5fc9341e8caf89d355a4850` was inspected for architecture and the documented API shapes. Its Go implementation was not copied into this server.
