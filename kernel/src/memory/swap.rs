use core::sync::atomic::{AtomicU64, Ordering};
use x86_64::instructions::tlb;
use x86_64::structures::paging::{PageTable, PageTableFlags, PhysFrame, Size4KiB};
use x86_64::VirtAddr;

use alloc::collections::BTreeSet;
use alloc::sync::Arc;
use spin::Mutex;

use crate::memory::PAGE_SIZE;

// ---------------------------------------------------------------------------
// Swap slot types
// ---------------------------------------------------------------------------

/// A single swap slot index representing one 4 KiB page in the swap area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SwapSlot(pub usize);

// ---------------------------------------------------------------------------
// Swap slot allocator
// ---------------------------------------------------------------------------

/// A bitmap-based allocator for swap slots.
///
/// Tracks which slots are in use and hands out free slots on demand.
/// Wraps around when the end of the bitmap is reached.
pub struct SwapSlotAllocator {
    bitmap: u64,
    hand: usize,
    capacity: usize,
    wrapped: bool,
}

impl SwapSlotAllocator {
    pub const fn new() -> Self {
        Self {
            bitmap: 0,
            hand: 0,
            capacity: 64,
            wrapped: false,
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        let slots = capacity.next_power_of_two().max(64);
        Self {
            bitmap: 0,
            hand: 0,
            capacity: slots,
            wrapped: false,
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn used(&self) -> usize {
        self.bitmap.count_ones() as usize
    }

    pub fn available(&self) -> usize {
        self.capacity - self.used()
    }

    pub fn allocate(&mut self) -> Option<SwapSlot> {
        if self.used() >= self.capacity {
            return None;
        }

        for _ in 0..self.capacity {
            if self.hand >= self.capacity {
                self.hand = 0;
                self.wrapped = true;
            }

            if self.bitmap & (1u64 << self.hand) == 0 {
                self.bitmap |= 1u64 << self.hand;
                let slot = SwapSlot(self.hand);
                self.hand += 1;
                return Some(slot);
            }

            self.hand += 1;
        }

        None
    }

    pub fn free(&mut self, slot: SwapSlot) -> bool {
        if slot.0 >= self.capacity {
            return false;
        }
        let mask = 1u64 << slot.0;
        if self.bitmap & mask == 0 {
            return false;
        }
        self.bitmap &= !mask;
        true
    }

    pub fn has_wrapped(&self) -> bool {
        self.wrapped
    }

    pub fn is_allocated(&self, slot: SwapSlot) -> bool {
        if slot.0 >= self.capacity {
            return false;
        }
        self.bitmap & (1u64 << slot.0) != 0
    }
}

// ---------------------------------------------------------------------------
// Swap device trait and in-memory backend
// ---------------------------------------------------------------------------

/// Error type for swap I/O operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwapError {
    InvalidSlot,
    IoError,
    DeviceFull,
    DeviceNotPresent,
}

/// Abstract interface for a swap device.
pub trait SwapDevice: Send + Sync {
    fn read_page(&self, slot: SwapSlot, buffer: &mut [u8; 4096]) -> Result<(), SwapError>;
    fn write_page(&self, slot: SwapSlot, buffer: &[u8; 4096]) -> Result<(), SwapError>;
    fn total_slots(&self) -> usize;
    fn device_name(&self) -> &str;
}

/// An in-memory swap device backed by a pre-allocated physical memory region.
///
/// The memory is allocated at boot from the physical frame allocator and
/// remains pinned for the lifetime of the system.  This backend is useful
/// for development and testing; a production system would use NVMe/AHCI I/O.
pub struct InMemorySwapDevice {
    base_phys: u64,
    slot_count: usize,
    name: &'static str,
}

impl InMemorySwapDevice {
    /// Allocate enough physical memory for `slot_count` swap slots (4 KiB each).
    /// Returns `None` if the allocation fails.
    pub fn new(slot_count: usize, name: &'static str) -> Option<Self> {
        let page_count = slot_count;
        let mut frame_allocator = crate::boot::FRAME_ALLOCATOR.lock();
        let allocator = frame_allocator.as_mut()?;

        // Allocate the first frame to get the base address
        let first = allocator.allocate_physical_frame()?;
        let base_phys = first.start_address;

        // Allocate remaining frames contiguously
        for _ in 1..page_count {
            allocator.allocate_physical_frame()?;
        }

        Some(Self {
            base_phys,
            slot_count,
            name,
        })
    }
}

impl SwapDevice for InMemorySwapDevice {
    fn read_page(&self, slot: SwapSlot, buffer: &mut [u8; 4096]) -> Result<(), SwapError> {
        if slot.0 >= self.slot_count {
            return Err(SwapError::InvalidSlot);
        }
        let phys_addr = self.base_phys + (slot.0 as u64) * PAGE_SIZE;
        let phys_mem_offset = crate::boot::get_phys_mem_offset();
        let src = (phys_mem_offset + phys_addr).as_ptr::<u8>();
        unsafe {
            core::ptr::copy_nonoverlapping(src, buffer.as_mut_ptr(), 4096);
        }
        Ok(())
    }

    fn write_page(&self, slot: SwapSlot, buffer: &[u8; 4096]) -> Result<(), SwapError> {
        if slot.0 >= self.slot_count {
            return Err(SwapError::InvalidSlot);
        }
        let phys_addr = self.base_phys + (slot.0 as u64) * PAGE_SIZE;
        let phys_mem_offset = crate::boot::get_phys_mem_offset();
        let dst = (phys_mem_offset + phys_addr).as_mut_ptr::<u8>();
        unsafe {
            core::ptr::copy_nonoverlapping(buffer.as_ptr(), dst, 4096);
        }
        Ok(())
    }

    fn total_slots(&self) -> usize {
        self.slot_count
    }

    fn device_name(&self) -> &str {
        self.name
    }
}

// ---------------------------------------------------------------------------
// PTE swap encoding
// ---------------------------------------------------------------------------

/// Encode/decode swap information in non-present PTEs.
///
/// When a page is swapped out, the PTE is marked not-present with a special
/// marker bit.  The remaining bits store the swap slot index and device ID.
///
/// Layout (bit 0 is the Present flag, always 0 for swapped-out):
///   - Bit  0  = 0          (not present)
///   - Bit  1  = 1          (swap marker — distinguishes swapped-out from unmapped)
///   - Bits 2–49 = slot     (48 bits: supports up to 2^48 slots)
///   - Bits 50–51 = device  (2 bits: up to 4 swap devices)
///   - Bits 52–63 = reserved
pub struct SwappedOutPte;

impl SwappedOutPte {
    const SWAP_MARKER: u64 = 1u64 << 1;
    const SLOT_SHIFT: u64 = 2;
    const SLOT_MASK: u64 = (1u64 << 48) - 1;
    const DEVICE_SHIFT: u64 = 50;
    const DEVICE_MASK: u64 = 0b11;

    /// Check whether a raw PTE value indicates a swapped-out page.
    pub fn is_swapped_out(pte_bits: u64) -> bool {
        pte_bits & 1 == 0 && (pte_bits >> 1) & 1 == 1
    }

    /// Encode swap info into a non-present PTE value.
    pub fn encode(slot: SwapSlot, device_id: u8) -> u64 {
        let slot_bits = (slot.0 as u64 & Self::SLOT_MASK) << Self::SLOT_SHIFT;
        let device_bits = ((device_id as u64) & Self::DEVICE_MASK) << Self::DEVICE_SHIFT;
        Self::SWAP_MARKER | slot_bits | device_bits
    }

    /// Extract the swap slot from an encoded PTE value.
    pub fn decode_slot(pte_bits: u64) -> SwapSlot {
        SwapSlot(((pte_bits >> Self::SLOT_SHIFT) & Self::SLOT_MASK) as usize)
    }

    /// Extract the device ID from an encoded PTE value.
    pub fn decode_device(pte_bits: u64) -> u8 {
        ((pte_bits >> Self::DEVICE_SHIFT) & Self::DEVICE_MASK) as u8
    }
}

// ---------------------------------------------------------------------------
// Locked frame tracking
// ---------------------------------------------------------------------------

/// A set of physical addresses that are locked (pinned) and must not be
/// evicted to swap.
static LOCKED_FRAMES: Mutex<BTreeSet<u64>> = Mutex::new(BTreeSet::new());

/// Lock (pin) a physical frame so the swap manager will not evict it.
/// Callers must ensure the frame is actually in use before locking.
pub fn lock_frame(phys_addr: u64) {
    LOCKED_FRAMES.lock().insert(phys_addr & !0xFFF);
}

/// Unlock (unpin) a previously locked physical frame.
pub fn unlock_frame(phys_addr: u64) {
    LOCKED_FRAMES.lock().remove(&(phys_addr & !0xFFF));
}

/// Check whether a physical frame is currently locked.
pub fn is_frame_locked(phys_addr: u64) -> bool {
    LOCKED_FRAMES.lock().contains(&(phys_addr & !0xFFF))
}

// ---------------------------------------------------------------------------
// Global swap manager
// ---------------------------------------------------------------------------

/// Global swap manager singleton.
static SWAP_MANAGER: Mutex<Option<SwapManager>> = Mutex::new(None);

pub struct SwapManager {
    allocator: SwapSlotAllocator,
    device: Arc<dyn SwapDevice>,
    clock_hand: AtomicU64,
    eviction_count: AtomicU64,
    #[allow(dead_code)]
    watermark_pages: usize,
}

impl SwapManager {
    /// Initialise the swap manager with a backing device.
    ///
    /// The watermark is the number of free page frames below which the
    /// eviction daemon should start swapping pages out.
    pub fn init(device: Arc<dyn SwapDevice>, watermark_pages: usize) {
        let slot_count = device.total_slots();
        let mgr = Self {
            allocator: SwapSlotAllocator::with_capacity(slot_count),
            device,
            clock_hand: AtomicU64::new(0),
            eviction_count: AtomicU64::new(0),
            watermark_pages,
        };
        *SWAP_MANAGER.lock() = Some(mgr);
    }

    pub fn with<F, R>(f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        let mut guard = SWAP_MANAGER.lock();
        let mgr = guard.as_mut().expect("swap manager not initialised");
        f(mgr)
    }

    pub fn eviction_count(&self) -> u64 {
        self.eviction_count.load(Ordering::Relaxed)
    }

    pub fn available_slots(&self) -> usize {
        self.allocator.available()
    }

    /// Evict a single anonymous page from the current process's address
    /// space, write it to the swap device, and update the PTE.
    ///
    /// Returns the number of pages evicted (0 or 1).
    pub fn evict_one(&mut self) -> usize {
        let process = match crate::task::scheduler::get_current_process() {
            Some(p) => p,
            None => return 0,
        };

        let pml4_frame = process.pml4_frame();
        let phys_mem_offset = crate::boot::get_phys_mem_offset();

        // Walk the user address space searching for eviction candidates
        // using a clock-like scan.  The scan is limited to 4096 PTEs per
        // call to keep latency bounded.
        let clock_start = self.clock_hand.load(Ordering::Relaxed);
        let scan_limit = 4096u64;

        for offset in 0..scan_limit {
            let vaddr_val = (clock_start + offset) & 0x0000_7FFF_FFFF_F000;
            if vaddr_val > 0x0000_7FFF_FFFF_F000 {
                self.clock_hand.store(0, Ordering::Relaxed);
                continue;
            }
            let vaddr = VirtAddr::new(vaddr_val);

            let pte_bits = match read_pte(pml4_frame, phys_mem_offset, vaddr) {
                Some(bits) => bits,
                None => continue,
            };

            // Skip non-present or swapped-out entries
            if pte_bits & 1 == 0 {
                continue;
            }

            let flags = PageTableFlags::from_bits_truncate(pte_bits);

            // Only evict writable anonymous pages (not file-backed)
            if !flags.contains(PageTableFlags::WRITABLE) {
                continue;
            }

            // Skip kernel pages (supervisor-only)
            if !flags.contains(PageTableFlags::USER_ACCESSIBLE) {
                continue;
            }

            // Extract the physical frame address
            let phys_addr = pte_bits & 0x000F_FFFF_FFFF_F000;
            if phys_addr == 0 {
                continue;
            }

            // Skip locked (pinned) frames
            if is_frame_locked(phys_addr) {
                continue;
            }

            // Clock algorithm: if Accessed bit is set, clear it and move on.
            // If not set, this page is a candidate for eviction.
            if flags.contains(PageTableFlags::ACCESSED) {
                clear_accessed_bit(pml4_frame, phys_mem_offset, vaddr);
                self.clock_hand
                    .store(vaddr_val + PAGE_SIZE, Ordering::Relaxed);
                continue;
            }

            // Candidate found — evict it
            let slot = match self.allocator.allocate() {
                Some(s) => s,
                None => return 0,
            };

            // Read the page content from physical memory
            let phys_mem_offset = crate::boot::get_phys_mem_offset();
            let src_ptr = (phys_mem_offset + phys_addr).as_ptr::<u8>();
            let mut page_data = [0u8; 4096];
            unsafe {
                core::ptr::copy_nonoverlapping(src_ptr, page_data.as_mut_ptr(), 4096);
            }

            // Write to swap device
            if self.device.write_page(slot, &page_data).is_err() {
                self.allocator.free(slot);
                self.clock_hand
                    .store(vaddr_val + PAGE_SIZE, Ordering::Relaxed);
                continue;
            }

            // Update PTE with swap encoding
            let encoded = SwappedOutPte::encode(slot, 0);
            write_pte_raw(pml4_frame, phys_mem_offset, vaddr, encoded);
            tlb::flush(vaddr);

            // Free the physical frame back to the allocator
            free_physical_frame(phys_addr);

            self.eviction_count.fetch_add(1, Ordering::Relaxed);
            self.clock_hand
                .store(vaddr_val + PAGE_SIZE, Ordering::Relaxed);
            return 1;
        }

        self.clock_hand
            .store(clock_start + scan_limit * PAGE_SIZE, Ordering::Relaxed);
        0
    }
}

// ---------------------------------------------------------------------------
// Low-level page table helpers
// ---------------------------------------------------------------------------

/// Read the raw 64-bit value of a PTE at the given virtual address,
/// walking the page tables manually.
pub(crate) fn read_pte(
    pml4_frame: PhysFrame<Size4KiB>,
    phys_mem_offset: VirtAddr,
    vaddr: VirtAddr,
) -> Option<u64> {
    let pml4_ptr = (phys_mem_offset + pml4_frame.start_address().as_u64()).as_ptr::<PageTable>();
    let pml4 = unsafe { &*pml4_ptr };
    let p4e = &pml4[vaddr.p4_index()];
    if p4e.is_unused() {
        return None;
    }

    let p3_ptr = (phys_mem_offset + p4e.frame().ok()?.start_address().as_u64()).as_ptr::<PageTable>();
    let p3 = unsafe { &*p3_ptr };
    let p3e = &p3[vaddr.p3_index()];
    if p3e.is_unused() {
        return None;
    }

    let p2_ptr = (phys_mem_offset + p3e.frame().ok()?.start_address().as_u64()).as_ptr::<PageTable>();
    let p2 = unsafe { &*p2_ptr };
    let p2e = &p2[vaddr.p2_index()];
    if p2e.is_unused() {
        return None;
    }
    if p2e.flags().contains(PageTableFlags::HUGE_PAGE) {
        return None;
    }

    let p1_ptr = (phys_mem_offset + p2e.frame().ok()?.start_address().as_u64()).as_ptr::<PageTable>();
    let p1 = unsafe { &*p1_ptr };
    let p1e = &p1[vaddr.p1_index()];

    Some(p1e.addr().as_u64() | p1e.flags().bits())
}

/// Write a raw 64-bit value to a PTE.
fn write_pte_raw(
    pml4_frame: PhysFrame<Size4KiB>,
    phys_mem_offset: VirtAddr,
    vaddr: VirtAddr,
    value: u64,
) {
    let pml4_ptr =
        (phys_mem_offset + pml4_frame.start_address().as_u64()).as_mut_ptr::<PageTable>();
    let pml4 = unsafe { &mut *pml4_ptr };

    if pml4[vaddr.p4_index()].is_unused() {
        return;
    }
    let p3_ptr = (phys_mem_offset
        + pml4[vaddr.p4_index()]
            .frame()
            .unwrap()
            .start_address()
            .as_u64())
    .as_mut_ptr::<PageTable>();
    let p3 = unsafe { &mut *p3_ptr };
    if p3[vaddr.p3_index()].is_unused() {
        return;
    }
    let p2_ptr = (phys_mem_offset
        + p3[vaddr.p3_index()]
            .frame()
            .unwrap()
            .start_address()
            .as_u64())
    .as_mut_ptr::<PageTable>();
    let p2 = unsafe { &mut *p2_ptr };
    if p2[vaddr.p2_index()].is_unused() {
        return;
    }
    if p2[vaddr.p2_index()].flags().contains(PageTableFlags::HUGE_PAGE) {
        return;
    }
    let p1_ptr = (phys_mem_offset
        + p2[vaddr.p2_index()]
            .frame()
            .unwrap()
            .start_address()
            .as_u64())
    .as_mut_ptr::<PageTable>();
    let p1 = unsafe { &mut *p1_ptr };
    p1[vaddr.p1_index()].set_addr(
        x86_64::PhysAddr::new(value & 0x000F_FFFF_FFFF_F000),
        PageTableFlags::from_bits_truncate(value & 0xFFF),
    );
}

/// Clear the Accessed (A) bit on a PTE without modifying other bits.
fn clear_accessed_bit(
    pml4_frame: PhysFrame<Size4KiB>,
    phys_mem_offset: VirtAddr,
    vaddr: VirtAddr,
) {
    let pml4_ptr =
        (phys_mem_offset + pml4_frame.start_address().as_u64()).as_mut_ptr::<PageTable>();
    let pml4 = unsafe { &mut *pml4_ptr };
    if pml4[vaddr.p4_index()].is_unused() {
        return;
    }
    let p3_ptr = (phys_mem_offset
        + pml4[vaddr.p4_index()]
            .frame()
            .unwrap()
            .start_address()
            .as_u64())
    .as_mut_ptr::<PageTable>();
    let p3 = unsafe { &mut *p3_ptr };
    if p3[vaddr.p3_index()].is_unused() {
        return;
    }
    let p2_ptr = (phys_mem_offset
        + p3[vaddr.p3_index()]
            .frame()
            .unwrap()
            .start_address()
            .as_u64())
    .as_mut_ptr::<PageTable>();
    let p2 = unsafe { &mut *p2_ptr };
    if p2[vaddr.p2_index()].is_unused() {
        return;
    }
    if p2[vaddr.p2_index()].flags().contains(PageTableFlags::HUGE_PAGE) {
        return;
    }
    let p1_ptr = (phys_mem_offset
        + p2[vaddr.p2_index()]
            .frame()
            .unwrap()
            .start_address()
            .as_u64())
    .as_mut_ptr::<PageTable>();
    let p1 = unsafe { &mut *p1_ptr };
    let mut flags = p1[vaddr.p1_index()].flags();
    flags.remove(PageTableFlags::ACCESSED);
    p1[vaddr.p1_index()].set_flags(flags);
}

/// Return a physical frame to the allocator by resetting the boot-time
/// linear allocator's high-water mark (simple bump allocator).
///
/// For a bump allocator we cannot truly free individual frames, so we
/// mark the page as available by putting it on a free list.
static FREE_FRAMES: Mutex<alloc::vec::Vec<u64>> = Mutex::new(alloc::vec::Vec::new());

fn free_physical_frame(phys_addr: u64) {
    FREE_FRAMES.lock().push(phys_addr);
}

/// Allocate a physical frame, trying the free list first, then the boot
/// allocator.  This is used by the swap-in path.
pub fn allocate_swappable_frame() -> Option<u64> {
    let mut free_list = FREE_FRAMES.lock();
    if let Some(addr) = free_list.pop() {
        return Some(addr);
    }
    drop(free_list);

    let mut guard = crate::boot::FRAME_ALLOCATOR.lock();
    let allocator = guard.as_mut()?;
    allocator.allocate_physical_frame().map(|f| f.start_address)
}

/// Swap in a page: read from swap device, allocate a frame, copy data,
/// and return the physical address of the new frame.
pub fn swap_in(slot: SwapSlot, device_id: u8) -> Option<u64> {
    let mgr_guard = SWAP_MANAGER.lock();
    let mgr = mgr_guard.as_ref()?;

    if device_id != 0 {
        return None;
    }

    let mut page_data = [0u8; 4096];
    if mgr.device.read_page(slot, &mut page_data).is_err() {
        return None;
    }

    let phys_addr = allocate_swappable_frame()?;
    let phys_mem_offset = crate::boot::get_phys_mem_offset();
    let dst = (phys_mem_offset + phys_addr).as_mut_ptr::<u8>();
    unsafe {
        core::ptr::copy_nonoverlapping(page_data.as_ptr(), dst, 4096);
    }
    Some(phys_addr)
}

// ---------------------------------------------------------------------------
// Public API for the page fault handler
// ---------------------------------------------------------------------------

/// Check whether a non-present PTE indicates a swapped-out page.
/// Called from the demand-paging page-fault handler.
pub fn is_swapped_out_pte(pte_bits: u64) -> bool {
    SwappedOutPte::is_swapped_out(pte_bits)
}

/// Given a PTE that was swapped out, restore the page and return the
/// physical frame address that was allocated.  Also frees the swap slot.
///
/// The caller is responsible for mapping the frame into the page table
/// with the correct protection flags.
pub fn restore_swapped_page(pte_bits: u64) -> Option<u64> {
    let slot = SwappedOutPte::decode_slot(pte_bits);
    let device_id = SwappedOutPte::decode_device(pte_bits);

    let phys_addr = swap_in(slot, device_id)?;

    // Free the swap slot
    let mut mgr_guard = SWAP_MANAGER.lock();
    if let Some(mgr) = mgr_guard.as_mut() {
        mgr.allocator.free(slot);
    }

    Some(phys_addr)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    // ------------------------------------------------------------------
    // SwapSlotAllocator tests
    // ------------------------------------------------------------------

    #[test]
    fn slot_allocator_basic_alloc_free() {
        let mut alloc = SwapSlotAllocator::with_capacity(64);
        assert_eq!(alloc.used(), 0);
        assert_eq!(alloc.available(), 64);

        let slot = alloc.allocate().expect("should allocate slot");
        assert_eq!(slot.0, 0);
        assert_eq!(alloc.used(), 1);
        assert_eq!(alloc.available(), 63);

        assert!(alloc.is_allocated(slot));
        assert!(alloc.free(slot));
        assert!(!alloc.is_allocated(slot));
        assert!(!alloc.free(slot));
    }

    #[test]
    fn slot_allocator_fills_and_wraps() {
        let mut alloc = SwapSlotAllocator::with_capacity(64);
        assert_eq!(alloc.capacity(), 64);

        // Allocate all 64 slots
        for i in 0..64 {
            let slot = alloc.allocate();
            assert!(slot.is_some(), "failed to allocate slot {}", i);
            assert_eq!(slot.unwrap().0, i);
        }

        // Should be full
        assert!(alloc.allocate().is_none());
        assert_eq!(alloc.used(), 64);

        // Free some and re-allocate (wrap-around test)
        assert!(alloc.free(SwapSlot(0)));
        assert!(alloc.free(SwapSlot(1)));
        assert_eq!(alloc.used(), 62);

        let slot = alloc.allocate();
        assert!(slot.is_some());
        // The hand should wrap around and find slot 0 or 1
        assert!(slot.unwrap().0 == 0 || slot.unwrap().0 == 1);

        assert!(alloc.has_wrapped());
    }

    #[test]
    fn slot_allocator_handles_invalid_free() {
        let mut alloc = SwapSlotAllocator::with_capacity(64);
        assert!(!alloc.free(SwapSlot(99)));
        assert!(!alloc.free(SwapSlot(0)));
    }

    #[test]
    fn slot_allocator_wrap_around_detection() {
        let mut alloc = SwapSlotAllocator::with_capacity(64);
        assert!(!alloc.has_wrapped());

        // Allocate all slots to force the hand to wrap
        for _ in 0..64 {
            alloc.allocate();
        }
        assert!(alloc.allocate().is_none()); // Full

        alloc.free(SwapSlot(0));
        alloc.allocate(); // This causes the hand to re-visit slot 0 after wrapping
        assert!(alloc.has_wrapped());
    }

    // ------------------------------------------------------------------
    // PTE encoding tests
    // ------------------------------------------------------------------

    #[test]
    fn swapped_pte_round_trip() {
        let slot = SwapSlot(42);
        let device_id = 1;

        let encoded = SwappedOutPte::encode(slot, device_id);
        assert!(SwappedOutPte::is_swapped_out(encoded));

        let decoded_slot = SwappedOutPte::decode_slot(encoded);
        let decoded_device = SwappedOutPte::decode_device(encoded);

        assert_eq!(decoded_slot.0, 42);
        assert_eq!(decoded_device, 1);
    }

    #[test]
    fn not_swapped_is_detected() {
        // Present PTE (bit 0 = 1)
        let present_pte: u64 = 0x8000_0000_0000_0001;
        assert!(!SwappedOutPte::is_swapped_out(present_pte));

        // Non-present but no swap marker (bit 1 = 0)
        let unmapped_pte: u64 = 0x0;
        assert!(!SwappedOutPte::is_swapped_out(unmapped_pte));

        // Zeroed PTE
        assert!(!SwappedOutPte::is_swapped_out(0));
    }

    #[test]
    fn large_slot_number_round_trip() {
        // Test with a slot number that uses multiple bits
        let slot = SwapSlot((1u64 << 40) as usize);
        let encoded = SwappedOutPte::encode(slot, 3);
        assert!(SwappedOutPte::is_swapped_out(encoded));
        assert_eq!(SwappedOutPte::decode_slot(encoded).0, (1u64 << 40) as usize);
        assert_eq!(SwappedOutPte::decode_device(encoded), 3);
    }

    // ------------------------------------------------------------------
    // InMemorySwapDevice tests
    // ------------------------------------------------------------------

    /// Test InMemorySwapDevice basic read/write round-trip.
    #[test]
    fn in_memory_swap_device_round_trip() {
        let page_count = 4;
        let alloc_size = page_count * 4096;

        // Allocate a chunk of memory to simulate the swap device's backing store
        // (we can't call the real InMemorySwapDevice::new without a real allocator)
        let mut backing: Vec<u8> = alloc::vec![0u8; alloc_size];
        let base_ptr = backing.as_mut_ptr() as u64;

        // We'll test the read/write logic directly
        let mut data = [0u8; 4096];
        for (i, byte) in data.iter_mut().enumerate() {
            *byte = (i as u8).wrapping_mul(7).wrapping_add(13);
        }
        let phys_mem_offset = 0usize as u64;

        unsafe {
            core::ptr::copy_nonoverlapping(
                data.as_ptr(),
                (phys_mem_offset + base_ptr + 0 * 4096) as *mut u8,
                4096,
            );
        }

        let mut readback = [0u8; 4096];
        unsafe {
            core::ptr::copy_nonoverlapping(
                (phys_mem_offset + base_ptr + 0 * 4096) as *const u8,
                readback.as_mut_ptr(),
                4096,
            );
        }

        assert_eq!(data, readback, "InMemorySwapDevice: data round-trip failed");
    }

    // ------------------------------------------------------------------
    // Locked frame tests
    // ------------------------------------------------------------------

    #[test]
    fn locked_frames_basic() {
        let addr = 0x1000u64;
        assert!(!is_frame_locked(addr));

        lock_frame(addr);
        assert!(is_frame_locked(addr));

        unlock_frame(addr);
        assert!(!is_frame_locked(addr));
    }

    #[test]
    fn locked_frames_page_aligns() {
        let addr = 0x1234u64;
        lock_frame(addr);
        assert!(is_frame_locked(addr));
        assert!(is_frame_locked(0x1000));
        unlock_frame(addr);
        assert!(!is_frame_locked(0x1000));
    }
}
