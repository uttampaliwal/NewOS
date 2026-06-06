use alloc::collections::{BTreeMap, VecDeque};
use alloc::vec::Vec;

use lazy_static::lazy_static;
use spin::Mutex;

use crate::memory::PhysFrame;
use crate::memory::vma::InodeId;

lazy_static! {
    /// Global page cache instance.
    ///
    /// All file-backed page faults check this cache before allocating a
    /// new physical frame.  Cache hits share the existing frame; misses
    /// allocate, read from the backing store, and populate the cache.
    ///
    /// The low-watermark (max cached pages before LRU eviction fires)
    /// is set to 4096, roughly 16 MiB of cached file data.
    pub static ref PAGE_CACHE: Mutex<PageCache> = Mutex::new(PageCache::new(4096));
}

#[derive(Debug, Clone)]
pub struct CachedPage {
    pub frame: PhysFrame,
    pub dirty: bool,
    pub ref_count: usize,
    pub last_access: u64,
    pub dirty_since: Option<u64>,
}

pub struct PageCache {
    pages: BTreeMap<(InodeId, u64), CachedPage>,
    lru: VecDeque<(InodeId, u64)>,
    total_pages: usize,
    dirty_pages: usize,
    low_watermark: usize,
}

impl PageCache {
    pub fn new(low_watermark: usize) -> Self {
        Self {
            pages: BTreeMap::new(),
            lru: VecDeque::new(),
            total_pages: 0,
            dirty_pages: 0,
            low_watermark,
        }
    }

    pub fn with_low_watermark(mut self, wm: usize) -> Self {
        self.low_watermark = wm;
        self
    }

    pub fn lookup(&mut self, inode: InodeId, page_idx: u64, now: u64) -> Option<PhysFrame> {
        let key = (inode, page_idx);
        if let Some(cached) = self.pages.get_mut(&key) {
            cached.last_access = now;
            let frame = cached.frame;
            let _ = cached;
            self.touch_lru(key);
            Some(frame)
        } else {
            None
        }
    }

    pub fn insert(&mut self, inode: InodeId, page_idx: u64, frame: PhysFrame, now: u64) {
        let key = (inode, page_idx);
        let entry = CachedPage {
            frame,
            dirty: false,
            ref_count: 0,
            last_access: now,
            dirty_since: None,
        };
        if self.pages.insert(key, entry).is_none() {
            self.total_pages += 1;
        }
        self.lru.push_back(key);

        if self.total_pages > self.low_watermark {
            self.evict_lru(1);
        }
    }

    pub fn mark_dirty(&mut self, inode: InodeId, page_idx: u64, now: u64) {
        let key = (inode, page_idx);
        if let Some(cached) = self.pages.get_mut(&key)
            && !cached.dirty
        {
            cached.dirty = true;
            cached.dirty_since = Some(now);
            self.dirty_pages += 1;
        }
    }

    pub fn add_ref(&mut self, inode: InodeId, page_idx: u64) {
        if let Some(cached) = self.pages.get_mut(&(inode, page_idx)) {
            cached.ref_count += 1;
        }
    }

    pub fn release(&mut self, inode: InodeId, page_idx: u64) {
        if let Some(cached) = self.pages.get_mut(&(inode, page_idx)) {
            cached.ref_count = cached.ref_count.saturating_sub(1);
        }
    }

    pub fn evict_lru(&mut self, count: usize) -> Vec<((InodeId, u64), u64)> {
        let mut evicted = Vec::new();
        let mut checked = 0usize;
        let max_checks = self.lru.len();

        while evicted.len() < count && checked < max_checks {
            if let Some(key) = self.lru.pop_front() {
                checked += 1;
                if let Some(cached) = self.pages.get(&key)
                    && cached.ref_count == 0
                    && !cached.dirty
                {
                    let frame_addr = cached.frame.start_address;
                    self.pages.remove(&key);
                    self.total_pages = self.total_pages.saturating_sub(1);
                    evicted.push((key, frame_addr));
                    continue;
                }
                self.lru.push_back(key);
            } else {
                break;
            }
        }

        evicted
    }

    pub fn writeback_dirty_pages(
        &mut self,
        now: u64,
        max_age_ticks: u64,
    ) -> Vec<(InodeId, u64, u64)> {
        let mut written = Vec::new();
        let expired_keys: Vec<(InodeId, u64)> = self
            .pages
            .iter()
            .filter(|(_, cached)| {
                cached.dirty
                    && cached
                        .dirty_since
                        .is_some_and(|ts| now.saturating_sub(ts) >= max_age_ticks)
            })
            .map(|(key, _)| *key)
            .collect();

        for key in expired_keys {
            if let Some(cached) = self.pages.get_mut(&key)
                && cached.dirty
            {
                cached.dirty = false;
                cached.dirty_since = None;
                self.dirty_pages = self.dirty_pages.saturating_sub(1);
                written.push((key.0, key.1, cached.frame.start_address));
            }
        }

        written
    }

    pub fn total_pages(&self) -> usize {
        self.total_pages
    }

    pub fn dirty_pages(&self) -> usize {
        self.dirty_pages
    }

    pub fn contains(&self, inode: InodeId, page_idx: u64) -> bool {
        self.pages.contains_key(&(inode, page_idx))
    }

    pub fn ref_count(&self, inode: InodeId, page_idx: u64) -> usize {
        self.pages
            .get(&(inode, page_idx))
            .map(|c| c.ref_count)
            .unwrap_or(0)
    }

    pub fn low_watermark(&self) -> usize {
        self.low_watermark
    }

    fn touch_lru(&mut self, key: (InodeId, u64)) {
        if let Some(pos) = self.lru.iter().position(|k| *k == key) {
            self.lru.remove(pos);
        }
        self.lru.push_back(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::collection as pc;
    use proptest::prelude::*;

    fn arb_inode() -> impl Strategy<Value = InodeId> {
        (0..100u64).prop_map(InodeId)
    }

    fn arb_page_idx() -> impl Strategy<Value = u64> {
        0..10_000u64
    }

    fn arb_frame_addr() -> impl Strategy<Value = u64> {
        (0x1000u64..0x1000_0000u64).prop_map(|a| a.next_multiple_of(4096))
    }

    fn fake_frame(addr: u64) -> PhysFrame {
        PhysFrame {
            start_address: addr,
        }
    }

    proptest! {
        /// Property 6 — Page Cache Read Idempotence
        ///
        /// For any file and page-index pair, reading the same page twice
        /// returns the same physical frame both times, with the cache
        /// containing exactly one entry after the first read.
        #[test]
        fn page_cache_read_idempotence(
            inode in arb_inode(),
            page_idx in arb_page_idx(),
            frame_addr in arb_frame_addr(),
            tick in 0..1_000_000u64,
        ) {
            let mut cache = PageCache::new(usize::MAX);
            let frame = fake_frame(frame_addr);

            // First read — insert
            cache.insert(inode, page_idx, frame, tick);
            prop_assert!(cache.contains(inode, page_idx));
            prop_assert_eq!(cache.total_pages(), 1);

            // Second read — lookup; must return the same frame
            let lookup_result = cache.lookup(inode, page_idx, tick + 1);
            prop_assert!(
                lookup_result.is_some(),
                "second lookup must return the cached frame"
            );
            if let Some(found) = lookup_result {
                prop_assert_eq!(
                    found.start_address, frame_addr,
                    "second lookup must return the same physical frame"
                );
            }

            // Still exactly one page in cache
            prop_assert_eq!(cache.total_pages(), 1);
        }
    }

    proptest! {
        /// Property 7 — Page Cache LRU Eviction Order
        ///
        /// For any sequence of page accesses, the evicted page is always
        /// the one with the earliest last-access timestamp among all
        /// evictable (unreferenced, clean) pages.
        #[test]
        fn page_cache_lru_eviction_order(
            inode in arb_inode(),
            mut page_indices in pc::vec(arb_page_idx(), 3..10),
            frame_base in arb_frame_addr(),
        ) {
            // Deduplicate — keep first occurrence so insertion order
            // maps deterministically to increasing timestamps.
            let mut unique = Vec::new();
            for &idx in &page_indices {
                if !unique.contains(&idx) {
                    unique.push(idx);
                }
            }
            let count = unique.len();
            if count < 2 {
                return Ok(()); // skip degenerate
            }
            page_indices = unique;

            let mut cache = PageCache::new(usize::MAX);

            // Insert in vec order at increasing timestamps
            for (i, &idx) in page_indices.iter().enumerate() {
                let frame = fake_frame(frame_base + i as u64 * 4096);
                cache.insert(inode, idx, frame, i as u64);
            }

            prop_assert_eq!(cache.total_pages(), count);

            // The LRU page = first inserted (earliest timestamp) =
            // page_indices[0].
            let earliest_idx = page_indices[0];

            let evicted = cache.evict_lru(1);
            prop_assert_eq!(
                evicted.len(), 1,
                "should evict exactly one page"
            );

            if let Some(((ev_inode, ev_idx), _)) = evicted.first() {
                prop_assert_eq!(
                    *ev_idx, earliest_idx,
                    "LRU eviction must select earliest-inserted page (front of LRU)"
                );
                prop_assert_eq!(*ev_inode, inode);
                prop_assert!(
                    !cache.contains(inode, earliest_idx),
                    "evicted page must not remain in cache"
                );
            }

            prop_assert_eq!(cache.total_pages(), count - 1);
        }
    }

    proptest! {
        /// Property 8 — Page Cache Sharing
        ///
        /// For any file mapped by N processes, the number of distinct
        /// physical frames backing that file's pages equals the number
        /// of unique pages, not N times that number.  Concretely:
        /// inserting once and adding N refs yields ref_count == N.
        #[test]
        fn page_cache_sharing(
            inode in arb_inode(),
            page_idx in arb_page_idx(),
            frame_addr in arb_frame_addr(),
            n_procs in 2..64u64,
            tick in 0..1_000_000u64,
        ) {
            let mut cache = PageCache::new(usize::MAX);
            let frame = fake_frame(frame_addr);

            // One process maps the page → insert (ref_count = 0)
            // then add_ref to account for the mapping
            cache.insert(inode, page_idx, frame, tick);
            cache.add_ref(inode, page_idx);
            prop_assert_eq!(cache.ref_count(inode, page_idx), 1);
            prop_assert_eq!(cache.total_pages(), 1);

            // N-1 more processes map the same page → add_ref
            for _ in 1..n_procs {
                cache.add_ref(inode, page_idx);
            }

            // Distinct physical frame count = 1 (only one frame was ever allocated)
            prop_assert_eq!(
                cache.ref_count(inode, page_idx),
                n_procs as usize,
                "ref_count must equal number of sharing processes"
            );

            // Only one cached page exists for this (inode, page_idx)
            prop_assert_eq!(cache.total_pages(), 1);

            // All processes see the same physical frame
            let shared_frame = cache.lookup(inode, page_idx, tick);
            prop_assert!(shared_frame.is_some());
            if let Some(f) = shared_frame {
                prop_assert_eq!(f.start_address, frame_addr);
            }

            // Releasing all N refs still leaves the page alive (LRU evictable)
            for _ in 0..n_procs {
                cache.release(inode, page_idx);
            }
            prop_assert_eq!(cache.ref_count(inode, page_idx), 0);
            prop_assert!(cache.contains(inode, page_idx));
        }
    }

    #[test]
    fn empty_cache_lookup_returns_none() {
        let mut cache = PageCache::new(100);
        assert!(cache.lookup(InodeId(1), 0, 0).is_none());
    }

    #[test]
    fn insert_and_lookup() {
        let mut cache = PageCache::new(usize::MAX);
        let frame = fake_frame(0x2000);
        cache.insert(InodeId(42), 7, frame, 100);
        assert!(cache.contains(InodeId(42), 7));
        assert_eq!(cache.total_pages(), 1);
        let found = cache.lookup(InodeId(42), 7, 200);
        assert!(found.is_some());
        assert_eq!(found.unwrap().start_address, 0x2000);
    }

    #[test]
    fn eviction_removes_unreferenced_clean_pages() {
        let mut cache = PageCache::new(usize::MAX);
        cache.insert(InodeId(1), 0, fake_frame(0x1000), 0);
        cache.insert(InodeId(1), 1, fake_frame(0x2000), 1);
        assert_eq!(cache.total_pages(), 2);

        let evicted = cache.evict_lru(1);
        assert_eq!(evicted.len(), 1);
        assert_eq!(evicted[0].0, (InodeId(1), 0)); // oldest access
        assert!(!cache.contains(InodeId(1), 0));
        assert!(cache.contains(InodeId(1), 1));
    }

    #[test]
    fn referenced_pages_not_evicted() {
        let mut cache = PageCache::new(usize::MAX);
        cache.insert(InodeId(1), 0, fake_frame(0x1000), 0);
        cache.insert(InodeId(1), 1, fake_frame(0x2000), 1);
        cache.add_ref(InodeId(1), 0); // page 0 is now referenced

        let evicted = cache.evict_lru(1);
        // Page 0 has ref_count=1 → not evictable, page 1 should be evicted
        assert_eq!(evicted.len(), 1);
        assert_eq!(evicted[0].0, (InodeId(1), 1));
        assert!(cache.contains(InodeId(1), 0));
    }

    #[test]
    fn dirty_pages_not_evicted() {
        let mut cache = PageCache::new(usize::MAX);
        cache.insert(InodeId(1), 0, fake_frame(0x1000), 0);
        cache.insert(InodeId(1), 1, fake_frame(0x2000), 1);
        cache.mark_dirty(InodeId(1), 0, 0);

        let evicted = cache.evict_lru(1);
        // Page 0 is dirty → not evictable, page 1 should be evicted
        assert_eq!(evicted.len(), 1);
        assert_eq!(evicted[0].0, (InodeId(1), 1));
    }

    #[test]
    fn mark_dirty_and_writeback() {
        let mut cache = PageCache::new(usize::MAX);
        cache.insert(InodeId(1), 0, fake_frame(0x1000), 0);
        cache.mark_dirty(InodeId(1), 0, 0);
        assert_eq!(cache.dirty_pages(), 1);

        // Writeback with max_age=30 ticks, now=29 → not expired (29-0=29 < 30)
        let written = cache.writeback_dirty_pages(29, 30);
        assert!(written.is_empty());
        assert_eq!(cache.dirty_pages(), 1);

        // Writeback with now=30, max_age=30 → expired (30-0=30 >= 30)
        let written = cache.writeback_dirty_pages(30, 30);
        assert_eq!(written.len(), 1);
        assert_eq!(written[0].1, 0); // page_idx=0
        assert_eq!(cache.dirty_pages(), 0);
    }

    #[test]
    fn low_watermark_triggers_eviction_on_insert() {
        let mut cache = PageCache::new(2); // watermark = 2
        cache.insert(InodeId(1), 0, fake_frame(0x1000), 0);
        cache.insert(InodeId(1), 1, fake_frame(0x2000), 1);
        assert_eq!(cache.total_pages(), 2);

        // Inserting a third page should trigger eviction of the LRU
        cache.insert(InodeId(1), 2, fake_frame(0x3000), 2);
        // LRU was page 0, so it should be evicted
        assert_eq!(cache.total_pages(), 2);
        assert!(!cache.contains(InodeId(1), 0));
        assert!(cache.contains(InodeId(1), 1));
        assert!(cache.contains(InodeId(1), 2));
    }

    #[test]
    fn release_decrements_ref_count() {
        let mut cache = PageCache::new(usize::MAX);
        cache.insert(InodeId(1), 0, fake_frame(0x1000), 0);
        cache.add_ref(InodeId(1), 0);
        cache.add_ref(InodeId(1), 0);
        assert_eq!(cache.ref_count(InodeId(1), 0), 2);

        cache.release(InodeId(1), 0);
        assert_eq!(cache.ref_count(InodeId(1), 0), 1);

        cache.release(InodeId(1), 0);
        assert_eq!(cache.ref_count(InodeId(1), 0), 0);
    }

    #[test]
    fn release_does_not_underflow() {
        let mut cache = PageCache::new(usize::MAX);
        cache.insert(InodeId(1), 0, fake_frame(0x1000), 0);
        cache.release(InodeId(1), 0);
        assert_eq!(cache.ref_count(InodeId(1), 0), 0);
        cache.release(InodeId(1), 0);
        assert_eq!(cache.ref_count(InodeId(1), 0), 0);
    }

    #[test]
    fn writeback_only_expired_dirty_pages() {
        let mut cache = PageCache::new(usize::MAX);
        cache.insert(InodeId(1), 0, fake_frame(0x1000), 0);
        cache.insert(InodeId(1), 1, fake_frame(0x2000), 1);
        cache.mark_dirty(InodeId(1), 0, 10); // dirty at tick 10
        cache.mark_dirty(InodeId(1), 1, 30); // dirty at tick 30

        // now=59, max_age=50 → page 0: 59-10=49 < 50 → not expired
        //                       page 1: 59-30=29 < 50 → not expired
        let written = cache.writeback_dirty_pages(59, 50);
        assert_eq!(written.len(), 0);
        assert_eq!(cache.dirty_pages(), 2);

        // now=60, max_age=50 → page 0: 60-10=50 >= 50 → expired
        //                       page 1: 60-30=30 < 50 → not expired
        let written = cache.writeback_dirty_pages(60, 50);
        assert_eq!(written.len(), 1);
        assert_eq!(written[0].1, 0);
        assert_eq!(cache.dirty_pages(), 1);

        // now=80, max_age=50 → page 1: 80-30=50 >= 50 → expired
        let written = cache.writeback_dirty_pages(80, 50);
        assert_eq!(written.len(), 1);
        assert_eq!(written[0].1, 1);
        assert_eq!(cache.dirty_pages(), 0);
    }
}
