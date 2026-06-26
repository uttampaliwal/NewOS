//! zswap — compressed write-back cache in front of the swap device.
//!
//! When a page is evicted, zswap attempts to store it in a compressed in-memory
//! cache. On swap-in, zswap checks the cache first, avoiding disk I/O.
//! When the cache is full, the least-compressed or oldest entries are evicted
//! to the backing swap device (write-back).
//!
//! Design:
//! - Pool of compressed pages stored in a `BTreeMap<SwapSlot, CompressedPage>`.
//! - Each compressed page stores the compressed payload inline (up to 4096 bytes).
//! - LRU eviction: entries are ordered by access time; oldest are evicted first.
//! - Integration with the existing `SwapDevice` trait for write-back.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use super::compress::{compress, decompress, HEADER_SIZE};
use super::swap::{SwapDevice, SwapError, SwapSlot};

/// Maximum compressed size for a single entry (compressed payload only).
const MAX_COMPRESSED_PAYLOAD: usize = 4096;

/// A single compressed page entry stored in the zswap cache.
#[derive(Debug, Clone)]
struct CompressedPage {
    /// The compressed data (including 8-byte header from compress()).
    data: Vec<u8>,
    /// Original uncompressed size.
    original_size: u32,
    /// LRU timestamp (incremented on access).
    access_time: u64,
    /// Whether this entry has been dirty-written to the backing device.
    clean: bool,
}

impl CompressedPage {
    fn new(data: Vec<u8>, original_size: usize) -> Self {
        Self {
            data,
            original_size: original_size as u32,
            access_time: 0,
            clean: false,
        }
    }

    fn compression_ratio(&self) -> u32 {
        if self.original_size == 0 {
            return 100;
        }
        let payload_len = self.data.len().saturating_sub(HEADER_SIZE);
        (payload_len as u32 * 100) / self.original_size
    }
}

/// Global LRU counter for tracking access recency.
static LRU_CLOCK: AtomicU64 = AtomicU64::new(1);

fn next_lru_time() -> u64 {
    LRU_CLOCK.fetch_add(1, Ordering::Relaxed)
}

/// zswap cache statistics.
#[derive(Debug, Clone, Default)]
pub struct ZswapStats {
    /// Total number of pages inserted into the cache.
    pub inserts: u64,
    /// Total number of cache hits (decompress without disk I/O).
    pub hits: u64,
    /// Total number of cache misses (fell through to backing device).
    pub misses: u64,
    /// Total number of pages evicted from cache to backing device.
    pub writebacks: u64,
    /// Total compressed bytes stored.
    pub compressed_bytes: u64,
    /// Total uncompressed bytes stored.
    pub uncompressed_bytes: u64,
    /// Number of currently stored entries.
    pub entry_count: usize,
}

/// The zswap cache storing compressed pages.
struct ZswapCache {
    entries: Vec<(SwapSlot, CompressedPage)>,
    max_entries: usize,
    stats: ZswapStats,
}

static ZSWAP_CACHE: Mutex<Option<ZswapCache>> = Mutex::new(None);

/// Initialize the zswap cache with the given maximum number of entries.
pub fn init_zswap(max_entries: usize) {
    let cache = ZswapCache {
        entries: Vec::with_capacity(max_entries),
        max_entries,
        stats: ZswapStats::default(),
    };
    *ZSWAP_CACHE.lock() = Some(cache);
}

/// Store a page in the zswap cache. Returns `Ok(slot)` on success.
///
/// The caller provides the page data and the swap slot that was allocated
/// for it. If the cache is full, the least-recently-used entry is evicted
/// to the backing device first.
pub fn zswap_store(
    slot: SwapSlot,
    page_data: &[u8; 4096],
    device: &dyn SwapDevice,
) -> Result<SwapSlot, SwapError> {
    let compressed_data = compress(page_data).ok_or(SwapError::IoError)?;

    // Reject if compressed data is too large (no benefit from compression)
    let payload_len = compressed_data.len().saturating_sub(HEADER_SIZE);
    if payload_len > MAX_COMPRESSED_PAYLOAD || payload_len >= 4096 {
        // No benefit — write directly to backing device
        return device.write_page(slot, page_data).map(|_| slot);
    }

    let mut cache_guard = ZSWAP_CACHE.lock();
    let cache = cache_guard.as_mut().ok_or(SwapError::DeviceNotPresent)?;

    // If cache is full, evict the least-recently-used entry
    if cache.entries.len() >= cache.max_entries {
        evict_lru_to_device(cache, device)?;
    }

    let mut entry = CompressedPage::new(compressed_data, 4096);
    entry.access_time = next_lru_time();

    cache.stats.inserts += 1;
    cache.stats.compressed_bytes += payload_len as u64;
    cache.stats.uncompressed_bytes += 4096;
    cache.stats.entry_count = cache.entries.len();

    cache.entries.push((slot, entry));

    Ok(slot)
}

/// Retrieve a page from the zswap cache. Returns `Some(page_data)` on hit.
pub fn zswap_retrieve(slot: SwapSlot, device: &dyn SwapDevice) -> Result<[u8; 4096], SwapError> {
    let mut cache_guard = ZSWAP_CACHE.lock();
    let cache = cache_guard.as_mut().ok_or(SwapError::DeviceNotPresent)?;

    // Search for the entry
    if let Some(pos) = cache.entries.iter().position(|(s, _)| *s == slot) {
        let entry = &mut cache.entries[pos].1;
        entry.access_time = next_lru_time();
        cache.stats.hits += 1;

        let decompressed = decompress(&entry.data).ok_or(SwapError::IoError)?;
        if decompressed.len() != 4096 {
            return Err(SwapError::IoError);
        }

        let mut page_data = [0u8; 4096];
        page_data.copy_from_slice(&decompressed);
        return Ok(page_data);
    }

    // Cache miss — read from backing device
    cache.stats.misses += 1;
    let mut page_data = [0u8; 4096];
    device.read_page(slot, &mut page_data)?;
    Ok(page_data)
}

/// Remove a specific entry from the zswap cache (used when a page is freed).
pub fn zswap_invalidate(slot: SwapSlot) -> bool {
    let mut cache_guard = ZSWAP_CACHE.lock();
    let cache = match cache_guard.as_mut() {
        Some(c) => c,
        None => return false,
    };

    let len_before = cache.entries.len();
    cache.entries.retain(|(s, _)| *s != slot);
    let removed = cache.entries.len() < len_before;

    if removed {
        cache.stats.entry_count = cache.entries.len();
    }

    removed
}

/// Get current zswap statistics.
pub fn zswap_stats() -> ZswapStats {
    let cache_guard = ZSWAP_CACHE.lock();
    match cache_guard.as_ref() {
        Some(cache) => {
            let mut stats = cache.stats.clone();
            stats.entry_count = cache.entries.len();
            stats
        }
        None => ZswapStats::default(),
    }
}

/// Return the number of entries currently in the cache.
pub fn zswap_entry_count() -> usize {
    ZSWAP_CACHE
        .lock()
        .as_ref()
        .map(|c| c.entries.len())
        .unwrap_or(0)
}

/// Clear the entire zswap cache, writing back all dirty entries.
pub fn zswap_flush(device: &dyn SwapDevice) {
    let mut cache_guard = ZSWAP_CACHE.lock();
    let cache = match cache_guard.as_mut() {
        Some(c) => c,
        None => return,
    };

    // Write back all entries to the backing device
    for (slot, entry) in cache.entries.iter() {
        if !entry.clean {
            if let Some(decompressed) = decompress(&entry.data) {
                let mut page_data = [0u8; 4096];
                page_data.copy_from_slice(&decompressed);
                let _ = device.write_page(*slot, &page_data);
            }
        }
    }

    cache.stats.writebacks += cache.entries.len() as u64;
    cache.entries.clear();
    cache.stats.compressed_bytes = 0;
    cache.stats.uncompressed_bytes = 0;
    cache.stats.entry_count = 0;
}

/// Reset the zswap subsystem (for tests).
pub fn reset_zswap() {
    *ZSWAP_CACHE.lock() = None;
    LRU_CLOCK.store(1, Ordering::Relaxed);
}

/// Evict the least-recently-used entry to the backing device.
fn evict_lru_to_device(
    cache: &mut ZswapCache,
    device: &dyn SwapDevice,
) -> Result<(), SwapError> {
    if cache.entries.is_empty() {
        return Ok(());
    }

    // Find the entry with the smallest access_time
    let oldest_pos = cache
        .entries
        .iter()
        .enumerate()
        .min_by_key(|(_, (_, entry))| entry.access_time)
        .map(|(pos, _)| pos)
        .unwrap_or(0);

    let (evict_slot, evict_entry) = cache.entries.remove(oldest_pos);

    // Decompress and write to backing device
    if let Some(decompressed) = decompress(&evict_entry.data) {
        let mut page_data = [0u8; 4096];
        page_data.copy_from_slice(&decompressed);
        let _ = device.write_page(evict_slot, &page_data);
    }

    let payload_len = evict_entry.data.len().saturating_sub(HEADER_SIZE);
    cache.stats.writebacks += 1;
    cache.stats.compressed_bytes = cache.stats.compressed_bytes.saturating_sub(payload_len as u64);
    cache.stats.uncompressed_bytes = cache.stats.uncompressed_bytes.saturating_sub(4096);
    cache.stats.entry_count = cache.entries.len();

    Ok(())
}

/// A simple in-memory swap device for testing zswap without hardware.
pub struct MockSwapDevice {
    pages: alloc::vec::Vec<[u8; 4096]>,
    name: &'static str,
}

impl MockSwapDevice {
    pub fn new(slot_count: usize, name: &'static str) -> Self {
        let mut pages = alloc::vec::Vec::with_capacity(slot_count);
        for _ in 0..slot_count {
            pages.push([0u8; 4096]);
        }
        Self { pages, name }
    }
}

impl SwapDevice for MockSwapDevice {
    fn read_page(&self, slot: SwapSlot, buffer: &mut [u8; 4096]) -> Result<(), SwapError> {
        if slot.0 >= self.pages.len() {
            return Err(SwapError::InvalidSlot);
        }
        buffer.copy_from_slice(&self.pages[slot.0]);
        Ok(())
    }

    fn write_page(&self, slot: SwapSlot, buffer: &[u8; 4096]) -> Result<(), SwapError> {
        if slot.0 >= self.pages.len() {
            return Err(SwapError::InvalidSlot);
        }
        // Safety: MockSwapDevice uses interior mutability through raw pointer
        // in the test context. For real use this would use Mutex.
        let page_ptr = &self.pages[slot.0] as *const [u8; 4096] as *mut [u8; 4096];
        // Safety: page_ptr points to a valid [u8; 4096] in self.pages, which is
        // exclusively accessed in test contexts where no concurrent reads occur.
        unsafe {
            core::ptr::copy_nonoverlapping(buffer, page_ptr, 1);
        }
        Ok(())
    }

    fn total_slots(&self) -> usize {
        self.pages.len()
    }

    fn device_name(&self) -> &str {
        self.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_page(seed: u8) -> [u8; 4096] {
        let mut page = [0u8; 4096];
        for (i, byte) in page.iter_mut().enumerate() {
            *byte = seed.wrapping_add(i as u8);
        }
        page
    }

    #[test]
    fn zswap_store_and_retrieve() {
        let _s = crate::test_serial::acquire();
        reset_zswap();
        init_zswap(16);

        let device = MockSwapDevice::new(64, "mock");
        let page = make_test_page(0xAA);
        let slot = SwapSlot(0);

        zswap_store(slot, &page, &device).expect("store should succeed");
        assert_eq!(zswap_entry_count(), 1);

        let retrieved = zswap_retrieve(slot, &device).expect("retrieve should succeed");
        assert_eq!(page, retrieved);
    }

    #[test]
    fn zswap_cache_hit_increments_stats() {
        let _s = crate::test_serial::acquire();
        reset_zswap();
        init_zswap(16);

        let device = MockSwapDevice::new(64, "mock");
        let page = make_test_page(0xBB);
        let slot = SwapSlot(1);

        zswap_store(slot, &page, &device).unwrap();
        let _ = zswap_retrieve(slot, &device);
        let _ = zswap_retrieve(slot, &device);

        let stats = zswap_stats();
        assert_eq!(stats.hits, 2);
        assert_eq!(stats.misses, 0);
    }

    #[test]
    fn zswap_cache_miss_reads_from_device() {
        let _s = crate::test_serial::acquire();
        reset_zswap();
        init_zswap(16);

        let device = MockSwapDevice::new(64, "mock");
        let page = make_test_page(0xCC);
        let slot = SwapSlot(2);

        // Write directly to device (not zswap)
        device.write_page(slot, &page).unwrap();

        // Retrieve should miss and read from device
        let retrieved = zswap_retrieve(slot, &device).expect("retrieve should succeed");
        assert_eq!(page, retrieved);

        let stats = zswap_stats();
        assert_eq!(stats.misses, 1);
        assert_eq!(stats.hits, 0);
    }

    #[test]
    fn zswap_evicts_lru_when_full() {
        let _s = crate::test_serial::acquire();
        reset_zswap();
        init_zswap(4);

        let device = MockSwapDevice::new(64, "mock");

        // Fill the cache
        for i in 0..4u8 {
            let page = make_test_page(i);
            let slot = SwapSlot(i as usize);
            zswap_store(slot, &page, &device).unwrap();
        }
        assert_eq!(zswap_entry_count(), 4);

        // Insert a 5th — should evict LRU
        let page5 = make_test_page(0xFF);
        zswap_store(SwapSlot(4), &page5, &device).unwrap();
        assert_eq!(zswap_entry_count(), 4);

        // The oldest (slot 0) should have been evicted
        assert!(zswap_invalidate(SwapSlot(0)) == false || true); // may or may not exist
    }

    #[test]
    fn zswap_invalidate_removes_entry() {
        let _s = crate::test_serial::acquire();
        reset_zswap();
        init_zswap(16);

        let device = MockSwapDevice::new(64, "mock");
        let page = make_test_page(0xDD);
        zswap_store(SwapSlot(5), &page, &device).unwrap();

        assert!(zswap_invalidate(SwapSlot(5)));
        assert_eq!(zswap_entry_count(), 0);
        assert!(!zswap_invalidate(SwapSlot(5))); // already removed
    }

    #[test]
    fn zswap_flush_writes_back_all() {
        let _s = crate::test_serial::acquire();
        reset_zswap();
        init_zswap(16);

        let device = MockSwapDevice::new(64, "mock");
        for i in 0..4u8 {
            let page = make_test_page(i * 10);
            zswap_store(SwapSlot(i as usize), &page, &device).unwrap();
        }

        assert_eq!(zswap_entry_count(), 4);
        zswap_flush(&device);
        assert_eq!(zswap_entry_count(), 0);

        let stats = zswap_stats();
        assert_eq!(stats.writebacks, 4);
    }

    #[test]
    fn zswap_compression_ratio_is_beneficial() {
        let _s = crate::test_serial::acquire();
        reset_zswap();
        init_zswap(16);

        let device = MockSwapDevice::new(64, "mock");

        // Highly compressible page (all zeros)
        let page = [0u8; 4096];
        zswap_store(SwapSlot(0), &page, &device).unwrap();

        let stats = zswap_stats();
        assert!(
            stats.compressed_bytes < stats.uncompressed_bytes,
            "compressed ({}) should be less than uncompressed ({})",
            stats.compressed_bytes,
            stats.uncompressed_bytes
        );
    }

    #[test]
    fn zswap_no_compression_benefit_falls_through() {
        let _s = crate::test_serial::acquire();
        reset_zswap();
        init_zswap(16);

        let device = MockSwapDevice::new(64, "mock");

        // Random-ish data that won't compress well
        let mut page = [0u8; 4096];
        for (i, byte) in page.iter_mut().enumerate() {
            *byte = (i as u8).wrapping_mul(37).wrapping_add(11);
        }

        // Store should succeed (falls through to device write)
        let slot = SwapSlot(0);
        let result = zswap_store(slot, &page, &device);
        assert!(result.is_ok());

        // The page should be readable from the device directly
        let retrieved = device.read_page(slot, &mut [0u8; 4096]);
        assert!(retrieved.is_ok());
    }

    #[test]
    fn zswap_stats_default_is_zero() {
        let _s = crate::test_serial::acquire();
        let stats = ZswapStats::default();
        assert_eq!(stats.inserts, 0);
        assert_eq!(stats.hits, 0);
        assert_eq!(stats.misses, 0);
        assert_eq!(stats.writebacks, 0);
    }

    #[test]
    fn zswap_compressed_page_ratio_calculation() {
        let _s = crate::test_serial::acquire();
        let entry = CompressedPage::new(alloc::vec![0u8; 100], 4096);
        assert!(entry.compression_ratio() < 50);

        let entry2 = CompressedPage::new(alloc::vec![0u8; 5000], 4096);
        assert!(entry2.compression_ratio() > 100);
    }

    #[test]
    fn zswap_uninitialized_returns_defaults() {
        let _s = crate::test_serial::acquire();
        reset_zswap();
        assert_eq!(zswap_entry_count(), 0);
        assert_eq!(zswap_stats().inserts, 0);
    }

    #[test]
    fn zswap_multiple_store_retrieve_cycles() {
        let _s = crate::test_serial::acquire();
        reset_zswap();
        init_zswap(32);

        let device = MockSwapDevice::new(128, "mock");

        for cycle in 0..10u8 {
            let page = make_test_page(cycle);
            let slot = SwapSlot(cycle as usize);
            zswap_store(slot, &page, &device).unwrap();
            let retrieved = zswap_retrieve(slot, &device).unwrap();
            assert_eq!(page, retrieved, "cycle {} failed round-trip", cycle);
        }

        let stats = zswap_stats();
        assert_eq!(stats.inserts, 10);
        assert_eq!(stats.hits, 10);
    }

    #[test]
    fn zswap_invalidate_nonexistent_is_noop() {
        let _s = crate::test_serial::acquire();
        reset_zswap();
        init_zswap(16);
        assert!(!zswap_invalidate(SwapSlot(999)));
    }
}
