use core::mem::size_of;

#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootEnvironment {
    Unknown = 0,
    Uefi = 1,
}

#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootLoaderKind {
    Unknown = 0,
    UefiLoader = 1,
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootOutcome {
    ExitSuccess = 0,
    ExitFailure = 1,
}

pub const BOOT_FLAG_BOOT_SERVICES_EXITED: u32 = 1 << 0;
pub const MEMORY_TYPE_CONVENTIONAL: u32 = 7;
pub const MEMORY_TYPE_LOADER_DATA: u32 = 2;
pub const MEMORY_TYPE_BOOT_SERVICES_CODE: u32 = 3;
pub const MEMORY_TYPE_BOOT_SERVICES_DATA: u32 = 4;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BootMemoryDescriptor {
    pub ty: u32,
    pub reserved: u32,
    pub phys_start: u64,
    pub virt_start: u64,
    pub page_count: u64,
    pub att: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BootMemoryMap {
    pub descriptors: *const BootMemoryDescriptor,
    pub map_size: usize,
    pub desc_size: usize,
    pub desc_version: u32,
}

impl BootMemoryMap {
    pub const fn empty() -> Self {
        Self {
            descriptors: core::ptr::null(),
            map_size: 0,
            desc_size: 0,
            desc_version: 0,
        }
    }

    #[allow(clippy::manual_checked_ops)]
    pub const fn entry_count(&self) -> usize {
        if self.desc_size == 0 {
            0
        } else {
            self.map_size / self.desc_size
        }
    }

    pub fn get(&self, index: usize) -> Option<&BootMemoryDescriptor> {
        if index >= self.entry_count()
            || self.descriptors.is_null()
            || self.desc_size < size_of::<BootMemoryDescriptor>()
        {
            return None;
        }

        let offset = index.checked_mul(self.desc_size)?;
        unsafe {
            Some(
                &*self
                    .descriptors
                    .cast::<u8>()
                    .add(offset)
                    .cast::<BootMemoryDescriptor>(),
            )
        }
    }

    pub fn iter(&self) -> BootMemoryMapIter<'_> {
        BootMemoryMapIter {
            map: self,
            index: 0,
        }
    }
}

pub struct BootMemoryMapIter<'a> {
    map: &'a BootMemoryMap,
    index: usize,
}

impl<'a> Iterator for BootMemoryMapIter<'a> {
    type Item = &'a BootMemoryDescriptor;

    fn next(&mut self) -> Option<Self::Item> {
        let item = self.map.get(self.index)?;
        self.index += 1;
        Some(item)
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BootFramebuffer {
    pub addr: u64,
    pub size: u64,
    pub width: u32,
    pub height: u32,
    pub pitch: u32,
    pub format: u32, // 0: BGRX8888, 1: RGBX8888
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BootInfo {
    pub abi_version: u32,
    pub environment: BootEnvironment,
    pub loader: BootLoaderKind,
    pub flags: u32,
    pub kernel_image_base: u64,
    pub kernel_image_size: u64,
    pub physical_memory_offset: u64,
    pub ramdisk_addr: u64,
    pub ramdisk_size: u64,
    pub memory_map: BootMemoryMap,
    pub framebuffer: BootFramebuffer,
}

impl BootInfo {
    pub const fn uefi(abi_version: u32) -> Self {
        Self {
            abi_version,
            environment: BootEnvironment::Uefi,
            loader: BootLoaderKind::UefiLoader,
            flags: 0,
            kernel_image_base: 0,
            kernel_image_size: 0,
            physical_memory_offset: 0,
            ramdisk_addr: 0,
            ramdisk_size: 0,
            memory_map: BootMemoryMap::empty(),
            framebuffer: BootFramebuffer {
                addr: 0,
                size: 0,
                width: 0,
                height: 0,
                pitch: 0,
                format: 0,
            },
        }
    }

    pub const fn boot_services_exited(&self) -> bool {
        (self.flags & BOOT_FLAG_BOOT_SERVICES_EXITED) != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uefi_boot_info_uses_expected_defaults() {
        let boot_info = BootInfo::uefi(3);

        assert_eq!(boot_info.abi_version, 3);
        assert_eq!(boot_info.environment, BootEnvironment::Uefi);
        assert_eq!(boot_info.loader, BootLoaderKind::UefiLoader);
        assert_eq!(boot_info.flags, 0);
        assert_eq!(boot_info.kernel_image_base, 0);
        assert_eq!(boot_info.kernel_image_size, 0);
        assert_eq!(boot_info.physical_memory_offset, 0);
        assert_eq!(boot_info.ramdisk_addr, 0);
        assert_eq!(boot_info.ramdisk_size, 0);
        assert_eq!(boot_info.memory_map.entry_count(), 0);
    }

    #[test]
    fn empty_memory_map_has_no_entries() {
        let map = BootMemoryMap::empty();

        assert_eq!(map.entry_count(), 0);
        assert_eq!(map.get(0), None);
    }

    #[test]
    fn memory_map_iterator_visits_each_descriptor() {
        let descriptors = [
            BootMemoryDescriptor {
                ty: MEMORY_TYPE_CONVENTIONAL,
                reserved: 0,
                phys_start: 0x1000,
                virt_start: 0,
                page_count: 2,
                att: 0,
            },
            BootMemoryDescriptor {
                ty: 0,
                reserved: 0,
                phys_start: 0x4000,
                virt_start: 0,
                page_count: 1,
                att: 0,
            },
        ];

        let map = BootMemoryMap {
            descriptors: descriptors.as_ptr(),
            map_size: descriptors.len() * size_of::<BootMemoryDescriptor>(),
            desc_size: size_of::<BootMemoryDescriptor>(),
            desc_version: 1,
        };

        let mut iter = map.iter();
        assert_eq!(iter.next().map(|desc| desc.phys_start), Some(0x1000));
        assert_eq!(iter.next().map(|desc| desc.phys_start), Some(0x4000));
        assert_eq!(iter.next(), None);
    }
}
