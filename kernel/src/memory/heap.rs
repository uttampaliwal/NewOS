use x86_64::{
    VirtAddr,
    structures::paging::{
        FrameAllocator, Mapper, Page, PageTableFlags, Size4KiB, mapper::MapToError,
    },
};

use crate::memory::allocator::Locked;
use crate::memory::allocator::fixed_size_block::FixedSizeBlockAllocator;

pub const HEAP_START: usize = 0xFFFF_A000_0000_0000;
pub const HEAP_SIZE: usize = 8 * 1024 * 1024; // 8 MiB

/// Shadow memory lives just below the heap.  Each 8 bytes of heap maps to
/// 1 byte of shadow, so shadow_size = HEAP_SIZE / 8 = 1 MiB.
pub const KASAN_SHADOW_SIZE: usize = HEAP_SIZE / 8;
pub const KASAN_SHADOW_START: usize = HEAP_START - KASAN_SHADOW_SIZE;

#[cfg_attr(not(test), global_allocator)]
pub static ALLOCATOR: Locked<FixedSizeBlockAllocator> = Locked::new(FixedSizeBlockAllocator::new());

pub fn init_heap(
    mapper: &mut impl Mapper<Size4KiB>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) -> Result<(), MapToError<Size4KiB>> {
    let page_range = {
        let heap_start = VirtAddr::new(HEAP_START as u64);
        let heap_end = heap_start + HEAP_SIZE as u64 - 1u64;
        let heap_start_page = Page::containing_address(heap_start);
        let heap_end_page = Page::containing_address(heap_end);
        Page::range_inclusive(heap_start_page, heap_end_page)
    };

    for page in page_range {
        let frame = frame_allocator
            .allocate_frame()
            .ok_or(MapToError::FrameAllocationFailed)?;
        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
        unsafe {
            mapper.map_to(page, frame, flags, frame_allocator)?.flush();
        }
    }

    // Map KASAN shadow pages just below the heap
    let shadow_page_range = {
        let shadow_start = VirtAddr::new(KASAN_SHADOW_START as u64);
        let shadow_end = VirtAddr::new((KASAN_SHADOW_START + KASAN_SHADOW_SIZE - 1) as u64);
        let shadow_start_page = Page::containing_address(shadow_start);
        let shadow_end_page = Page::containing_address(shadow_end);
        Page::range_inclusive(shadow_start_page, shadow_end_page)
    };

    for page in shadow_page_range {
        let frame = frame_allocator
            .allocate_frame()
            .ok_or(MapToError::FrameAllocationFailed)?;
        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
        unsafe {
            mapper.map_to(page, frame, flags, frame_allocator)?.flush();
        }
    }

    unsafe {
        ALLOCATOR.lock().init(HEAP_START, HEAP_SIZE);
    }

    crate::memory::kasan::init(HEAP_START, HEAP_START + HEAP_SIZE, KASAN_SHADOW_START);

    Ok(())
}
