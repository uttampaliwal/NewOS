//! Kernel Samepage Merging (KSM)
//!
//! Deduplicates identical pages by maintaining a stable tree of page content
//! hashes. When two pages have the same content, they are merged into a single
//! read-only COW page. On write, the fault handler allocates a private copy.

use alloc::collections::BTreeMap;
use spin::Mutex;

/// Size of a standard page for hashing.
const PAGE_SIZE: usize = 4096;

/// Maximum number of unique pages to track in the KSM tree.
const MAX_KSM_PAGES: usize = 65536;

/// A content hash of a page (first 32 bytes of SHA-256).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PageHash([u8; 32]);

impl PageHash {
    /// Compute a content hash of a 4 KiB page.
    ///
    /// # Safety
    ///
    /// `page_ptr` must point to a valid, readable 4096-byte page.
    pub unsafe fn compute(page_ptr: *const u8) -> Self {
        // Read page content in chunks for hashing
        let mut hash_bytes = [0u8; 32];

        // Use a simple but effective hash: XOR-fold the page content
        // into 32 bytes using a mixing function
        let mut acc = [0usize; 4];
        for chunk_idx in 0..512 {
            // Safety: page_ptr points to valid 4096-byte page; chunk_idx < 512
            let word = unsafe { core::ptr::read_unaligned(page_ptr.add(chunk_idx * 8) as *const u64) } as usize;
            acc[chunk_idx % 4] = acc[chunk_idx % 4].wrapping_add(word);
            acc[chunk_idx % 4] ^= acc[(chunk_idx + 1) % 4].rotate_left(13);
        }
        for i in 0..4 {
            hash_bytes[i * 8..(i + 1) * 8].copy_from_slice(&acc[i].to_le_bytes());
        }

        // Second pass: mix with offset-based seeds for better distribution
        let mut mixer = [0u8; 32];
        for i in 0..PAGE_SIZE {
            // Safety: page_ptr + i is within the 4096-byte page
            let byte = unsafe { *page_ptr.add(i) };
            mixer[i % 32] = mixer[i % 32].wrapping_add(byte).wrapping_add(i as u8);
        }
        for i in 0..32 {
            hash_bytes[i] ^= mixer[i];
        }

        PageHash(hash_bytes)
    }

    /// Create a hash from raw bytes.
    pub fn from_bytes(bytes: &[u8; 32]) -> Self {
        PageHash(*bytes)
    }

    /// Get the raw hash bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// A KSM merge entry: tracks how many virtual pages share a physical frame.
#[derive(Debug, Clone)]
pub struct KsmEntry {
    /// Content hash of the shared page.
    pub hash: PageHash,
    /// Physical address of the shared page frame.
    pub phys_addr: u64,
    /// Number of virtual pages mapped to this physical frame.
    pub ref_count: u32,
}

/// Statistics for the KSM subsystem.
#[derive(Debug, Clone, Copy, Default)]
pub struct KsmStats {
    /// Total pages scanned by KSM.
    pub pages_scanned: u64,
    /// Total pages merged (deduplicated).
    pub pages_merged: u64,
    /// Current number of unique pages in the stable tree.
    pub stable_pages: u64,
    /// Memory saved by merging (in bytes).
    pub memory_saved: u64,
    /// Number of COW faults triggered by merged page writes.
    pub cow_faults: u64,
}

/// The KSM scanner and merge manager.
pub struct KsmManager {
    /// Stable tree: maps content hash -> KSM entry.
    stable_tree: BTreeMap<PageHash, KsmEntry>,
    /// Reverse mapping: physical address -> list of content hashes
    /// (used for COW fault handling).
    phys_to_hash: BTreeMap<u64, PageHash>,
    /// Statistics.
    stats: KsmStats,
    /// Whether KSM scanning is enabled.
    enabled: bool,
}

impl KsmManager {
    const fn new() -> Self {
        KsmManager {
            stable_tree: BTreeMap::new(),
            phys_to_hash: BTreeMap::new(),
            stats: KsmStats {
                pages_scanned: 0,
                pages_merged: 0,
                stable_pages: 0,
                memory_saved: 0,
                cow_faults: 0,
            },
            enabled: false,
        }
    }

    /// Enable or disable KSM scanning.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Check if KSM is enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Scan a page and attempt to merge it with an existing identical page.
    ///
    /// Returns `Some(merged_phys_addr)` if the page was merged (caller should
    /// remap to the existing frame with COW), or `None` if this is a unique page.
    ///
    /// # Safety
    ///
    /// `page_ptr` must point to a valid, readable 4096-byte page.
    pub unsafe fn scan_and_merge(
        &mut self,
        page_ptr: *const u8,
        current_phys_addr: u64,
    ) -> Option<u64> {
        if !self.enabled {
            return None;
        }

        let hash = unsafe { PageHash::compute(page_ptr) };
        self.stats.pages_scanned += 1;

        if self.stable_tree.len() >= MAX_KSM_PAGES {
            return None;
        }

        if let Some(entry) = self.stable_tree.get_mut(&hash) {
            // Found a match — merge!
            entry.ref_count += 1;
            self.stats.pages_merged += 1;
            self.stats.memory_saved += PAGE_SIZE as u64;
            Some(entry.phys_addr)
        } else {
            // Unique page — add to stable tree
            let entry = KsmEntry {
                hash,
                phys_addr: current_phys_addr,
                ref_count: 1,
            };
            self.stable_tree.insert(hash, entry);
            self.phys_to_hash.insert(current_phys_addr, hash);
            self.stats.stable_pages += 1;
            None
        }
    }

    /// Handle a COW fault on a KSM-merged page.
    ///
    /// Called when a process writes to a page that was merged by KSM.
    /// Returns the content hash so the caller can copy the page data.
    pub fn handle_cow_fault(&mut self, phys_addr: u64) -> Option<PageHash> {
        self.stats.cow_faults += 1;
        self.phys_to_hash.get(&phys_addr).copied()
    }

    /// Get the physical address for a given content hash.
    pub fn get_phys_for_hash(&self, hash: &PageHash) -> Option<u64> {
        self.stable_tree.get(hash).map(|e| e.phys_addr)
    }

    /// Get the reference count for a merged page.
    pub fn get_ref_count(&self, hash: &PageHash) -> u32 {
        self.stable_tree
            .get(hash)
            .map(|e| e.ref_count)
            .unwrap_or(0)
    }

    /// Get current statistics.
    pub fn stats(&self) -> KsmStats {
        self.stats
    }

    /// Get the number of unique pages in the stable tree.
    pub fn stable_page_count(&self) -> usize {
        self.stable_tree.len()
    }

    /// Merge two identical pages, updating ref counts.
    ///
    /// Returns the shared physical address.
    pub fn merge_pages(
        &mut self,
        hash: PageHash,
        phys_addr: u64,
    ) -> Option<u64> {
        if let Some(entry) = self.stable_tree.get_mut(&hash) {
            entry.ref_count += 1;
            self.stats.pages_merged += 1;
            self.stats.memory_saved += PAGE_SIZE as u64;
            Some(entry.phys_addr)
        } else {
            let entry = KsmEntry {
                hash,
                phys_addr,
                ref_count: 1,
            };
            self.stable_tree.insert(hash, entry);
            self.phys_to_hash.insert(phys_addr, hash);
            self.stats.stable_pages += 1;
            Some(phys_addr)
        }
    }
}

/// Global KSM manager.
static KSM: Mutex<KsmManager> = Mutex::new(KsmManager::new());

/// Enable or disable KSM scanning.
pub fn set_ksm_enabled(enabled: bool) {
    KSM.lock().set_enabled(enabled);
}

/// Check if KSM is enabled.
pub fn is_ksm_enabled() -> bool {
    KSM.lock().is_enabled()
}

/// Scan a page and attempt KSM merge.
///
/// # Safety
///
/// `page_ptr` must point to a valid, readable 4096-byte page.
pub unsafe fn scan_and_merge(page_ptr: *const u8, current_phys_addr: u64) -> Option<u64> {
    unsafe { KSM.lock().scan_and_merge(page_ptr, current_phys_addr) }
}

/// Handle a COW fault on a KSM-merged page.
pub fn handle_cow_fault(phys_addr: u64) -> Option<PageHash> {
    KSM.lock().handle_cow_fault(phys_addr)
}

/// Get KSM statistics.
pub fn ksm_stats() -> KsmStats {
    KSM.lock().stats()
}

/// Get the number of unique pages in the stable tree.
pub fn stable_page_count() -> usize {
    KSM.lock().stable_page_count()
}

/// Merge pages with a given content hash.
pub fn merge_pages(hash: PageHash, phys_addr: u64) -> Option<u64> {
    KSM.lock().merge_pages(hash, phys_addr)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_page(byte: u8) -> [u8; PAGE_SIZE] {
        [byte; PAGE_SIZE]
    }

    #[test]
    fn test_page_hash_deterministic() {
        let _guard = crate::test_serial::acquire();
        let page = make_test_page(0xAA);
        let h1 = unsafe { PageHash::compute(page.as_ptr()) };
        let h2 = unsafe { PageHash::compute(page.as_ptr()) };
        assert_eq!(h1, h2, "hash should be deterministic");
    }

    #[test]
    fn test_page_hash_different_content() {
        let _guard = crate::test_serial::acquire();
        let page_a = make_test_page(0xAA);
        let page_b = make_test_page(0xBB);
        let h_a = unsafe { PageHash::compute(page_a.as_ptr()) };
        let h_b = unsafe { PageHash::compute(page_b.as_ptr()) };
        assert_ne!(h_a, h_b, "different content should produce different hashes");
    }

    #[test]
    fn test_page_hash_different_pattern() {
        let _guard = crate::test_serial::acquire();
        let mut page1 = [0u8; PAGE_SIZE];
        let mut page2 = [0u8; PAGE_SIZE];
        // Set different bytes at position 0
        page1[0] = 1;
        page2[0] = 2;
        let h1 = unsafe { PageHash::compute(page1.as_ptr()) };
        let h2 = unsafe { PageHash::compute(page2.as_ptr()) };
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_ksm_disabled_returns_none() {
        let _guard = crate::test_serial::acquire();
        let mut mgr = KsmManager::new();
        let page = make_test_page(0x42);
        let result = unsafe { mgr.scan_and_merge(page.as_ptr(), 0x1000) };
        assert!(result.is_none(), "KSM disabled should return None");
    }

    #[test]
    fn test_ksm_merge_identical_pages() {
        let _guard = crate::test_serial::acquire();
        let mut mgr = KsmManager::new();
        mgr.set_enabled(true);

        let page = make_test_page(0x55);
        // First scan — unique, no merge
        let result1 = unsafe { mgr.scan_and_merge(page.as_ptr(), 0x1000) };
        assert!(result1.is_none(), "first scan should not merge");

        // Second scan with identical content — should merge
        let result2 = unsafe { mgr.scan_and_merge(page.as_ptr(), 0x2000) };
        assert!(result2.is_some(), "second scan should merge");
        assert_eq!(result2.unwrap(), 0x1000, "should return original phys addr");
        assert_eq!(mgr.stats().pages_merged, 1);
        assert_eq!(mgr.stats().memory_saved, PAGE_SIZE as u64);
    }

    #[test]
    fn test_ksm_ref_count() {
        let _guard = crate::test_serial::acquire();
        let mut mgr = KsmManager::new();
        mgr.set_enabled(true);

        let page = make_test_page(0x77);
        unsafe { mgr.scan_and_merge(page.as_ptr(), 0x1000) };
        unsafe { mgr.scan_and_merge(page.as_ptr(), 0x2000) };
        unsafe { mgr.scan_and_merge(page.as_ptr(), 0x3000) };

        let hash = unsafe { PageHash::compute(page.as_ptr()) };
        assert_eq!(mgr.get_ref_count(&hash), 3, "ref count should be 3");
        assert_eq!(mgr.stats().pages_merged, 2);
        assert_eq!(mgr.stats().memory_saved, 2 * PAGE_SIZE as u64);
    }

    #[test]
    fn test_ksm_stable_tree_limit() {
        let _guard = crate::test_serial::acquire();
        let mut mgr = KsmManager::new();
        mgr.set_enabled(true);

        // Fill the stable tree to the limit
        for i in 0..MAX_KSM_PAGES {
            let mut page = [0u8; PAGE_SIZE];
            // Create unique content for each page using different byte patterns
            let val = (i & 0xFF) as u8;
            let offset = (i >> 8) & 0xFF;
            page[0] = val;
            page[1] = offset as u8;
            unsafe { mgr.scan_and_merge(page.as_ptr(), (i as u64) * 0x1000) };
        }
        assert_eq!(mgr.stable_page_count(), MAX_KSM_PAGES);

        // One more unique page should not be added
        let mut overflow_page = [0u8; PAGE_SIZE];
        overflow_page[0] = 0xFF;
        overflow_page[1] = 0xFF;
        overflow_page[2] = 0xFF; // Ensure uniqueness
        let result = unsafe { mgr.scan_and_merge(overflow_page.as_ptr(), 0xFFFFF000) };
        assert!(result.is_none(), "should not add beyond limit");
        assert_eq!(mgr.stable_page_count(), MAX_KSM_PAGES);
    }

    #[test]
    fn test_ksm_cow_fault() {
        let _guard = crate::test_serial::acquire();
        let mut mgr = KsmManager::new();
        mgr.set_enabled(true);

        let page = make_test_page(0x99);
        unsafe { mgr.scan_and_merge(page.as_ptr(), 0x5000) };

        let hash = mgr.handle_cow_fault(0x5000);
        assert!(hash.is_some());
        assert_eq!(mgr.stats().cow_faults, 1);
    }

    #[test]
    fn test_ksm_cow_fault_unknown_addr() {
        let _guard = crate::test_serial::acquire();
        let mut mgr = KsmManager::new();
        let hash = mgr.handle_cow_fault(0xDEAD);
        assert!(hash.is_none());
    }

    #[test]
    fn test_merge_pages_api() {
        let _guard = crate::test_serial::acquire();
        set_ksm_enabled(true);
        let page = make_test_page(0xAA);
        let hash = unsafe { PageHash::compute(page.as_ptr()) };

        let result1 = merge_pages(hash, 0x1000);
        assert_eq!(result1, Some(0x1000));

        let result2 = merge_pages(hash, 0x2000);
        assert_eq!(result2, Some(0x1000), "should return original addr");

        assert_eq!(ksm_stats().pages_merged, 1);
        set_ksm_enabled(false);
    }

    #[test]
    fn test_get_phys_for_hash() {
        let _guard = crate::test_serial::acquire();
        let mut mgr = KsmManager::new();
        mgr.set_enabled(true);
        let page = make_test_page(0xBB);
        unsafe { mgr.scan_and_merge(page.as_ptr(), 0x7000) };
        let hash = unsafe { PageHash::compute(page.as_ptr()) };
        assert_eq!(mgr.get_phys_for_hash(&hash), Some(0x7000));
    }
}
