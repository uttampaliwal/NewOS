use newos_abi::boot::{BootInfo, BootMemoryDescriptor, MEMORY_TYPE_CONVENTIONAL};
use x86_64::PhysAddr;
use x86_64::structures::paging::{
    FrameAllocator as X86FrameAllocator, PhysFrame as X86PhysFrame, Size4KiB,
};

pub mod allocator;
pub mod heap;
pub mod paging;
pub mod user;

pub const PAGE_SIZE: u64 = 4096;
const LOW_MEMORY_CUTOFF: u64 = 0x100000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysFrame {
    pub start_address: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemorySummary {
    pub descriptor_count: usize,
    pub conventional_region_count: usize,
    pub conventional_page_count: u64,
    pub largest_conventional_region_pages: u64,
}

impl MemorySummary {
    pub fn from_boot_info(boot_info: &BootInfo) -> Self {
        let mut descriptor_count = 0usize;
        let mut conventional_region_count = 0usize;
        let mut conventional_page_count = 0u64;
        let mut largest_conventional_region_pages = 0u64;

        for descriptor in boot_info.memory_map.iter() {
            descriptor_count += 1;

            if descriptor.ty == MEMORY_TYPE_CONVENTIONAL {
                conventional_region_count += 1;
                conventional_page_count += descriptor.page_count;
                largest_conventional_region_pages =
                    largest_conventional_region_pages.max(descriptor.page_count);
            }
        }

        Self {
            descriptor_count,
            conventional_region_count,
            conventional_page_count,
            largest_conventional_region_pages,
        }
    }
}

pub struct FrameAllocator<'a> {
    boot_info: &'a BootInfo,
    next_address: u64,
}

impl<'a> FrameAllocator<'a> {
    pub fn new(boot_info: &'a BootInfo) -> Self {
        Self {
            boot_info,
            next_address: LOW_MEMORY_CUTOFF,
        }
    }

    pub fn allocate_physical_frame(&mut self) -> Option<PhysFrame> {
        for descriptor in self.boot_info.memory_map.iter() {
            if descriptor.ty != MEMORY_TYPE_CONVENTIONAL {
                continue;
            }

            let range = usable_range(descriptor)?;
            let candidate = align_up(self.next_address.max(range.start), PAGE_SIZE);

            if candidate < range.end {
                self.next_address = candidate.checked_add(PAGE_SIZE)?;
                return Some(PhysFrame {
                    start_address: candidate,
                });
            }
        }

        None
    }
}

unsafe impl<'a> X86FrameAllocator<Size4KiB> for FrameAllocator<'a> {
    fn allocate_frame(&mut self) -> Option<X86PhysFrame<Size4KiB>> {
        let frame = self.allocate_physical_frame()?;
        Some(X86PhysFrame::containing_address(PhysAddr::new(
            frame.start_address,
        )))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AddressRange {
    start: u64,
    end: u64,
}

fn usable_range(descriptor: &BootMemoryDescriptor) -> Option<AddressRange> {
    let start = descriptor.phys_start;
    let length = descriptor.page_count.checked_mul(PAGE_SIZE)?;
    let end = start.checked_add(length)?;

    Some(AddressRange { start, end })
}

fn align_up(value: u64, alignment: u64) -> u64 {
    if alignment == 0 {
        return value;
    }

    value
        .checked_add(alignment - 1)
        .map(|adjusted| adjusted & !(alignment - 1))
        .unwrap_or(value)
}

#[cfg(test)]
mod tests {
    use core::mem::size_of;

    use newos_abi::boot::{BootEnvironment, BootInfo, BootLoaderKind, BootMemoryMap};

    use super::*;

    #[test]
    fn memory_summary_counts_conventional_regions() {
        let descriptors = [
            descriptor(MEMORY_TYPE_CONVENTIONAL, 0x100000, 4),
            descriptor(0, 0x200000, 2),
            descriptor(MEMORY_TYPE_CONVENTIONAL, 0x300000, 8),
        ];

        let boot_info = boot_info(&descriptors);
        let summary = MemorySummary::from_boot_info(&boot_info);

        assert_eq!(summary.descriptor_count, 3);
        assert_eq!(summary.conventional_region_count, 2);
        assert_eq!(summary.conventional_page_count, 12);
        assert_eq!(summary.largest_conventional_region_pages, 8);
    }

    #[test]
    fn frame_allocator_skips_non_conventional_ranges() {
        let descriptors = [
            descriptor(0, 0x100000, 4),
            descriptor(MEMORY_TYPE_CONVENTIONAL, 0x200000, 4),
        ];

        let boot_info = boot_info(&descriptors);
        let mut allocator = FrameAllocator::new(&boot_info);

        assert_eq!(
            allocator.allocate_physical_frame(),
            Some(PhysFrame {
                start_address: 0x200000,
            })
        );
    }

    #[test]
    fn frame_allocator_advances_across_regions() {
        let descriptors = [
            descriptor(MEMORY_TYPE_CONVENTIONAL, 0x100000, 2),
            descriptor(MEMORY_TYPE_CONVENTIONAL, 0x400000, 2),
        ];

        let boot_info = boot_info(&descriptors);
        let mut allocator = FrameAllocator::new(&boot_info);

        assert_eq!(
            allocator
                .allocate_physical_frame()
                .map(|frame| frame.start_address),
            Some(0x100000)
        );
        assert_eq!(
            allocator
                .allocate_physical_frame()
                .map(|frame| frame.start_address),
            Some(0x101000)
        );
        assert_eq!(
            allocator
                .allocate_physical_frame()
                .map(|frame| frame.start_address),
            Some(0x400000)
        );
        assert_eq!(
            allocator
                .allocate_physical_frame()
                .map(|frame| frame.start_address),
            Some(0x401000)
        );
        assert_eq!(allocator.allocate_physical_frame(), None);
    }

    #[test]
    fn frame_allocator_skips_low_memory() {
        let descriptors = [
            descriptor(MEMORY_TYPE_CONVENTIONAL, 0x0, 256),
            descriptor(MEMORY_TYPE_CONVENTIONAL, 0x100000, 2),
        ];

        let boot_info = boot_info(&descriptors);
        let mut allocator = FrameAllocator::new(&boot_info);

        assert_eq!(
            allocator
                .allocate_physical_frame()
                .map(|frame| frame.start_address),
            Some(0x100000)
        );
    }

    fn descriptor(ty: u32, phys_start: u64, page_count: u64) -> BootMemoryDescriptor {
        BootMemoryDescriptor {
            ty,
            reserved: 0,
            phys_start,
            virt_start: 0,
            page_count,
            att: 0,
        }
    }

    fn boot_info(descriptors: &[BootMemoryDescriptor]) -> BootInfo {
        BootInfo {
            abi_version: 1,
            environment: BootEnvironment::Uefi,
            loader: BootLoaderKind::UefiLoader,
            flags: 0,
            kernel_image_base: 0,
            kernel_image_size: 0,
            memory_map: BootMemoryMap {
                descriptors: descriptors.as_ptr(),
                map_size: descriptors.len() * size_of::<BootMemoryDescriptor>(),
                desc_size: size_of::<BootMemoryDescriptor>(),
                desc_version: 1,
            },
        }
    }
}
