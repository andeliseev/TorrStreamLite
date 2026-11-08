# Vendored Rust torrent engine

Source: https://github.com/ikatson/rqbit

Commit: `f23bacdddcc5318caa813418b8eb1b1d7307faaa` (workspace version 9.1.0).

License: Apache-2.0, retained at `rqbit/LICENSE`. Source authorship and package metadata are retained in each crate. The upstream desktop app and web build assets are not included. The upstream workspace's desktop/binary default members are removed from its root manifest; this project builds the library only.

Local changes supporting bounded RAM streaming:

- `storage/mod.rs`: opt-in cache-slot, pre-request reservation, and release hooks, including forwarding through boxed storage.
- `torrent_state/initializing.rs`: ephemeral storage starts with an empty bitfield without scanning nonexistent files.
- `piece_tracker.rs`: bounded demand-only acquisition, explicit verifying state, safe reservation/cancellation, no whole-file queue in RAM mode.
- `chunk_tracker.rs`: eviction clears HAVE/chunk state and subtracts per-file and total cached-byte counters.
- `torrent_state/live/mod.rs`: integrate reservations before peer requests, cancel obsolete pieces, protect concurrent writers and hash checks, cap discovered candidates, wake downloaders after reader changes.
- `torrent_state/streaming.rs`: cache-sized interleaved reader windows, EOF handling, wakeups, and atomic HAVE/read access relative to eviction.
- `dht_utils.rs`: metadata connection concurrency 4 and candidate limit 256.
- `peer_info_reader/mod.rs`: metadata size limit 2 MiB and zero-size rejection.

The last two limits apply to this vendored build generally. The storage/scheduler changes opt in only for RAM storage. Metadata and cache limits are intentional product constraints and are documented in the root README.

TorrServer's source at commit `0c48c9ac78de2864f5fc9341e8caf89d355a4850` was inspected for architecture and the documented API shapes. Its Go implementation was not copied into this server.
