use alloc::vec::Vec;
use turnix_abi::boot::{BootInfo, BootMemoryDescriptor, MEMORY_TYPE_CONVENTIONAL};
use x86_64::PhysAddr;
use x86_64::structures::paging::{
    FrameAllocator as X86FrameAllocator, PhysFrame as X86PhysFrame, Size4KiB,
};

pub mod allocator;
pub mod aslr;
pub mod demand;
pub mod heap;
pub mod oom;
pub mod page_cache;
pub mod paging;
pub mod slab;
pub mod swap;
pub mod user;
pub mod vma;
pub mod wx;

pub const PAGE_SIZE: u64 = 4096;
const LOW_MEMORY_CUTOFF: u64 = 0x100000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysFrame {
    pub start_address: u64,
}

pub struct FrameAllocator<'a> {
    boot_info: &'a BootInfo,
    next_address: u64,
    free_list: Vec<u64>,
}

unsafe impl Sync for FrameAllocator<'static> {}
unsafe impl Send for FrameAllocator<'static> {}

impl<'a> FrameAllocator<'a> {
    pub fn new(boot_info: &'a BootInfo) -> Self {
        Self {
            boot_info,
            next_address: LOW_MEMORY_CUTOFF,
            free_list: Vec::new(),
        }
    }

    pub fn allocate_physical_frame(&mut self) -> Option<PhysFrame> {
        if let Some(start_address) = self.free_list.pop() {
            return Some(PhysFrame { start_address });
        }

        use turnix_abi::boot::{MEMORY_TYPE_BOOT_SERVICES_CODE, MEMORY_TYPE_BOOT_SERVICES_DATA};

        for descriptor in self.boot_info.memory_map.iter() {
            let is_usable = matches!(
                descriptor.ty,
                MEMORY_TYPE_CONVENTIONAL
                    | MEMORY_TYPE_BOOT_SERVICES_CODE
                    | MEMORY_TYPE_BOOT_SERVICES_DATA
            );

            if !is_usable {
                continue;
            }

            let range = usable_range(descriptor).unwrap();
            let candidate = align_up(self.next_address.max(range.start), PAGE_SIZE);

            if candidate < range.end {
                self.next_address = candidate.checked_add(PAGE_SIZE)?;
                return Some(PhysFrame {
                    start_address: candidate,
                });
            }
        }

        // Out of memory — invoke OOM killer and retry once
        if crate::memory::oom::oom_kill_with_retry().is_some() {
            // Retry the allocation after reclaim
            for descriptor in self.boot_info.memory_map.iter() {
                let is_usable = matches!(
                    descriptor.ty,
                    MEMORY_TYPE_CONVENTIONAL
                        | MEMORY_TYPE_BOOT_SERVICES_CODE
                        | MEMORY_TYPE_BOOT_SERVICES_DATA
                );

                if !is_usable {
                    continue;
                }

                let range = usable_range(descriptor).unwrap();
                let candidate = align_up(self.next_address.max(range.start), PAGE_SIZE);

                if candidate < range.end {
                    self.next_address = candidate.checked_add(PAGE_SIZE)?;
                    return Some(PhysFrame {
                        start_address: candidate,
                    });
                }
            }
        }

        None
    }

    pub fn deallocate_physical_frame(&mut self, frame: PhysFrame) {
        self.free_list.push(frame.start_address);
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
    use super::*;
    use core::mem::size_of;
    use turnix_abi::boot::{BootEnvironment, BootInfo, BootLoaderKind, BootMemoryMap};

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
        use turnix_abi::boot::BootFramebuffer;
        BootInfo {
            abi_version: 2,
            environment: BootEnvironment::Uefi,
            loader: BootLoaderKind::UefiLoader,
            flags: 0,
            kernel_image_base: 0,
            kernel_image_size: 0,
            physical_memory_offset: 0,
            ramdisk_addr: 0,
            ramdisk_size: 0,
            memory_map: BootMemoryMap {
                descriptors: descriptors.as_ptr(),
                map_size: core::mem::size_of_val(descriptors),
                desc_size: size_of::<BootMemoryDescriptor>(),
                desc_version: 1,
            },
            framebuffer: BootFramebuffer {
                addr: 0,
                size: 0,
                width: 0,
                height: 0,
                pitch: 0,
                format: 0,
            },
            rsdp_addr: 0,
            kaslr_offset: 0,
        }
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

    #[test]
    fn frame_allocator_empty_map() {
        let descriptors: [BootMemoryDescriptor; 0] = [];
        let boot_info = boot_info(&descriptors);
        let mut allocator = FrameAllocator::new(&boot_info);
        assert!(allocator.allocate_physical_frame().is_none());
    }

    #[test]
    fn frame_allocator_exhausted() {
        let descriptors = [
            descriptor(MEMORY_TYPE_CONVENTIONAL, 0x100000, 3),
        ];
        let boot_info = boot_info(&descriptors);
        let mut allocator = FrameAllocator::new(&boot_info);

        for i in 0..3 {
            let frame = allocator.allocate_physical_frame();
            assert!(frame.is_some(), "allocation {} should succeed", i);
        }
        assert!(allocator.allocate_physical_frame().is_none());
    }

    #[test]
    fn frame_allocator_deallocate_reuse() {
        let descriptors = [
            descriptor(MEMORY_TYPE_CONVENTIONAL, 0x100000, 1),
        ];
        let boot_info = boot_info(&descriptors);
        let mut allocator = FrameAllocator::new(&boot_info);

        let frame = allocator.allocate_physical_frame().unwrap();
        assert_eq!(frame.start_address, 0x100000);

        allocator.deallocate_physical_frame(frame);
        let reused = allocator.allocate_physical_frame().unwrap();
        assert_eq!(reused.start_address, 0x100000);
    }

    #[test]
    fn frame_allocator_skips_non_conventional() {
        use turnix_abi::boot::MEMORY_TYPE_LOADER_DATA;
        let descriptors = [
            // Boot services memory is still usable, so use a type that's skipped
            descriptor(MEMORY_TYPE_LOADER_DATA, 0x100000, 10),
        ];
        let boot_info = boot_info(&descriptors);
        let mut allocator = FrameAllocator::new(&boot_info);
        assert!(allocator.allocate_physical_frame().is_none());
    }

    #[test]
    fn frame_allocator_multiple_regions() {
        let descriptors = [
            descriptor(MEMORY_TYPE_CONVENTIONAL, 0x100000, 2),
            descriptor(MEMORY_TYPE_CONVENTIONAL, 0x200000, 2),
        ];
        let boot_info = boot_info(&descriptors);
        let mut allocator = FrameAllocator::new(&boot_info);

        let f1 = allocator.allocate_physical_frame().unwrap();
        assert_eq!(f1.start_address, 0x100000);
        let f2 = allocator.allocate_physical_frame().unwrap();
        assert_eq!(f2.start_address, 0x101000);
        let f3 = allocator.allocate_physical_frame().unwrap();
        assert_eq!(f3.start_address, 0x200000);
    }
}
