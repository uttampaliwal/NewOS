use turnix_abi::boot::{BootInfo, BootMemoryDescriptor, MEMORY_TYPE_CONVENTIONAL};
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

pub struct FrameAllocator<'a> {
    boot_info: &'a BootInfo,
    next_address: u64,
}

unsafe impl Sync for FrameAllocator<'static> {}
unsafe impl Send for FrameAllocator<'static> {}

impl<'a> FrameAllocator<'a> {
    pub fn new(boot_info: &'a BootInfo) -> Self {
        Self {
            boot_info,
            next_address: LOW_MEMORY_CUTOFF,
        }
    }

    pub fn allocate_physical_frame(&mut self) -> Option<PhysFrame> {
        use turnix_abi::boot::{
            MEMORY_TYPE_BOOT_SERVICES_CODE, MEMORY_TYPE_BOOT_SERVICES_DATA,
        };

        for descriptor in self.boot_info.memory_map.iter() {
            let is_usable = match descriptor.ty {
                MEMORY_TYPE_CONVENTIONAL => true,
                MEMORY_TYPE_BOOT_SERVICES_CODE => true,
                MEMORY_TYPE_BOOT_SERVICES_DATA => true,
                _ => false,
            };

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
                map_size: descriptors.len() * size_of::<BootMemoryDescriptor>(),
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
}

