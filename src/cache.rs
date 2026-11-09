//! One reusable slab, reserved before network requests. No payload files exist.
use anyhow::{Context, Result, ensure};
use librqbit::{
    ManagedTorrentShared, TorrentMetadata,
    storage::{BoxStorageFactory, StorageFactory, StorageFactoryExt, TorrentStorage},
};
use librqbit_core::lengths::ValidPieceIndex;
use parking_lot::Mutex;
use serde::Serialize;
use std::{
    collections::HashMap,
    path::Path,
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

#[derive(Clone)]
pub struct RamFactory {
    /// Shared by every clone, including the engine's, so a new size reaches the next torrent.
    budget: Arc<AtomicUsize>,
    occupied: Arc<AtomicBool>,
    current: Arc<Mutex<Weak<RamCache>>>,
}

impl RamFactory {
    pub fn new(budget: usize) -> Self {
        Self {
            budget: Arc::new(AtomicUsize::new(budget)),
            occupied: Default::default(),
            current: Default::default(),
        }
    }
    /// The size of the next torrent's cache.
    pub fn budget(&self) -> usize {
        self.budget.load(Ordering::Relaxed)
    }
    /// Settings → Cache. A loaded torrent keeps the cache it was given.
    pub fn set_budget(&self, bytes: usize) {
        self.budget.store(bytes, Ordering::Relaxed);
    }
    pub fn snapshot(&self) -> CacheStats {
        self.current
            .lock()
            .upgrade()
            .map(|c| c.stats())
            .unwrap_or(CacheStats {
                budget_bytes: self.budget(),
                ..Default::default()
            })
    }
    pub fn is_occupied(&self) -> bool {
        self.occupied.load(Ordering::Acquire)
    }
}

impl StorageFactory for RamFactory {
    type Storage = RamStorage;
    fn create(&self, _: &ManagedTorrentShared, meta: &TorrentMetadata) -> Result<RamStorage> {
        ensure!(
            meta.lengths().default_piece_length() >= 16384,
            "Torrent pieces must be at least 16 KiB for this memory profile"
        );
        ensure!(
            meta.lengths().total_pieces() <= 131_072,
            "Torrent has too many pieces (maximum 131072)"
        );
        ensure!(
            meta.file_infos.len() <= 4096,
            "Torrent has too many files (maximum 4096)"
        );
        ensure!(
            meta.lengths().total_length() <= 512 * 1024_u64.pow(3),
            "Torrent exceeds the 512 GiB metadata budget"
        );
        ensure!(
            self.occupied
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok(),
            "Previous torrent is still releasing its cache; try again shortly"
        );
        let cache = match RamCache::new(
            self.budget(),
            meta.lengths().default_piece_length() as usize,
            self.occupied.clone(),
        ) {
            Ok(c) => Arc::new(c),
            Err(e) => {
                self.occupied.store(false, Ordering::Release);
                return Err(e);
            }
        };
        *self.current.lock() = Arc::downgrade(&cache);
        Ok(RamStorage {
            cache,
            files: meta
                .file_infos
                .iter()
                .map(|f| (f.offset_in_torrent, f.len))
                .collect(),
        })
    }
    fn clone_box(&self) -> BoxStorageFactory {
        self.clone().boxed()
    }
}

#[derive(Clone)]
pub struct RamStorage {
    cache: Arc<RamCache>,
    files: Vec<(u64, u64)>,
}

#[derive(Default, Serialize)]
pub struct CacheStats {
    pub budget_bytes: usize,
    pub allocated_bytes: usize,
    pub reserved_bytes: usize,
    pub cached_pieces: usize,
    pub slots: usize,
    pub piece_bytes: usize,
    pub peak_reserved_bytes: usize,
    pub evictions: u64,
    pub reservations_denied: u64,
}

struct Slot {
    offset: usize,
    touched: u64,
}
struct Inner {
    slab: Option<memmap2::MmapMut>,
    pieces: HashMap<u32, Slot>,
    free: Vec<usize>,
    clock: u64,
    peak: usize,
    evictions: u64,
    denied: u64,
}
struct RamCache {
    budget: usize,
    piece_len: usize,
    slot_count: usize,
    inner: Mutex<Inner>,
    occupied: Arc<AtomicBool>,
}
impl Drop for RamCache {
    fn drop(&mut self) {
        // munmap returns pages immediately instead of retaining them in malloc arenas.
        self.inner.get_mut().slab.take();
        self.occupied.store(false, Ordering::Release);
    }
}

impl RamCache {
    fn new(budget: usize, piece_len: usize, occupied: Arc<AtomicBool>) -> Result<Self> {
        ensure!(piece_len > 0, "Invalid zero piece length");
        let slots = budget / piece_len;
        ensure!(
            slots >= 3,
            "This torrent's pieces are {} MiB, and the cache must hold at least three of them. Choose a bigger cache (Settings → Cache in the Mac app, or --cache-mib), or a torrent with smaller pieces.",
            piece_len.div_ceil(1 << 20)
        );
        let size = slots
            .checked_mul(piece_len)
            .context("Cache size overflow")?;
        let slab = memmap2::MmapMut::map_anon(size).context("Unable to allocate RAM cache")?;
        Ok(Self {
            budget,
            piece_len,
            slot_count: slots,
            occupied,
            inner: Mutex::new(Inner {
                slab: Some(slab),
                pieces: HashMap::new(),
                free: (0..slots).map(|i| i * piece_len).collect(),
                clock: 0,
                peak: 0,
                evictions: 0,
                denied: 0,
            }),
        })
    }

    fn reserve(&self, piece: u32, protected: &dyn Fn(u32) -> bool) -> Option<Vec<u32>> {
        let mut g = self.inner.lock();
        if g.pieces.contains_key(&piece) {
            return Some(Vec::new());
        }
        let mut evicted = Vec::new();
        let offset = if let Some(offset) = g.free.pop() {
            offset
        } else {
            let oldest = g
                .pieces
                .iter()
                .filter(|(id, _)| !protected(**id))
                .min_by_key(|(_, slot)| slot.touched)
                .map(|(&id, _)| id);
            let Some(oldest) = oldest else {
                g.denied += 1;
                return None;
            };
            let slot = g.pieces.remove(&oldest).unwrap();
            g.evictions += 1;
            evicted.push(oldest);
            slot.offset
        };
        // Padding bytes and partial retries must never expose data from the old piece.
        g.slab.as_mut().unwrap()[offset..offset + self.piece_len].fill(0);
        g.clock += 1;
        let touched = g.clock;
        g.pieces.insert(piece, Slot { offset, touched });
        g.peak = g.peak.max(g.pieces.len() * self.piece_len);
        Some(evicted)
    }

    fn release(&self, piece: u32) {
        let mut g = self.inner.lock();
        if let Some(slot) = g.pieces.remove(&piece) {
            g.free.push(slot.offset);
        }
    }

    fn read(&self, mut absolute: u64, mut out: &mut [u8]) -> Result<()> {
        let mut g = self.inner.lock();
        while !out.is_empty() {
            let id: u32 = (absolute / self.piece_len as u64).try_into()?;
            let within = (absolute % self.piece_len as u64) as usize;
            let len = out.len().min(self.piece_len - within);
            g.clock += 1;
            let touched = g.clock;
            let slot = g.pieces.get_mut(&id).context("Piece is not resident")?;
            slot.touched = touched;
            let start = slot.offset + within;
            out[..len].copy_from_slice(&g.slab.as_ref().unwrap()[start..start + len]);
            out = &mut out[len..];
            absolute += len as u64;
        }
        Ok(())
    }

    fn write(&self, mut absolute: u64, mut data: &[u8]) -> Result<()> {
        let mut g = self.inner.lock();
        while !data.is_empty() {
            let id: u32 = (absolute / self.piece_len as u64).try_into()?;
            let within = (absolute % self.piece_len as u64) as usize;
            let len = data.len().min(self.piece_len - within);
            let start = g
                .pieces
                .get(&id)
                .context("Unreserved write rejected")?
                .offset
                + within;
            g.slab.as_mut().unwrap()[start..start + len].copy_from_slice(&data[..len]);
            data = &data[len..];
            absolute += len as u64;
        }
        Ok(())
    }

    fn stats(&self) -> CacheStats {
        let g = self.inner.lock();
        CacheStats {
            budget_bytes: self.budget,
            allocated_bytes: g.slab.as_ref().unwrap().len(),
            reserved_bytes: g.pieces.len() * self.piece_len,
            cached_pieces: g.pieces.len(),
            slots: self.slot_count,
            piece_bytes: self.piece_len,
            peak_reserved_bytes: g.peak,
            evictions: g.evictions,
            reservations_denied: g.denied,
        }
    }
}

impl RamStorage {
    fn absolute(&self, file: usize, offset: u64, len: usize) -> Result<u64> {
        let &(start, size) = self.files.get(file).context("Invalid file index")?;
        ensure!(
            offset
                .checked_add(len as u64)
                .is_some_and(|end| end <= size),
            "File access out of bounds"
        );
        start.checked_add(offset).context("File offset overflow")
    }
}
impl TorrentStorage for RamStorage {
    fn init(&mut self, _: &ManagedTorrentShared, _: &TorrentMetadata) -> Result<()> {
        Ok(())
    }
    fn pread_exact(&self, file: usize, off: u64, out: &mut [u8]) -> Result<()> {
        self.cache.read(self.absolute(file, off, out.len())?, out)
    }
    fn pwrite_all(&self, file: usize, off: u64, data: &[u8]) -> Result<()> {
        self.cache
            .write(self.absolute(file, off, data.len())?, data)
    }
    fn remove_file(&self, _: usize, _: &Path) -> Result<()> {
        Ok(())
    }
    fn remove_directory_if_empty(&self, _: &Path) -> Result<()> {
        Ok(())
    }
    fn ensure_file_length(&self, _: usize, _: u64) -> Result<()> {
        Ok(())
    }
    fn take(&self) -> Result<Box<dyn TorrentStorage>> {
        Ok(Box::new(self.clone()))
    }
    fn ram_cache_slots(&self) -> Option<usize> {
        Some(self.cache.slot_count)
    }
    fn reserve_ram_piece(
        &self,
        id: ValidPieceIndex,
        protected: &dyn Fn(u32) -> bool,
    ) -> Option<Vec<u32>> {
        self.cache.reserve(id.get(), protected)
    }
    fn release_ram_piece(&self, id: ValidPieceIndex) {
        self.cache.release(id.get());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn cache() -> RamCache {
        RamCache::new(12 * 1024, 4096, Default::default()).unwrap()
    }
    #[test]
    fn reserves_before_writing_and_never_exceeds_budget() {
        let c = cache();
        assert!(c.write(0, &[1]).is_err());
        for p in 0..3 {
            assert!(c.reserve(p, &|_| false).is_some());
        }
        assert!(c.reserve(3, &|p| p <= 2).is_none());
        assert_eq!(c.stats().allocated_bytes, 12288);
        assert_eq!(c.stats().peak_reserved_bytes, 12288);
        assert_eq!(c.reserve(3, &|p| p == 0 || p == 2), Some(vec![1]));
        assert!(c.read(4096, &mut [0]).is_err());
    }
    #[test]
    fn reuse_zeroes_old_data_and_cross_piece_io_works() {
        let c = cache();
        c.reserve(0, &|_| false);
        c.reserve(1, &|_| false);
        c.write(4090, &[7; 16]).unwrap();
        let mut buf = [0; 16];
        c.read(4090, &mut buf).unwrap();
        assert_eq!(buf, [7; 16]);
        c.release(0);
        c.reserve(4, &|_| false);
        c.read(4 * 4096 + 4090, &mut [0; 6]).unwrap();
        let mut zero = [1; 6];
        c.read(4 * 4096 + 4090, &mut zero).unwrap();
        assert_eq!(zero, [0; 6]);
    }
    #[test]
    fn concurrent_reservations_share_a_single_budget() {
        let c = Arc::new(cache());
        let threads: Vec<_> = (0..8)
            .map(|i| {
                let c = c.clone();
                std::thread::spawn(move || {
                    for n in 0..100 {
                        c.reserve(i * 100 + n, &|_| false);
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        assert_eq!(c.stats().peak_reserved_bytes, 12288);
        assert_eq!(c.stats().cached_pieces, 3);
    }
    #[test]
    fn many_small_slots_evict_the_oldest_unprotected_piece_quickly() {
        // 4096 slots, as with small pieces. The old list scan made each eviction
        // check every slot against every protected piece: about 16 million steps.
        let c = RamCache::new(4096 * 4096, 4096, Default::default()).unwrap();
        for p in 0..4096 {
            assert_eq!(c.reserve(p, &|_| false), Some(vec![]));
        }
        let started = std::time::Instant::now();
        for p in 4096..4608 {
            // Everything from the newest 4000 pieces down is protected, like a reader's window.
            assert_eq!(
                c.reserve(p, &|q| (p - 4000..p).contains(&q)),
                Some(vec![p - 4096])
            );
        }
        assert!(
            started.elapsed() < std::time::Duration::from_secs(20),
            "eviction is too slow"
        );
        assert_eq!(c.stats().evictions, 512);
        assert_eq!(c.stats().cached_pieces, 4096);
    }
    #[test]
    fn a_new_cache_size_reaches_the_engines_copy() {
        let factory = RamFactory::new(96 << 20);
        let engine = factory.clone();
        factory.set_budget(192 << 20);
        assert_eq!(engine.budget(), 192 << 20);
        assert_eq!(factory.snapshot().budget_bytes, 192 << 20);
    }
    #[test]
    fn rejects_pieces_that_cannot_fit_working_set() {
        assert!(RamCache::new(4096, 4096, Default::default()).is_err());
    }
}
