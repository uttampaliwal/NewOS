use alloc::alloc::{Layout, alloc, dealloc};
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::mem;
use spin::Mutex;

pub struct Slab {
    base: *mut u8,
    obj_size: usize,
    alloc_size: usize,
    total: usize,
    free_bitmap: u64,
    cursor: usize,
}

unsafe impl Send for Slab {}
unsafe impl Sync for Slab {}

impl Slab {
    pub fn new(base: *mut u8, obj_size: usize, total: usize, alloc_size: usize) -> Self {
        assert!(total <= 64, "slab max 64 objects");
        let mut slab = Slab {
            base,
            obj_size,
            alloc_size,
            total,
            free_bitmap: (1u64 << total) - 1,
            cursor: 0,
        };
        slab.build_free_list();
        slab
    }

    fn build_free_list(&mut self) {
        for i in 0..self.total {
            let ptr = unsafe { self.base.add(i * self.obj_size) };
            let next_free = if i + 1 < self.total {
                (i + 1) as u64
            } else {
                u64::MAX
            };
            unsafe {
                core::ptr::write_volatile(ptr as *mut u64, next_free);
            }
        }
    }

    pub fn alloc(&mut self) -> Option<*mut u8> {
        while self.cursor < self.total {
            if self.free_bitmap & (1u64 << self.cursor) != 0 {
                let idx = self.cursor;
                self.free_bitmap &= !(1u64 << idx);
                let ptr = unsafe { self.base.add(idx * self.obj_size) };
                self.cursor += 1;
                return Some(ptr);
            }
            self.cursor += 1;
        }
        None
    }

    pub fn dealloc(&mut self, ptr: *mut u8) -> bool {
        let offset = (ptr as usize) - (self.base as usize);
        if !offset.is_multiple_of(self.obj_size) || offset / self.obj_size >= self.total {
            return false;
        }
        let idx = offset / self.obj_size;
        self.free_bitmap |= 1u64 << idx;
        if idx < self.cursor {
            self.cursor = idx;
        }
        true
    }

    pub fn is_full(&self) -> bool {
        self.free_bitmap == 0
    }

    pub fn free_count(&self) -> usize {
        self.free_bitmap.count_ones() as usize
    }

    pub fn used_count(&self) -> usize {
        self.total - self.free_count()
    }
}

pub struct SlabCache {
    obj_size: usize,
    page_size: usize,
    slabs: Vec<Slab>,
    stats: SlabStats,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SlabStats {
    pub total_allocated: usize,
    pub total_freed: usize,
    pub active_objects: usize,
    pub slab_count: usize,
    pub alloc_failures: usize,
}

impl SlabCache {
    pub fn new(obj_size: usize) -> Self {
        let aligned_size = (obj_size + mem::align_of::<u64>() - 1) & !(mem::align_of::<u64>() - 1);
        let page_size = 4096;
        SlabCache {
            obj_size: aligned_size.max(mem::size_of::<u64>()),
            page_size,
            slabs: Vec::new(),
            stats: SlabStats::default(),
        }
    }

    #[allow(clippy::collapsible_if)]
    pub fn alloc(&mut self) -> Option<*mut u8> {
        for slab in &mut self.slabs {
            if !slab.is_full() {
                if let Some(ptr) = slab.alloc() {
                    self.stats.total_allocated += 1;
                    self.stats.active_objects += 1;
                    return Some(ptr);
                }
            }
        }
        self.grow();
        if let Some(slab) = self.slabs.last_mut()
            && let Some(ptr) = slab.alloc()
        {
            self.stats.total_allocated += 1;
            self.stats.active_objects += 1;
            return Some(ptr);
        }
        self.stats.alloc_failures += 1;
        None
    }

    pub fn dealloc(&mut self, ptr: *mut u8) -> bool {
        for slab in &mut self.slabs {
            if slab.base as usize <= ptr as usize
                && (ptr as usize) < slab.base as usize + slab.alloc_size
                && slab.dealloc(ptr)
            {
                self.stats.total_freed += 1;
                self.stats.active_objects -= 1;
                return true;
            }
        }
        false
    }

    fn grow(&mut self) {
        let alloc_size = self.page_size.max(self.obj_size).next_multiple_of(4096);
        let layout = Layout::from_size_align(alloc_size, 4096).unwrap();
        unsafe {
            let ptr = alloc(layout);
            if ptr.is_null() {
                return;
            }
            core::ptr::write_bytes(ptr, 0, alloc_size);
            let objects_per_slab = alloc_size / self.obj_size;
            let slab = Slab::new(ptr, self.obj_size, objects_per_slab, alloc_size);
            self.slabs.push(slab);
            self.stats.slab_count += 1;
        }
    }

    pub fn stats(&self) -> SlabStats {
        self.stats
    }
}

impl Drop for SlabCache {
    fn drop(&mut self) {
        for slab in &self.slabs {
            let layout = Layout::from_size_align(slab.alloc_size, 4096).unwrap();
            unsafe {
                dealloc(slab.base, layout);
            }
        }
    }
}

static SLAB_CACHES: Mutex<BTreeMap<usize, SlabCache>> = Mutex::new(BTreeMap::new());

pub fn slab_alloc(obj_size: usize) -> Option<*mut u8> {
    let mut caches = SLAB_CACHES.lock();
    let cache = caches
        .entry(obj_size)
        .or_insert_with(|| SlabCache::new(obj_size));
    cache.alloc()
}

pub fn slab_dealloc(ptr: *mut u8, obj_size: usize) -> bool {
    let mut caches = SLAB_CACHES.lock();
    if let Some(cache) = caches.get_mut(&obj_size) {
        cache.dealloc(ptr)
    } else {
        false
    }
}

pub fn slab_stats(obj_size: usize) -> Option<SlabStats> {
    let caches = SLAB_CACHES.lock();
    caches.get(&obj_size).map(|c| c.stats())
}

pub fn slab_stats_all() -> Vec<(usize, SlabStats)> {
    let caches = SLAB_CACHES.lock();
    caches.iter().map(|(&size, c)| (size, c.stats())).collect()
}
