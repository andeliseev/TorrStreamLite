//! Coordinates piece download state by wrapping ChunkTracker with inflight piece tracking.
//!
//! This module provides the [`PieceTracker`] type which encapsulates the relationship between
//! queued pieces (in ChunkTracker) and in-flight pieces (being downloaded by a peer).
//!
//! The key invariant maintained is: a piece is in exactly one state at any time:
//! - HAVE (completed)
//! - QUEUED (available to download)
//! - IN_FLIGHT (currently being downloaded)
//! - NOT_NEEDED (not selected for download)

use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};

use bitvec::{order::Msb0, slice::BitSlice};
use buffers::ByteBuf;
use librqbit_core::lengths::ValidPieceIndex;
use peer_binary_protocol::Piece;

use crate::{
    chunk_tracker::{ChunkMarkingResult, ChunkTracker},
    file_info::FileInfo,
    type_aliases::{FileInfos, FilePriorities, PeerHandle},
};

/// Tracks a piece currently being downloaded.
#[derive(Debug, Clone)]
pub struct InflightPiece {
    pub peer: PeerHandle,
    pub started: Instant,
    /// Taken over from another peer, so its time says little about this peer's speed.
    pub taken_over: bool,
}

/// In RAM mode, a piece whose owner has sent nothing at all for this long, since it took the
/// piece, goes to the next peer that asks: the owner choked us or stopped. The owner's last
/// chunk of any piece counts, since a new piece's first chunk waits behind its earlier requests.
pub(crate) const RAM_STALL: Duration = Duration::from_secs(5);

/// A piece handed to a peer by the RAM scheduler.
#[derive(Debug, PartialEq)]
pub(crate) enum RamAcquired {
    /// A cache slot was reserved for it; `evicted_bytes` of older pieces left the cache.
    Reserved {
        piece: ValidPieceIndex,
        evicted_bytes: u64,
    },
    /// Taken over from another peer. The caller MUST call `peers.on_steal()`.
    Stolen {
        piece: ValidPieceIndex,
        from_peer: PeerHandle,
    },
}

/// The pieces active readers want, as one range per reader rather than a list: with small
/// pieces a cache has thousands of slots, and the window is consulted for every new piece.
///
/// Readers take turns, each one's first piece, then each one's second, and so on, until
/// `limit` distinct pieces. That leaves each reader a prefix of its read-ahead.
#[derive(Debug, Default)]
pub(crate) struct RamWindow {
    /// (first piece, number of pieces) per reader, in turn order.
    readers: Vec<(u32, u32)>,
}

impl RamWindow {
    /// `queues` holds each reader's read-ahead as (first piece, end), in turn order.
    pub(crate) fn interleave(queues: &[(u32, u32)], limit: u32) -> Self {
        if let [(start, end)] = *queues {
            return Self {
                readers: vec![(start, end.saturating_sub(start).min(limit))],
            };
        }
        let mut readers: Vec<(u32, u32)> = queues.iter().map(|&(start, _)| (start, 0)).collect();
        let mut distinct = 0;
        loop {
            let mut took = false;
            for (i, &(start, end)) in queues.iter().enumerate() {
                let next = start + readers[i].1;
                if next >= end {
                    continue;
                }
                if distinct == limit {
                    return Self { readers };
                }
                // A piece another reader already took counts once.
                let taken = readers
                    .iter()
                    .enumerate()
                    .any(|(j, &(s, len))| j != i && next.wrapping_sub(s) < len);
                if !taken {
                    distinct += 1;
                }
                readers[i].1 += 1;
                took = true;
            }
            if !took {
                return Self { readers };
            }
        }
    }

    pub(crate) fn contains(&self, piece: u32) -> bool {
        self.readers
            .iter()
            .any(|&(start, len)| piece.wrapping_sub(start) < len)
    }

    /// How soon a reader needs the piece: 0 for one a reader is at, 1 for the next, and so on.
    pub(crate) fn rank(&self, piece: u32) -> Option<u32> {
        self.readers
            .iter()
            .filter_map(|&(start, len)| {
                let depth = piece.wrapping_sub(start);
                (depth < len).then_some(depth)
            })
            .min()
    }

    /// The first wanted piece, in turn order, that's missing from `have` and passes `accept`.
    /// Pieces already downloaded are skipped a word of bits at a time.
    pub(crate) fn first(
        &self,
        have: &BitSlice<u8, Msb0>,
        accept: impl Fn(u32) -> bool,
    ) -> Option<u32> {
        self.readers
            .iter()
            .enumerate()
            .filter_map(|(i, &(start, len))| {
                let missing = have.get(start as usize..(start + len) as usize)?;
                let piece = missing
                    .iter_zeros()
                    .map(|depth| start + depth as u32)
                    .find(|&p| accept(p))?;
                Some(((piece - start, i), piece))
            })
            .min_by_key(|&(turn, _)| turn)
            .map(|(_, piece)| piece)
    }

    /// The wanted pieces in turn order, each once. The scheduler only needs `first`.
    #[cfg(test)]
    pub(crate) fn pieces(&self) -> impl Iterator<Item = u32> + '_ {
        let rounds = self.readers.iter().map(|r| r.1).max().unwrap_or(0);
        (0..rounds).flat_map(move |round| {
            self.readers
                .iter()
                .enumerate()
                .filter_map(move |(i, &(start, len))| {
                    let piece = start + round;
                    // Another reader reached it in an earlier turn.
                    let earlier = self.readers.iter().enumerate().any(|(j, &(s, l))| {
                        let depth = piece.wrapping_sub(s);
                        j != i && depth < l && (depth < round || (depth == round && j < i))
                    });
                    (round < len && !earlier).then_some(piece)
                })
        })
    }
}

/// Result of attempting to acquire a piece.
#[derive(Debug)]
pub enum AcquireResult {
    /// A new piece was reserved from the queue.
    Reserved(ValidPieceIndex),
    /// A piece was stolen from a slower peer.
    Stolen {
        piece: ValidPieceIndex,
        from_peer: PeerHandle,
    },
    /// No pieces are available for this peer.
    NoneAvailable,
}

/// Parameters for acquiring a piece.
pub struct AcquireRequest<'a, I, P, S>
where
    I: Iterator<Item = ValidPieceIndex>,
    P: Fn(ValidPieceIndex) -> bool,
    S: Fn(ValidPieceIndex) -> bool,
{
    /// The peer requesting a piece.
    pub peer: PeerHandle,
    /// The peer's average piece download time (for steal calculations).
    pub peer_avg_time: Option<Duration>,
    /// Priority pieces to check first (e.g., for streaming).
    pub priority_pieces: I,
    /// File download priority ordering.
    pub file_priorities: &'a FilePriorities,
    /// File metadata for iterating pieces.
    pub file_infos: &'a FileInfos,
    /// Returns true if the peer has the given piece.
    pub peer_has_piece: P,
    /// Returns true if the piece can be stolen (e.g., not locked for writing).
    pub can_steal: S,
}

/// Coordinates piece download state.
///
/// Wraps a [`ChunkTracker`] with tracking of which pieces are currently being downloaded
/// (in-flight) and by which peer. This ensures that:
///
/// - A piece is only assigned to one peer at a time (unless stolen)
/// - Pieces are properly requeued when a peer dies
/// - State transitions maintain invariants
pub struct PieceTracker {
    chunks: ChunkTracker,
    inflight: HashMap<ValidPieceIndex, InflightPiece>,
    verifying: HashSet<ValidPieceIndex>,
    /// When each live peer last delivered a chunk, for the RAM stall rule.
    last_delivery: HashMap<PeerHandle, Instant>,
}

impl PieceTracker {
    // === CONSTRUCTION ===

    /// Create a new PieceTracker wrapping the given ChunkTracker.
    pub fn new(chunks: ChunkTracker) -> Self {
        Self {
            chunks,
            inflight: HashMap::new(),
            verifying: HashSet::new(),
            last_delivery: HashMap::new(),
        }
    }

    /// Read-only access to the underlying ChunkTracker.
    pub fn chunks(&self) -> &ChunkTracker {
        &self.chunks
    }

    /// Consume the PieceTracker, requeuing any in-flight pieces.
    ///
    /// This is used when pausing a torrent - any pieces that were being downloaded
    /// need to be put back in the queue so they can be re-downloaded on resume.
    pub fn into_chunks(mut self) -> ChunkTracker {
        // Requeue all in-flight pieces so they'll be re-downloaded on resume
        for piece in self.inflight.into_keys() {
            self.chunks.mark_piece_broken_if_not_have(piece);
        }
        self.chunks
    }

    // === PIECE ACQUISITION ===

    /// Attempt to acquire a piece for the requesting peer.
    ///
    /// The acquisition strategy is:
    /// 1. Try to steal a piece from a peer that's 10x slower
    /// 2. Try to reserve a piece from the queue (priority pieces first)
    /// 3. Try to steal a piece from a peer that's 3x slower
    ///
    /// If `Stolen` is returned, the caller MUST call `peers.on_steal()` to notify
    /// the old peer and update counters.
    pub fn acquire_piece<I, P, S>(&mut self, mut req: AcquireRequest<I, P, S>) -> AcquireResult
    where
        I: Iterator<Item = ValidPieceIndex>,
        P: Fn(ValidPieceIndex) -> bool,
        S: Fn(ValidPieceIndex) -> bool,
    {
        // 1. Try steal with 10x threshold (very slow peer)
        if let Some(result) = self.try_steal(&req, 10.0) {
            return result;
        }

        // 2. Try reserve from priority_pieces then queued pieces
        // First check priority pieces that aren't already downloaded or in-flight
        for piece in &mut req.priority_pieces {
            if !self.chunks.is_piece_have(piece)
                && !self.inflight.contains_key(&piece)
                && (req.peer_has_piece)(piece)
            {
                return self.reserve_piece(piece, req.peer);
            }
        }

        // Then check naturally ordered queued pieces
        // Note: iter_queued_pieces only returns pieces in queue_pieces (not in-flight)
        let queued: Vec<_> = self
            .chunks
            .iter_queued_pieces(req.file_priorities, req.file_infos)
            .collect();

        for piece in queued {
            if (req.peer_has_piece)(piece) {
                return self.reserve_piece(piece, req.peer);
            }
        }

        // 3. Try steal with 3x threshold (moderately slow peer)
        if let Some(result) = self.try_steal(&req, 3.0) {
            return result;
        }

        AcquireResult::NoneAvailable
    }

    /// Reserve a piece: remove from queue, add to inflight.
    fn reserve_piece(&mut self, piece: ValidPieceIndex, peer: PeerHandle) -> AcquireResult {
        self.chunks.reserve_needed_piece(piece);
        self.inflight.insert(
            piece,
            InflightPiece {
                peer,
                started: Instant::now(),
                taken_over: false,
            },
        );
        AcquireResult::Reserved(piece)
    }

    /// Try to steal a piece from a slower peer.
    fn try_steal<I, P, S>(
        &mut self,
        req: &AcquireRequest<I, P, S>,
        threshold: f64,
    ) -> Option<AcquireResult>
    where
        I: Iterator<Item = ValidPieceIndex>,
        P: Fn(ValidPieceIndex) -> bool,
        S: Fn(ValidPieceIndex) -> bool,
    {
        let my_avg = req.peer_avg_time?;
        let min_elapsed = Duration::from_secs_f64(my_avg.as_secs_f64() * threshold);

        // Find the slowest piece from another peer that exceeds threshold
        // and that the stealing peer actually has (can download)
        let (piece, old_peer, _) = self
            .inflight
            .iter()
            .filter(|(_, info)| info.peer != req.peer)
            .filter(|(p, _)| (req.peer_has_piece)(**p))
            .map(|(p, info)| (*p, info.peer, info.started.elapsed()))
            .filter(|(_, _, elapsed)| *elapsed >= min_elapsed)
            .max_by_key(|(_, _, elapsed)| *elapsed)?;

        // Check can_steal (e.g., per_piece_lock)
        if !(req.can_steal)(piece) {
            return None;
        }

        // Update ownership (piece stays in inflight, just changes owner)
        let info = self.inflight.get_mut(&piece)?;
        info.peer = req.peer;
        info.started = Instant::now();

        Some(AcquireResult::Stolen {
            piece,
            from_peer: old_peer,
        })
    }

    // === PIECE COMPLETION ===

    /// Remove piece from inflight tracking (e.g., after all chunks received).
    ///
    /// Returns download duration if piece was in-flight.
    /// Note: Does NOT mark the piece as downloaded - caller should do hash check
    /// and then call `mark_piece_hash_ok` or `mark_piece_hash_failed`.
    /// Notes that `peer` delivered a chunk, for the RAM stall rule.
    pub(crate) fn delivered_by(&mut self, peer: PeerHandle) {
        self.last_delivery.insert(peer, Instant::now());
    }

    pub fn take_inflight(&mut self, piece: ValidPieceIndex) -> Option<Duration> {
        let inflight = self.inflight.remove(&piece)?;
        self.verifying.insert(piece);
        Some(inflight.started.elapsed())
    }

    /// Mark piece as downloaded after successful hash verification.
    pub fn mark_piece_hash_ok(&mut self, piece: ValidPieceIndex) {
        self.chunks.mark_piece_downloaded(piece);
    }

    /// Mark piece as failed after hash verification failure - requeues the piece.
    pub fn mark_piece_hash_failed(&mut self, piece: ValidPieceIndex) {
        self.verifying.remove(&piece);
        self.chunks.mark_piece_broken_if_not_have(piece);
    }

    pub(crate) fn finish_verifying(&mut self, piece: ValidPieceIndex) {
        self.verifying.remove(&piece);
    }

    pub(crate) fn stale_ram_pieces(
        &self,
        window: &RamWindow,
    ) -> Vec<(ValidPieceIndex, PeerHandle)> {
        self.inflight
            .iter()
            .filter(|(p, _)| !window.contains(p.get()))
            .map(|(&p, owner)| (p, owner.peer))
            .collect()
    }

    pub(crate) fn cancel_ram_piece(&mut self, piece: ValidPieceIndex) {
        self.inflight.remove(&piece);
        self.chunks.mark_piece_broken_if_not_have(piece);
    }

    /// A separate bounded scheduling path: no background file queue and no payload
    /// allocation until storage has granted a reservation. The caller holds the live lock.
    ///
    /// Before reserving a new piece, the peer takes over a wanted piece that's stuck with
    /// another peer: one a reader is at, from a peer 3 times slower; any other, from one 10
    /// times slower. With nothing left to reserve, any wanted piece from a peer 3 times
    /// slower. As in `acquire_piece`, but by urgency: a peer that chokes us mid-piece would
    /// otherwise keep the piece a reader is waiting for until it disconnects.
    ///
    /// Every peer can work on wanted pieces at once; the readers' window bounds them.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn acquire_ram_piece(
        &mut self,
        peer: PeerHandle,
        peer_avg_time: Option<Duration>,
        window: &RamWindow,
        files: &FileInfos,
        storage: &dyn crate::storage::TorrentStorage,
        peer_has: impl Fn(ValidPieceIndex) -> bool,
        can_steal: impl Fn(ValidPieceIndex) -> bool,
    ) -> Option<RamAcquired> {
        if let Some(stolen) =
            self.steal_ram_piece(peer, peer_avg_time, window, 10.0, &peer_has, &can_steal)
        {
            return Some(stolen);
        }
        let lengths = *self.chunks.get_lengths();
        let next = window.first(self.chunks.get_have_pieces().as_slice(), |p| {
            lengths.validate_piece_index(p).is_some_and(|p| {
                peer_has(p) && !self.inflight.contains_key(&p) && !self.verifying.contains(&p)
            })
        });
        if let Some(id) = next.and_then(|p| lengths.validate_piece_index(p)) {
            let evicted = {
                let (inflight, verifying) = (&self.inflight, &self.verifying);
                let protected = |p: u32| {
                    window.contains(p)
                        || lengths
                            .validate_piece_index(p)
                            .is_some_and(|p| inflight.contains_key(&p) || verifying.contains(&p))
                };
                storage.reserve_ram_piece(id, &protected)
            };
            // None: no slot is free or evictable, for this piece or any other.
            if let Some(evicted) = evicted {
                let mut evicted_bytes = 0;
                for old in evicted {
                    if let Some(old) = lengths.validate_piece_index(old) {
                        evicted_bytes += self.chunks.forget_ram_piece(old, files);
                    }
                }
                self.reserve_piece(id, peer);
                return Some(RamAcquired::Reserved {
                    piece: id,
                    evicted_bytes,
                });
            }
        }
        self.steal_ram_piece(peer, peer_avg_time, window, 3.0, &peer_has, &can_steal)
    }

    /// Takes over the most urgent wanted piece that another peer has sent nothing of for
    /// `RAM_STALL`, or has had for `slower` times this peer's average piece time (3 times for
    /// a piece a reader is at). The chunks already received stay; only the missing ones are
    /// requested again.
    fn steal_ram_piece(
        &mut self,
        peer: PeerHandle,
        peer_avg_time: Option<Duration>,
        window: &RamWindow,
        slower: f64,
        peer_has: &impl Fn(ValidPieceIndex) -> bool,
        can_steal: &impl Fn(ValidPieceIndex) -> bool,
    ) -> Option<RamAcquired> {
        let after = |times: f64| peer_avg_time.map(|avg| avg.mul_f64(times));
        let (slow_after, urgent_after) = (after(slower), after(slower.min(3.0)));
        // Few pieces are in flight; the window only ranks the ones that qualify.
        let (_, piece) = self
            .inflight
            .iter()
            .filter(|(_, info)| info.peer != peer)
            .filter_map(|(&p, info)| {
                let rank = window.rank(p.get())?;
                let limit = if rank == 0 { urgent_after } else { slow_after };
                // Quiet: the owner sent nothing at all since it took the piece.
                let heard = self
                    .last_delivery
                    .get(&info.peer)
                    .map_or(info.started, |&at| at.max(info.started));
                let stuck = heard.elapsed() >= RAM_STALL
                    || limit.is_some_and(|limit| info.started.elapsed() >= limit);
                stuck.then_some((rank, p))
            })
            .filter(|&(_, p)| peer_has(p) && can_steal(p))
            .min_by_key(|&(rank, p)| (rank, p.get()))?;
        let info = self.inflight.get_mut(&piece)?;
        let from_peer = std::mem::replace(&mut info.peer, peer);
        info.started = Instant::now();
        info.taken_over = true;
        Some(RamAcquired::Stolen { piece, from_peer })
    }

    /// Release all pieces owned by a peer (on peer death).
    ///
    /// Moves all pieces owned by the peer from IN_FLIGHT back to QUEUED.
    /// Returns the number of pieces released.
    pub fn release_pieces_owned_by(&mut self, peer: PeerHandle) -> usize {
        // Collect pieces to release (can't modify while iterating)
        let pieces_to_release: Vec<_> = self
            .inflight
            .iter()
            .filter(|(_, info)| info.peer == peer)
            .map(|(p, _)| *p)
            .collect();

        self.last_delivery.remove(&peer);
        let count = pieces_to_release.len();
        for piece in pieces_to_release {
            self.inflight.remove(&piece);
            self.chunks.mark_piece_broken_if_not_have(piece);
        }
        count
    }

    // === QUERIES ===

    /// Get the inflight info for a piece, if it's currently being downloaded.
    pub fn get_inflight(&self, piece: ValidPieceIndex) -> Option<&InflightPiece> {
        self.inflight.get(&piece)
    }

    /// Check if a piece is currently in-flight.
    #[allow(dead_code)]
    pub fn is_inflight(&self, piece: ValidPieceIndex) -> bool {
        self.inflight.contains_key(&piece)
    }

    /// Get the number of pieces currently in-flight.
    #[allow(dead_code)]
    pub fn inflight_count(&self) -> usize {
        self.inflight.len()
    }

    // === PASS-THROUGH METHODS ===

    /// Mark a chunk as downloaded. Returns the result indicating if the piece is complete.
    pub fn mark_chunk_downloaded(
        &mut self,
        piece: &Piece<ByteBuf<'_>>,
    ) -> Option<ChunkMarkingResult> {
        self.chunks.mark_chunk_downloaded(piece)
    }

    /// Update which files are selected for download.
    pub fn update_only_files(
        &mut self,
        file_infos: &FileInfos,
        new_only_files: &HashSet<usize>,
    ) -> anyhow::Result<crate::chunk_tracker::HaveNeededSelected> {
        self.chunks.update_only_files(file_infos, new_only_files)
    }

    /// Update per-file have bytes when a piece completes. Returns remaining bytes for the file.
    pub fn update_file_have_on_piece_completed(
        &mut self,
        piece_id: ValidPieceIndex,
        file_id: usize,
        file_info: &FileInfo,
    ) -> u64 {
        self.chunks
            .update_file_have_on_piece_completed(piece_id, file_id, file_info)
    }

    /// Flush the have pieces bitfield to disk.
    pub fn flush_have_pieces(&mut self, flush_async: bool) -> anyhow::Result<()> {
        self.chunks.get_have_pieces_mut().flush(flush_async)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{bitv::BitV as BitVTrait, type_aliases::BF};
    use librqbit_core::lengths::Lengths;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    fn peer(id: u8) -> PeerHandle {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, id)), 6881)
    }

    /// Create a simple ChunkTracker for testing.
    /// Creates a torrent with the specified number of pieces, all selected.
    fn make_test_chunk_tracker(num_pieces: u32) -> ChunkTracker {
        // Create a simple single-file torrent
        let piece_length = 16384u32; // 16KB pieces
        let total_length = piece_length as u64 * num_pieces as u64;

        let lengths = Lengths::new(total_length, piece_length).unwrap();

        let bf_len = lengths.piece_bitfield_bytes();

        // No pieces downloaded yet (empty have)
        let have = BF::from_boxed_slice(vec![0u8; bf_len].into_boxed_slice());

        // All pieces selected
        let mut selected = BF::from_boxed_slice(vec![0u8; bf_len].into_boxed_slice());
        for i in 0..num_pieces as usize {
            selected.set(i, true);
        }

        // Single file spanning all pieces
        let file_infos = vec![crate::file_info::FileInfo {
            relative_filename: "test.dat".into(),
            offset_in_torrent: 0,
            len: total_length,
            piece_range: 0..num_pieces,
            attrs: Default::default(),
        }];

        ChunkTracker::new(have.into_dyn(), selected, lengths, &file_infos).unwrap()
    }

    fn make_test_file_infos(num_pieces: u32) -> FileInfos {
        let piece_length = 16384u64;
        vec![crate::file_info::FileInfo {
            relative_filename: "test.dat".into(),
            offset_in_torrent: 0,
            len: piece_length * num_pieces as u64,
            piece_range: 0..num_pieces,
            attrs: Default::default(),
        }]
    }

    fn make_default_file_priorities(file_infos: &FileInfos) -> FilePriorities {
        (0..file_infos.len()).collect()
    }

    #[test]
    fn test_new_piece_tracker() {
        let chunks = make_test_chunk_tracker(10);
        let tracker = PieceTracker::new(chunks);
        assert_eq!(tracker.inflight_count(), 0);
    }

    #[test]
    fn test_reserve_piece_from_queue() {
        let chunks = make_test_chunk_tracker(5);
        let mut tracker = PieceTracker::new(chunks);

        let file_infos = make_test_file_infos(5);
        let file_priorities = make_default_file_priorities(&file_infos);

        let result = tracker.acquire_piece(AcquireRequest {
            peer: peer(1),
            peer_avg_time: None,
            priority_pieces: std::iter::empty(),
            file_priorities: &file_priorities,
            file_infos: &file_infos,
            peer_has_piece: |_| true, // Peer has all pieces
            can_steal: |_| true,
        });

        // Should reserve piece 0 (first in queue)
        match result {
            AcquireResult::Reserved(piece) => {
                assert_eq!(piece.get(), 0);
                assert!(tracker.is_inflight(piece));
                assert_eq!(tracker.inflight_count(), 1);
            }
            _ => panic!("Expected Reserved, got {:?}", result),
        }
    }

    #[test]
    fn test_reserve_filters_by_peer_has_piece() {
        let chunks = make_test_chunk_tracker(5);
        let mut tracker = PieceTracker::new(chunks);

        let file_infos = make_test_file_infos(5);
        let file_priorities = make_default_file_priorities(&file_infos);

        // Peer only has piece 2 and later
        // Note: iter_queued_pieces iterates in order: first, last, middle
        // So for 0..5: 0, 4, 1, 2, 3
        // With filter >= 2, we get 4 first (skips 0, takes 4)
        let result = tracker.acquire_piece(AcquireRequest {
            peer: peer(1),
            peer_avg_time: None,
            priority_pieces: std::iter::empty(),
            file_priorities: &file_priorities,
            file_infos: &file_infos,
            peer_has_piece: |p| p.get() >= 2,
            can_steal: |_| true,
        });

        match result {
            AcquireResult::Reserved(piece) => {
                // Got piece 4 (first one peer has in iteration order)
                assert!(piece.get() >= 2, "Should have gotten a piece >= 2");
            }
            _ => panic!("Expected Reserved, got {:?}", result),
        }
    }

    #[test]
    fn test_complete_piece() {
        let chunks = make_test_chunk_tracker(5);
        let mut tracker = PieceTracker::new(chunks);

        let file_infos = make_test_file_infos(5);
        let file_priorities = make_default_file_priorities(&file_infos);

        // Reserve a piece first
        let result = tracker.acquire_piece(AcquireRequest {
            peer: peer(1),
            peer_avg_time: None,
            priority_pieces: std::iter::empty(),
            file_priorities: &file_priorities,
            file_infos: &file_infos,
            peer_has_piece: |_| true,
            can_steal: |_| true,
        });

        let piece = match result {
            AcquireResult::Reserved(p) => p,
            _ => panic!("Expected Reserved"),
        };

        // Complete the piece (take_inflight + hash check + mark_piece_hash_ok)
        let duration = tracker.take_inflight(piece);
        assert!(duration.is_some());
        assert!(!tracker.is_inflight(piece));
        // Simulate successful hash check
        tracker.mark_piece_hash_ok(piece);
        assert!(tracker.chunks().is_piece_have(piece));
    }

    #[test]
    fn test_fail_piece_requeues() {
        let chunks = make_test_chunk_tracker(5);
        let mut tracker = PieceTracker::new(chunks);

        let file_infos = make_test_file_infos(5);
        let file_priorities = make_default_file_priorities(&file_infos);

        // Reserve piece 0
        let result = tracker.acquire_piece(AcquireRequest {
            peer: peer(1),
            peer_avg_time: None,
            priority_pieces: std::iter::empty(),
            file_priorities: &file_priorities,
            file_infos: &file_infos,
            peer_has_piece: |_| true,
            can_steal: |_| true,
        });

        let piece = match result {
            AcquireResult::Reserved(p) => p,
            _ => panic!("Expected Reserved"),
        };

        assert!(tracker.is_inflight(piece));

        // Fail the piece (take_inflight + hash check fails + mark_piece_hash_failed)
        let duration = tracker.take_inflight(piece);
        assert!(duration.is_some());
        // Simulate failed hash check
        tracker.mark_piece_hash_failed(piece);

        // Should no longer be in-flight
        assert!(!tracker.is_inflight(piece));
        // Should not be in have
        assert!(!tracker.chunks().is_piece_have(piece));
        // Should be back in queue - verify by trying to reserve it again
        let result2 = tracker.acquire_piece(AcquireRequest {
            peer: peer(2),
            peer_avg_time: None,
            priority_pieces: std::iter::empty(),
            file_priorities: &file_priorities,
            file_infos: &file_infos,
            peer_has_piece: |p| p == piece, // Only has the failed piece
            can_steal: |_| true,
        });

        match result2 {
            AcquireResult::Reserved(p) => assert_eq!(p, piece),
            _ => panic!("Expected piece to be re-reservable after fail"),
        }
    }

    #[test]
    fn test_release_pieces_owned_by_peer() {
        let chunks = make_test_chunk_tracker(5);
        let mut tracker = PieceTracker::new(chunks);

        let file_infos = make_test_file_infos(5);
        let file_priorities = make_default_file_priorities(&file_infos);

        let peer_a = peer(1);
        let peer_b = peer(2);

        // Peer A reserves first two pieces (order: 0, 4, 1, 2, 3)
        // So peer A gets pieces 0 and 4
        let piece_a1 = match tracker.acquire_piece(AcquireRequest {
            peer: peer_a,
            peer_avg_time: None,
            priority_pieces: std::iter::empty(),
            file_priorities: &file_priorities,
            file_infos: &file_infos,
            peer_has_piece: |_| true,
            can_steal: |_| true,
        }) {
            AcquireResult::Reserved(p) => p,
            _ => panic!("Expected Reserved"),
        };
        let piece_a2 = match tracker.acquire_piece(AcquireRequest {
            peer: peer_a,
            peer_avg_time: None,
            priority_pieces: std::iter::empty(),
            file_priorities: &file_priorities,
            file_infos: &file_infos,
            peer_has_piece: |_| true,
            can_steal: |_| true,
        }) {
            AcquireResult::Reserved(p) => p,
            _ => panic!("Expected Reserved"),
        };

        // Peer B reserves next piece
        let piece_b = match tracker.acquire_piece(AcquireRequest {
            peer: peer_b,
            peer_avg_time: None,
            priority_pieces: std::iter::empty(),
            file_priorities: &file_priorities,
            file_infos: &file_infos,
            peer_has_piece: |_| true,
            can_steal: |_| true,
        }) {
            AcquireResult::Reserved(p) => p,
            _ => panic!("Expected Reserved"),
        };

        assert_eq!(tracker.inflight_count(), 3);
        assert!(tracker.is_inflight(piece_a1));
        assert!(tracker.is_inflight(piece_a2));
        assert!(tracker.is_inflight(piece_b));

        // Peer A dies
        let released = tracker.release_pieces_owned_by(peer_a);
        assert_eq!(released, 2);
        assert_eq!(tracker.inflight_count(), 1); // Only peer B's piece remains

        // Verify peer B's piece is still in-flight
        assert!(tracker.is_inflight(piece_b));
        // Verify peer A's pieces are no longer in-flight
        assert!(!tracker.is_inflight(piece_a1));
        assert!(!tracker.is_inflight(piece_a2));
    }

    #[test]
    fn test_into_chunks_requeues_inflight() {
        let chunks = make_test_chunk_tracker(5);
        let mut tracker = PieceTracker::new(chunks);

        let file_infos = make_test_file_infos(5);
        let file_priorities = make_default_file_priorities(&file_infos);

        // Reserve pieces 0 and 1
        tracker.acquire_piece(AcquireRequest {
            peer: peer(1),
            peer_avg_time: None,
            priority_pieces: std::iter::empty(),
            file_priorities: &file_priorities,
            file_infos: &file_infos,
            peer_has_piece: |_| true,
            can_steal: |_| true,
        });
        tracker.acquire_piece(AcquireRequest {
            peer: peer(1),
            peer_avg_time: None,
            priority_pieces: std::iter::empty(),
            file_priorities: &file_priorities,
            file_infos: &file_infos,
            peer_has_piece: |_| true,
            can_steal: |_| true,
        });

        assert_eq!(tracker.inflight_count(), 2);

        // Convert back to chunks (simulates pause)
        let chunks = tracker.into_chunks();

        // Create a new tracker and verify pieces are back in queue
        let mut new_tracker = PieceTracker::new(chunks);
        let result = new_tracker.acquire_piece(AcquireRequest {
            peer: peer(2),
            peer_avg_time: None,
            priority_pieces: std::iter::empty(),
            file_priorities: &file_priorities,
            file_infos: &file_infos,
            peer_has_piece: |_| true,
            can_steal: |_| true,
        });

        // Should get piece 0 again (was requeued)
        match result {
            AcquireResult::Reserved(p) => assert_eq!(p.get(), 0),
            _ => panic!("Expected to reserve piece 0 after into_chunks"),
        }
    }

    #[test]
    fn test_priority_pieces_checked_first() {
        let chunks = make_test_chunk_tracker(5);
        let mut tracker = PieceTracker::new(chunks);

        let file_infos = make_test_file_infos(5);
        let file_priorities = make_default_file_priorities(&file_infos);

        // Priority pieces: piece 3, then piece 2
        let piece3 = tracker
            .chunks()
            .get_lengths()
            .validate_piece_index(3)
            .unwrap();
        let piece2 = tracker
            .chunks()
            .get_lengths()
            .validate_piece_index(2)
            .unwrap();
        let priority = vec![piece3, piece2];

        let result = tracker.acquire_piece(AcquireRequest {
            peer: peer(1),
            peer_avg_time: None,
            priority_pieces: priority.into_iter(),
            file_priorities: &file_priorities,
            file_infos: &file_infos,
            peer_has_piece: |_| true,
            can_steal: |_| true,
        });

        // Should get piece 3 (first priority piece)
        match result {
            AcquireResult::Reserved(p) => assert_eq!(p.get(), 3),
            _ => panic!("Expected Reserved(3), got {:?}", result),
        }
    }

    #[test]
    fn test_none_available_when_no_pieces() {
        let chunks = make_test_chunk_tracker(5);
        let mut tracker = PieceTracker::new(chunks);

        let file_infos = make_test_file_infos(5);
        let file_priorities = make_default_file_priorities(&file_infos);

        // Peer has no pieces
        let result = tracker.acquire_piece(AcquireRequest {
            peer: peer(1),
            peer_avg_time: None,
            priority_pieces: std::iter::empty(),
            file_priorities: &file_priorities,
            file_infos: &file_infos,
            peer_has_piece: |_| false, // Peer has nothing
            can_steal: |_| true,
        });

        match result {
            AcquireResult::NoneAvailable => {}
            _ => panic!("Expected NoneAvailable, got {:?}", result),
        }
    }

    #[test]
    fn test_take_inflight_nonexistent_piece_returns_none() {
        let chunks = make_test_chunk_tracker(5);
        let mut tracker = PieceTracker::new(chunks);
        let piece = tracker
            .chunks()
            .get_lengths()
            .validate_piece_index(0)
            .unwrap();

        // Try to take a piece that's not in-flight
        let result = tracker.take_inflight(piece);
        assert!(result.is_none());
    }

    #[test]
    fn test_steal_only_pieces_peer_has() {
        // This test verifies the fix for a bug where try_steal didn't check
        // if the stealing peer actually has the piece in their bitfield.
        // Without this check, a peer could "steal" a piece they can't download,
        // leaving it stuck in inflight forever.

        let chunks = make_test_chunk_tracker(5);
        let mut tracker = PieceTracker::new(chunks);

        let file_infos = make_test_file_infos(5);
        let file_priorities = make_default_file_priorities(&file_infos);

        let peer_a = peer(1);
        let peer_b = peer(2);

        // Peer A reserves pieces 0 and 4 (first two in iteration order)
        let piece_0 = match tracker.acquire_piece(AcquireRequest {
            peer: peer_a,
            peer_avg_time: None,
            priority_pieces: std::iter::empty(),
            file_priorities: &file_priorities,
            file_infos: &file_infos,
            peer_has_piece: |_| true,
            can_steal: |_| true,
        }) {
            AcquireResult::Reserved(p) => {
                assert_eq!(p.get(), 0);
                p
            }
            _ => panic!("Expected Reserved"),
        };

        let piece_4 = match tracker.acquire_piece(AcquireRequest {
            peer: peer_a,
            peer_avg_time: None,
            priority_pieces: std::iter::empty(),
            file_priorities: &file_priorities,
            file_infos: &file_infos,
            peer_has_piece: |_| true,
            can_steal: |_| true,
        }) {
            AcquireResult::Reserved(p) => {
                assert_eq!(p.get(), 4);
                p
            }
            _ => panic!("Expected Reserved"),
        };

        // Sleep briefly so pieces become stealable
        std::thread::sleep(Duration::from_millis(5));

        // Peer B tries to acquire with:
        // - Very short avg_time (1ms) so 3x threshold = 3ms < 5ms elapsed
        // - peer_has_piece returns true ONLY for piece 4, NOT piece 0
        let result = tracker.acquire_piece(AcquireRequest {
            peer: peer_b,
            peer_avg_time: Some(Duration::from_millis(1)),
            priority_pieces: std::iter::empty(),
            file_priorities: &file_priorities,
            file_infos: &file_infos,
            peer_has_piece: |p| p.get() == 4, // Peer B only has piece 4
            can_steal: |_| true,
        });

        // Should steal piece 4 (which peer B has), NOT piece 0 (which peer B doesn't have)
        match result {
            AcquireResult::Stolen { piece, from_peer } => {
                assert_eq!(piece, piece_4, "Should steal piece 4 (the one peer B has)");
                assert_eq!(from_peer, peer_a);
                // Verify piece 0 is still owned by peer A (wasn't stolen)
                assert_eq!(tracker.get_inflight(piece_0).unwrap().peer, peer_a);
            }
            _ => panic!("Expected Stolen, got {:?}", result),
        }
    }

    fn index(tracker: &PieceTracker, i: u32) -> ValidPieceIndex {
        tracker
            .chunks()
            .get_lengths()
            .validate_piece_index(i)
            .unwrap()
    }

    /// One reader at piece 0 with read-ahead to piece 5.
    fn window() -> RamWindow {
        RamWindow::interleave(&[(0, 5)], 10)
    }

    #[test]
    fn ram_piece_stuck_with_a_quiet_peer_goes_to_the_next_peer() {
        let mut tracker = PieceTracker::new(make_test_chunk_tracker(5));
        let (p0, p1) = (index(&tracker, 0), index(&tracker, 1));
        tracker.reserve_piece(p0, peer(1));
        tracker.reserve_piece(p1, peer(1));
        // Peer 1 took both pieces RAM_STALL ago and has sent nothing since: it choked us.
        for p in [p0, p1] {
            tracker.inflight.get_mut(&p).unwrap().started -= RAM_STALL;
        }

        // Peer 2 has no history yet, and takes the piece the reader is at first.
        let got = tracker.steal_ram_piece(peer(2), None, &window(), 10.0, &|_| true, &|_| true);
        assert_eq!(
            got,
            Some(RamAcquired::Stolen {
                piece: p0,
                from_peer: peer(1)
            })
        );
        let taken = tracker.get_inflight(p0).unwrap();
        assert_eq!(taken.peer, peer(2));
        assert!(
            taken.taken_over,
            "its time won't count toward peer 2's average"
        );
        assert_eq!(tracker.get_inflight(p1).unwrap().peer, peer(1));
    }

    #[test]
    fn a_piece_stays_with_a_peer_that_keeps_sending() {
        let mut tracker = PieceTracker::new(make_test_chunk_tracker(5));
        let (p0, p1) = (index(&tracker, 0), index(&tracker, 1));
        tracker.reserve_piece(p0, peer(1));
        tracker.reserve_piece(p1, peer(1));
        for p in [p0, p1] {
            tracker.inflight.get_mut(&p).unwrap().started -= RAM_STALL;
        }
        // Nothing of piece 1 has come yet, but peer 1 is still sending piece 0: a slow peer's
        // new piece waits behind its earlier requests.
        tracker.delivered_by(peer(1));

        let got = tracker.steal_ram_piece(peer(2), None, &window(), 10.0, &|_| true, &|_| true);
        assert_eq!(got, None);
        assert_eq!(tracker.get_inflight(p1).unwrap().peer, peer(1));

        // Once the peer is gone, its record goes too.
        tracker.release_pieces_owned_by(peer(1));
        assert!(tracker.last_delivery.is_empty());
    }

    #[test]
    fn slow_ram_piece_goes_to_a_much_faster_peer() {
        let mut tracker = PieceTracker::new(make_test_chunk_tracker(5));
        let p0 = index(&tracker, 0);
        tracker.reserve_piece(p0, peer(1));
        std::thread::sleep(Duration::from_millis(20));
        let fast = Some(Duration::from_millis(1));
        let take = |tracker: &mut PieceTracker, who, avg, has: bool, unlocked: bool| {
            tracker.steal_ram_piece(who, avg, &window(), 10.0, &|_| has, &|_| unlocked)
        };

        assert_eq!(
            take(&mut tracker, peer(2), fast, false, true),
            None,
            "lacks it"
        );
        assert_eq!(
            take(&mut tracker, peer(2), fast, true, false),
            None,
            "being written"
        );
        let similar = Some(Duration::from_secs(1));
        assert_eq!(
            take(&mut tracker, peer(2), similar, true, true),
            None,
            "not faster"
        );
        assert_eq!(
            take(&mut tracker, peer(1), fast, true, true),
            None,
            "its own"
        );
        assert_eq!(
            take(&mut tracker, peer(2), fast, true, true),
            Some(RamAcquired::Stolen {
                piece: p0,
                from_peer: peer(1)
            })
        );
        let taken = tracker.get_inflight(p0).unwrap();
        assert!(taken.started.elapsed() < Duration::from_millis(20));
    }

    #[test]
    fn the_piece_a_reader_is_at_moves_sooner() {
        let mut tracker = PieceTracker::new(make_test_chunk_tracker(5));
        let (p0, p2) = (index(&tracker, 0), index(&tracker, 2));
        tracker.reserve_piece(p0, peer(1));
        tracker.reserve_piece(p2, peer(1));
        std::thread::sleep(Duration::from_millis(100));
        // Peer 2 takes 20 ms a piece: after 60 ms for the reader's piece, 200 ms for the rest.
        let avg = Some(Duration::from_millis(20));
        let take = |tracker: &mut PieceTracker, slower| {
            tracker.steal_ram_piece(peer(2), avg, &window(), slower, &|_| true, &|_| true)
        };

        let urgent = Some(RamAcquired::Stolen {
            piece: p0,
            from_peer: peer(1),
        });
        assert_eq!(take(&mut tracker, 10.0), urgent);
        assert_eq!(take(&mut tracker, 10.0), None, "piece 2 has 100 more ms");
        // With nothing new to download, 3 times slower is enough for any wanted piece.
        let rest = Some(RamAcquired::Stolen {
            piece: p2,
            from_peer: peer(1),
        });
        assert_eq!(take(&mut tracker, 3.0), rest);
    }

    #[test]
    fn pieces_outside_the_window_are_stale() {
        let mut tracker = PieceTracker::new(make_test_chunk_tracker(5));
        let (p1, p4) = (index(&tracker, 1), index(&tracker, 4));
        tracker.reserve_piece(p1, peer(1));
        tracker.reserve_piece(p4, peer(2));
        let window = RamWindow::interleave(&[(0, 5)], 3);
        assert_eq!(tracker.stale_ram_pieces(&window), vec![(p4, peer(2))]);
    }

    use bitvec::vec::BitVec;

    /// The window as a list, the way it was built before it became ranges.
    fn listed(queues: &[(u32, u32)], limit: u32) -> Vec<u32> {
        let mut queues: std::collections::VecDeque<_> = queues.iter().map(|&(s, e)| s..e).collect();
        let mut seen = HashSet::new();
        let mut wanted = Vec::new();
        while wanted.len() < limit as usize {
            let Some(mut queue) = queues.pop_front() else {
                break;
            };
            if let Some(p) = queue.next() {
                if seen.insert(p) {
                    wanted.push(p);
                }
                queues.push_back(queue);
            }
        }
        wanted
    }

    #[test]
    fn ram_window_matches_listing_the_readers_pieces() {
        let mut seed = 7u64;
        let mut next = |n: u32| {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((seed >> 33) % n as u64) as u32
        };
        for _ in 0..5000 {
            let readers = 1 + next(4);
            let queues: Vec<(u32, u32)> = (0..readers)
                .map(|_| {
                    let start = next(40);
                    (start, start + next(30))
                })
                .collect();
            let limit = next(60);
            let window = RamWindow::interleave(&queues, limit);
            let expected = listed(&queues, limit);
            let pieces: Vec<u32> = window.pieces().collect();
            assert_eq!(pieces, expected, "{queues:?}, limit {limit}");
            for p in 0..80 {
                assert_eq!(window.contains(p), expected.contains(&p), "piece {p}");
            }
            if let Some(&first) = expected.first() {
                assert_eq!(window.rank(first), Some(0));
            }
            let have: BitVec<u8, Msb0> = (0..80).map(|_| next(3) > 0).collect();
            let rejected = next(80);
            let accept = |p: u32| p != rejected;
            let first = expected
                .iter()
                .copied()
                .find(|&p| !have[p as usize] && accept(p));
            assert_eq!(
                window.first(&have, accept),
                first,
                "{queues:?}, limit {limit}"
            );
        }
    }
}
