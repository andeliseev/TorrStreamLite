use std::{
    collections::HashSet,
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use anyhow::Context;
use buffers::ByteBufOwned;
use futures::{Stream, StreamExt, stream::FuturesUnordered};
use librqbit_core::torrent_metainfo::TorrentMetaV1Info;
use tracing::{Instrument, debug, debug_span};

use crate::{
    peer_connection::PeerConnectionOptions, peer_info_reader, spawn_utils::BlockingSpawner,
    stream_connect::StreamConnector,
};
use librqbit_core::hash_id::Id20;

/// Peers asked for a magnet link's details at the same time.
const METADATA_CONNECTIONS: usize = 16;
/// Peers that may be sending the details, up to 2 MiB each, at the same time.
const METADATA_BUFFERS: usize = 4;

/// Trackers whose answers a lookup remembers; a magnet link can name any number.
const TRACKERS_REMEMBERED: usize = 64;
/// Characters kept of a tracker's reason for refusing.
const REFUSAL_LEN: usize = 120;

/// How a magnet link's lookup is going, so a caller can say why the torrent's details
/// didn't come: the peers trackers and the DHT gave, how many of them were asked and
/// failed, and what the trackers answered.
#[derive(Debug, Default)]
pub struct MetadataLookup {
    found: AtomicUsize,
    asked: AtomicUsize,
    failed: AtomicUsize,
    /// Each tracker's best answer so far, by its URL.
    trackers: parking_lot::Mutex<Vec<(String, TrackerAnswer)>>,
}

/// A tracker's answer, from the least to the most useful: a later, lesser one doesn't
/// replace it (a tracker with two addresses can fail on one and answer on the other).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum TrackerAnswer {
    Unreachable,
    Refused(String),
    Peers(usize),
}

/// A snapshot of a [`MetadataLookup`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MetadataLookupCounts {
    /// Peers trackers and the DHT gave, each once; at most 256 are kept.
    pub found: usize,
    /// Peers asked for the details so far, 16 at a time.
    pub asked: usize,
    /// Peers asked that couldn't be reached, didn't answer, or sent no details.
    pub failed: usize,
    /// Trackers that answered with a list of peers, even an empty one.
    pub trackers_answered: usize,
    /// Trackers that refused, with a reason like "Torrent not registered".
    pub trackers_refused: usize,
}

impl MetadataLookup {
    pub fn counts(&self) -> MetadataLookupCounts {
        MetadataLookupCounts {
            found: self.found.load(Ordering::Relaxed),
            asked: self.asked.load(Ordering::Relaxed),
            failed: self.failed.load(Ordering::Relaxed),
            trackers_answered: self.count_trackers(|a| matches!(a, TrackerAnswer::Peers(_))),
            trackers_refused: self.count_trackers(|a| matches!(a, TrackerAnswer::Refused(_))),
        }
    }

    fn count_trackers(&self, f: impl Fn(&TrackerAnswer) -> bool) -> usize {
        self.trackers.lock().iter().filter(|(_, a)| f(a)).count()
    }

    /// The reason trackers gave for refusing the torrent: the one most of them gave. It's the
    /// tracker's text, cut to 120 characters. Other trackers may have answered all the same:
    /// open ones answer for any torrent, also one its own tracker no longer has.
    pub fn tracker_refusal(&self) -> Option<String> {
        let trackers = self.trackers.lock();
        let reasons = trackers.iter().filter_map(|(_, a)| match a {
            TrackerAnswer::Refused(r) => Some(r),
            _ => None,
        });
        reasons
            .clone()
            .max_by_key(|r| reasons.clone().filter(|o| o == r).count())
            .cloned()
    }

    /// Records how an announce to `tracker` went.
    pub(crate) fn tracker_answered(
        &self,
        tracker: &url::Url,
        outcome: tracker_comms::AnnounceOutcome<'_>,
    ) {
        use tracker_comms::AnnounceOutcome as O;
        let answer = match outcome {
            O::Peers(n) => TrackerAnswer::Peers(n),
            O::Refused(reason) => TrackerAnswer::Refused(
                reason
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(REFUSAL_LEN)
                    .collect::<String>()
                    .trim()
                    .to_owned(),
            ),
            O::Unreachable => TrackerAnswer::Unreachable,
        };
        let mut trackers = self.trackers.lock();
        if let Some((_, best)) = trackers.iter_mut().find(|(t, _)| t == tracker.as_str()) {
            if answer > *best {
                *best = answer;
            }
        } else if trackers.len() < TRACKERS_REMEMBERED {
            trackers.push((tracker.as_str().to_owned(), answer));
        }
    }
}

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum ReadMetainfoResult<Rx> {
    Found {
        info: TorrentMetaV1Info<ByteBufOwned>,
        info_bytes: ByteBufOwned,
        rx: Rx,
        seen: HashSet<SocketAddr>,
    },
    ChannelClosed {
        #[allow(dead_code)]
        seen: HashSet<SocketAddr>,
    },
}

pub async fn read_metainfo_from_peer_receiver<A: Stream<Item = SocketAddr> + Unpin>(
    peer_id: Id20,
    info_hash: Id20,
    initial_addrs: Vec<SocketAddr>,
    addrs_stream: A,
    peer_connection_options: Option<PeerConnectionOptions>,
    connector: Arc<StreamConnector>,
    client_name_and_version: String,
    lookup: Option<Arc<MetadataLookup>>,
) -> ReadMetainfoResult<A> {
    let mut seen = HashSet::<SocketAddr>::new();
    let mut addrs = addrs_stream;
    let lookup = lookup.unwrap_or_default();

    // Most peers a tracker or the DHT gives never answer, and each takes seconds to fail, so
    // many are asked at once. The details can be 2 MiB, so only a few may send them at a time.
    let semaphore = tokio::sync::Semaphore::new(METADATA_CONNECTIONS);
    let buffers = Arc::new(tokio::sync::Semaphore::new(METADATA_BUFFERS));

    let read_info_guarded = |addr| {
        let semaphore = &semaphore;
        let lookup = &lookup;
        let buffers = buffers.clone();
        let connector = connector.clone();
        let client_name_and_version = client_name_and_version.clone();
        async move {
            let token = semaphore.acquire().await?;
            lookup.asked.fetch_add(1, Ordering::Relaxed);
            let ret = peer_info_reader::read_metainfo_from_peer(
                addr,
                peer_id,
                info_hash,
                peer_connection_options,
                // This shouldn't be called anyway as we aren't reading/writing to disk, so it's
                // ok not to use a shared one.
                BlockingSpawner::new(1),
                connector,
                client_name_and_version,
                buffers,
            )
            .instrument(debug_span!("read_metainfo_from_peer", ?addr))
            .await
            .with_context(|| format!("error reading metainfo from {addr}"));
            if ret.is_err() {
                lookup.failed.fetch_add(1, Ordering::Relaxed);
            }
            drop(token);
            ret
        }
    };

    let mut unordered = FuturesUnordered::new();

    for a in initial_addrs.into_iter().take(256) {
        seen.insert(a);
        unordered.push(read_info_guarded(a));
    }
    lookup.found.store(seen.len(), Ordering::Relaxed);

    let mut addrs_completed = false;

    loop {
        if addrs_completed && unordered.is_empty() {
            return ReadMetainfoResult::ChannelClosed { seen };
        }

        tokio::select! {
            done = unordered.next(), if !unordered.is_empty() => {
                match done {
                    Some(Ok((info, info_bytes))) => return ReadMetainfoResult::Found { info, info_bytes, seen, rx: addrs },
                    Some(Err(e)) => {
                        debug!("{:#}", e);
                    },
                    None => unreachable!()
                }
            }

            next_addr = addrs.next(), if !addrs_completed => {
                match next_addr {
                    Some(addr) => {
                        if seen.len() < 256 && seen.insert(addr) {
                            lookup.found.fetch_add(1, Ordering::Relaxed);
                            unordered.push(read_info_guarded(addr));
                        }
                        continue;
                    },
                    None => {
                        addrs_completed = true;
                    },
                }
            }
        };
    }
}

#[cfg(test)]
mod tests {
    use dht::{DhtBuilder, Id20};
    use librqbit_core::peer_id::generate_peer_id;

    use super::*;
    use std::{
        str::FromStr,
        sync::{Arc, Once},
    };

    static LOG_INIT: Once = Once::new();

    fn init_logging() {
        #[allow(unused_must_use)]
        LOG_INIT.call_once(|| {
            // pretty_env_logger::try_init();
        })
    }

    #[tokio::test]
    #[ignore]
    async fn read_metainfo_from_dht() {
        init_logging();

        let info_hash = Id20::from_str("cab507494d02ebb1178b38f2e9d7be299c86b862").unwrap();
        let dht = DhtBuilder::new().await.unwrap();

        let peer_rx = dht.get_peers(info_hash, None);
        let peer_id = generate_peer_id(b"-xx1234-");
        match read_metainfo_from_peer_receiver(
            peer_id,
            info_hash,
            Vec::new(),
            peer_rx,
            None,
            Arc::new(StreamConnector::new(Default::default()).await.unwrap()),
            crate::client_name_and_version().to_owned(),
            None,
        )
        .await
        {
            ReadMetainfoResult::Found { info, .. } => dbg!(info),
            ReadMetainfoResult::ChannelClosed { .. } => todo!("should not have happened"),
        };
    }

    #[test]
    fn a_lookup_remembers_each_trackers_best_answer() {
        use tracker_comms::AnnounceOutcome as O;
        let lookup = MetadataLookup::default();
        let a = url::Url::parse("http://a.example/ann?magnet").unwrap();
        let b = url::Url::parse("udp://b.example:6969").unwrap();
        lookup.tracker_answered(&a, O::Refused("Torrent not registered"));
        lookup.tracker_answered(&a, O::Unreachable);
        lookup.tracker_answered(&b, O::Unreachable);
        assert_eq!(
            lookup.tracker_refusal().as_deref(),
            Some("Torrent not registered")
        );
        let c = lookup.counts();
        assert_eq!((c.trackers_answered, c.trackers_refused), (0, 1));

        // A later failure of a tracker's other address doesn't undo its answer, and an open
        // tracker's answer doesn't hide that another one refused.
        lookup.tracker_answered(&b, O::Peers(0));
        lookup.tracker_answered(&b, O::Unreachable);
        assert_eq!(
            lookup.tracker_refusal().as_deref(),
            Some("Torrent not registered")
        );
        let c = lookup.counts();
        assert_eq!((c.trackers_answered, c.trackers_refused), (1, 1));

        // The reason is the tracker's text: bounded, and without control characters.
        let lookup = MetadataLookup::default();
        lookup.tracker_answered(&a, O::Refused(&format!("no\n{}", "x".repeat(500))));
        let reason = lookup.tracker_refusal().unwrap();
        assert_eq!(reason.chars().count(), REFUSAL_LEN);
        assert!(reason.starts_with("nox"));
        for i in 0..200 {
            let t = url::Url::parse(&format!("http://t{i}.example/ann")).unwrap();
            lookup.tracker_answered(&t, O::Unreachable);
        }
        assert_eq!(lookup.trackers.lock().len(), TRACKERS_REMEMBERED);
    }
}
