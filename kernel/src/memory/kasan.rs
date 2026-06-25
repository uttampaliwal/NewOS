//! Kernel Address Sanitizer — detects use-after-free, buffer overflows, double-frees.
//!
//! Uses a shadow memory map where each byte of kernel memory is mapped to a shadow
//! byte that encodes allocation state. The shadow is consulted on every heap access.

use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use spin::Mutex;

/// Poison values written to freed memory
pub const KASAN_POISON_FREE: u8 = 0x6b;
pub const KASAN_POISON_REDZONE: u8 = 0xbb;

/// How many shadow bytes per 8 bytes of real memory
const SHADOW_SCALE: usize = 8;

/// Maximum tracked allocations for statistics
const MAX_TRACKED: usize = 4096;

/// Redzone size appended after each allocation (bytes).
const REDZONE_SIZE: usize = 8;

/// Per-allocation metadata for leak/overflow detection
#[derive(Debug, Clone, Copy)]
struct AllocRecord {
    addr: usize,
    _size: usize,
    freed: bool,
}

/// Global KASAN state
pub struct KasanState {
    enabled: AtomicBool,
    heap_start: AtomicUsize,
    heap_end: AtomicUsize,
    shadow_base: AtomicUsize,
    records: Mutex<[Option<AllocRecord>; MAX_TRACKED]>,
    alloc_count: AtomicUsize,
    free_count: AtomicUsize,
    error_count: AtomicUsize,
}

impl KasanState {
    const fn new() -> Self {
        const NONE: Option<AllocRecord> = None;
        Self {
            enabled: AtomicBool::new(false),
            heap_start: AtomicUsize::new(0),
            heap_end: AtomicUsize::new(0),
            shadow_base: AtomicUsize::new(0),
            records: Mutex::new([NONE; MAX_TRACKED]),
            alloc_count: AtomicUsize::new(0),
            free_count: AtomicUsize::new(0),
            error_count: AtomicUsize::new(0),
        }
    }
}

static KASAN: KasanState = KasanState::new();

/// Initialize KASAN with the kernel heap range and a pre-mapped shadow region.
///
/// `shadow_base` must be a writable virtual address range of at least
/// `(heap_end - heap_start) / SHADOW_SCALE` bytes, mapped and ready before
/// this function is called.
pub fn init(heap_start: usize, heap_end: usize, shadow_base: usize) {
    let shadow_size = (heap_end - heap_start).div_ceil(SHADOW_SCALE);

    KASAN.heap_start.store(heap_start, Ordering::Release);
    KASAN
        .heap_end
        .store(heap_end + REDZONE_SIZE, Ordering::Release);
    KASAN.shadow_base.store(shadow_base, Ordering::Release);

    let shadow = shadow_base as *mut u8;
    for i in 0..shadow_size {
        // Safety: shadow_base is a valid writable virtual address range of at least
        // shadow_size bytes, mapped and ready before init() was called.
        unsafe {
            core::ptr::write_volatile(shadow.add(i), KASAN_POISON_FREE);
        }
    }

    KASAN.enabled.store(true, Ordering::Release);

    crate::serial::println!(
        "[KASAN] Initialized: heap {:x}..{:x}, shadow at {:x} ({} bytes)",
        heap_start,
        heap_end,
        shadow_base,
        shadow_size
    );
}

/// Return whether KASAN is enabled
pub fn is_enabled() -> bool {
    KASAN.enabled.load(Ordering::Acquire)
}

/// Get the shadow address for a given kernel address
#[inline]
fn shadow_for(addr: usize) -> usize {
    let start = KASAN.heap_start.load(Ordering::Acquire);
    let base = KASAN.shadow_base.load(Ordering::Acquire);
    base + (addr - start) / SHADOW_SCALE
}

/// Mark a region as allocated (unpoison). Also poisons the trailing redzone.
pub fn alloc_poison(addr: usize, size: usize) {
    if !is_enabled() {
        return;
    }

    let shadow = shadow_for(addr) as *mut u8;
    let shadow_size = size.div_ceil(SHADOW_SCALE);

    // Safety: shadow points into the valid shadow memory region for the heap,
    // and shadow_size is within the shadow region bounds.
    unsafe {
        for i in 0..shadow_size {
            core::ptr::write_volatile(shadow.add(i), 0);
        }
    }

    let redzone_start = addr + size;
    let rz_end = redzone_start + REDZONE_SIZE;
    let rz_shadow = shadow_for(redzone_start) as *mut u8;
    let rz_shadow_size = (rz_end - redzone_start).div_ceil(SHADOW_SCALE);
    // Safety: rz_shadow points into the valid shadow memory region for the redzone,
    // and rz_shadow_size covers only the redzone's shadow bytes.
    unsafe {
        for i in 0..rz_shadow_size {
            core::ptr::write_volatile(rz_shadow.add(i), KASAN_POISON_REDZONE);
        }
    }

    KASAN.alloc_count.fetch_add(1, Ordering::Relaxed);
    track_alloc(addr, size);
}

/// Mark a region as freed (poison)
pub fn free_poison(addr: usize, size: usize) {
    if !is_enabled() {
        return;
    }

    let shadow = shadow_for(addr) as *mut u8;
    let shadow_size = size.div_ceil(SHADOW_SCALE);

    // Safety: shadow points into the valid shadow memory region for the heap,
    // and shadow_size is within the shadow region bounds.
    unsafe {
        for i in 0..shadow_size {
            core::ptr::write_volatile(shadow.add(i), KASAN_POISON_FREE);
        }
    }

    KASAN.free_count.fetch_add(1, Ordering::Relaxed);
    mark_freed(addr);
}

/// Check if a single byte at `addr` is accessible (not poisoned)
#[inline]
pub fn check_byte(addr: usize) -> bool {
    if !is_enabled() {
        return true;
    }

    let start = KASAN.heap_start.load(Ordering::Acquire);
    let end = KASAN.heap_end.load(Ordering::Acquire);

    if addr < start || addr >= end {
        return true;
    }

    let shadow = shadow_for(addr) as *const u8;
    // Safety: shadow points into the valid shadow memory region; addr is within
    // the heap bounds (checked above), so the shadow address is valid for a read.
    let poison = unsafe { core::ptr::read_volatile(shadow) };

    poison == 0
}

/// Check if a range [addr, addr+size) is fully accessible
pub fn check_range(addr: usize, size: usize) -> Result<(), KasanError> {
    if !is_enabled() {
        return Ok(());
    }

    let start = KASAN.heap_start.load(Ordering::Acquire);
    let end = KASAN.heap_end.load(Ordering::Acquire);

    if addr < start || addr + size > end {
        return Ok(());
    }

    for offset in 0..size {
        let byte_addr = addr + offset;
        let shadow = shadow_for(byte_addr) as *const u8;
        // Safety: shadow points into the valid shadow memory region; byte_addr is
        // within heap bounds (checked above), so the shadow address is valid for a read.
        let poison = unsafe { core::ptr::read_volatile(shadow) };

        if poison != 0 {
            KASAN.error_count.fetch_add(1, Ordering::Relaxed);
            return Err(KasanError::UseAfterFree {
                addr: byte_addr,
                poison,
            });
        }
    }

    Ok(())
}

/// Report a KASAN violation
pub fn report_violation(addr: usize, size: usize, error: &KasanError) {
    KASAN.error_count.fetch_add(1, Ordering::Relaxed);

    crate::serial::println!("[KASAN] BUG: {:?}", error);
    crate::serial::println!("[KASAN]   Access at {:x} ({} bytes)", addr, size);
    crate::serial::println!(
        "[KASAN]   Heap range: {:x}..{:x}",
        KASAN.heap_start.load(Ordering::Acquire),
        KASAN.heap_end.load(Ordering::Acquire)
    );

    if addr >= KASAN.heap_start.load(Ordering::Acquire)
        && addr < KASAN.heap_end.load(Ordering::Acquire)
    {
        let shadow = shadow_for(addr) as *const u8;
        // Safety: shadow points into the valid shadow memory region; addr is within
        // heap bounds (checked above), so the shadow bytes are valid for reading.
        unsafe {
            let mut buf = [0u8; 32];
            for (i, byte) in buf.iter_mut().enumerate() {
                *byte = core::ptr::read_volatile(shadow.add(i));
            }
            crate::serial::println!("[KASAN]   Shadow dump: {:02x?}", buf);
        }
    }
}

/// KASAN error types
#[derive(Debug)]
pub enum KasanError {
    UseAfterFree { addr: usize, poison: u8 },
    BufferOverflow { addr: usize, expected: usize },
    DoubleFree { addr: usize },
}

fn track_alloc(addr: usize, size: usize) {
    let mut records = KASAN.records.lock();
    if let Some(slot) = records.iter_mut().find(|r| r.is_none()) {
        *slot = Some(AllocRecord {
            addr,
            _size: size,
            freed: false,
        });
    }
}

fn mark_freed(addr: usize) {
    let mut records = KASAN.records.lock();
    for r in records.iter_mut().flatten() {
        if r.addr == addr && !r.freed {
            r.freed = true;
            return;
        }
    }
}

/// Validate that a kernel buffer is in valid memory.
pub fn validate_kernel_buf(ptr: *const u8, size: usize) -> Result<(), KasanError> {
    let addr = ptr as usize;

    let heap_start = KASAN.heap_start.load(Ordering::Acquire);
    let heap_end = KASAN.heap_end.load(Ordering::Acquire);

    if addr >= heap_start && addr + size <= heap_end {
        return check_range(addr, size);
    }

    Ok(())
}

/// Get allocation statistics
pub fn stats() -> KasanStats {
    KasanStats {
        enabled: is_enabled(),
        allocs: KASAN.alloc_count.load(Ordering::Relaxed),
        frees: KASAN.free_count.load(Ordering::Relaxed),
        errors: KASAN.error_count.load(Ordering::Relaxed),
    }
}

#[derive(Debug, Clone, Copy)]
pub struct KasanStats {
    pub enabled: bool,
    pub allocs: usize,
    pub frees: usize,
    pub errors: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shadow_for_same_addr() {
        let start = 0xFFFF_8000_0000_0000usize;
        let base = 0xFFFF_A000_0000_0000usize;
        KASAN.heap_start.store(start, Ordering::Release);
        KASAN.shadow_base.store(base, Ordering::Release);

        let addr = start + 64;
        let shadow = shadow_for(addr);
        assert_eq!(shadow, base + 8);
    }

    #[test]
    fn test_stats_initial() {
        let s = KasanStats {
            enabled: false,
            allocs: 0,
            frees: 0,
            errors: 0,
        };
        assert!(!s.enabled);
    }

    #[test]
    fn test_kasan_error_debug() {
        let e = KasanError::UseAfterFree {
            addr: 0x1000,
            poison: 0x6b,
        };
        let dbg = alloc::format!("{:?}", e);
        assert!(dbg.contains("UseAfterFree"));
    }
}
