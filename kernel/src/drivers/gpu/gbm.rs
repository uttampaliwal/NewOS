use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use lazy_static::lazy_static;
use spin::Mutex;

use crate::memory::PhysFrame;

const PAGE_SIZE: u64 = 4096;
const BPP: u32 = 4;

pub type GbmBufferId = u64;

pub struct GbmBuffer {
    pub width: u32,
    pub height: u32,
    pub format: u32,
    pub stride: u32,
    pub size: u64,
    pub frames: Vec<PhysFrame>,
}

pub struct GbmManager {
    next_id: u64,
    pub buffers: BTreeMap<GbmBufferId, GbmBuffer>,
}

impl Default for GbmManager {
    fn default() -> Self {
        Self::new()
    }
}

impl GbmManager {
    pub const fn new() -> Self {
        Self {
            next_id: 1,
            buffers: BTreeMap::new(),
        }
    }

    pub fn create(&mut self, width: u32, height: u32, format: u32) -> Option<GbmBufferId> {
        let stride = width * BPP;
        let size = stride as u64 * height as u64;
        let num_pages = size.div_ceil(PAGE_SIZE) as usize;

        if num_pages == 0 {
            return None;
        }

        let mut guard = crate::boot::FRAME_ALLOCATOR.lock();
        let allocator = guard.as_mut()?;

        let mut frames = Vec::with_capacity(num_pages);
        for _ in 0..num_pages {
            let frame = allocator.allocate_physical_frame()?;
            frames.push(frame);
        }
        drop(guard);

        let id = self.next_id;
        self.next_id += 1;

        self.buffers.insert(
            id,
            GbmBuffer {
                width,
                height,
                format,
                stride,
                size,
                frames,
            },
        );

        Some(id)
    }

    pub fn get_phys_addr(&self, id: GbmBufferId) -> Option<u64> {
        self.buffers.get(&id).map(|buf| buf.frames[0].start_address)
    }

    pub fn get_phys_addrs(&self, id: GbmBufferId) -> Option<(u64, u64, u32)> {
        self.buffers.get(&id).map(|buf| {
            let first = buf.frames[0].start_address;
            (first, buf.size, buf.stride)
        })
    }

    pub fn destroy(&mut self, id: GbmBufferId) {
        if let Some(buf) = self.buffers.remove(&id) {
            let mut guard = crate::boot::FRAME_ALLOCATOR.lock();
            if let Some(allocator) = guard.as_mut() {
                for frame in buf.frames {
                    allocator.deallocate_physical_frame(frame);
                }
            }
        }
    }

    pub fn buffer_size(&self, id: GbmBufferId) -> Option<u64> {
        self.buffers.get(&id).map(|buf| buf.size)
    }
}

lazy_static! {
    pub static ref GBM_MANAGER: Mutex<GbmManager> = Mutex::new(GbmManager::new());
}

pub fn gbm_create(width: u32, height: u32, format: u32) -> Option<GbmBufferId> {
    GBM_MANAGER.lock().create(width, height, format)
}

pub fn gbm_map(id: GbmBufferId) -> Option<u64> {
    GBM_MANAGER.lock().get_phys_addr(id)
}

pub fn gbm_map_full(id: GbmBufferId) -> Option<(u64, u64, u32)> {
    GBM_MANAGER.lock().get_phys_addrs(id)
}

pub fn gbm_destroy(id: GbmBufferId) {
    GBM_MANAGER.lock().destroy(id);
}

pub fn gbm_buffer_size(id: GbmBufferId) -> Option<u64> {
    GBM_MANAGER.lock().buffer_size(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mutex to serialize GBM tests that share the global FRAME_ALLOCATOR.
    static TEST_MUTEX: spin::Mutex<()> = spin::Mutex::new(());

    fn with_init_allocator<F, R>(f: F) -> R
    where
        F: FnOnce() -> R,
    {
        let _guard = TEST_MUTEX.lock();
        use core::mem::size_of;
        use turnix_abi::boot::{
            BootEnvironment, BootInfo, BootLoaderKind, BootMemoryDescriptor, BootMemoryMap,
            MEMORY_TYPE_CONVENTIONAL,
        };

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

        let descriptors = [descriptor(MEMORY_TYPE_CONVENTIONAL, 0x100000, 8192)];

        let boot_info = alloc::boxed::Box::new(BootInfo {
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
            framebuffer: turnix_abi::boot::BootFramebuffer {
                addr: 0,
                size: 0,
                width: 0,
                height: 0,
                pitch: 0,
                format: 0,
            },
            rsdp_addr: 0,
            kaslr_offset: 0,
        });
        let boot_info_ref: &'static BootInfo =
            // Safety: Box::into_raw leaks the allocation; the &'static reference is valid for process lifetime.
            unsafe { &*alloc::boxed::Box::into_raw(boot_info) };

        let frame_alloc = crate::memory::FrameAllocator::new(boot_info_ref);
        *crate::boot::FRAME_ALLOCATOR.lock() = Some(frame_alloc);

        let result = f();

        *crate::boot::FRAME_ALLOCATOR.lock() = None;
        result
    }

    #[test]
    fn test_gbm_create_and_destroy() {
        with_init_allocator(|| {
            let id = gbm_create(1920, 1080, 0).expect("should allocate buffer");
            assert!(id > 0, "buffer ID must be positive");

            let mgr = GBM_MANAGER.lock();
            let buf = mgr.buffers.get(&id).expect("buffer should exist");
            assert_eq!(buf.width, 1920);
            assert_eq!(buf.height, 1080);
            assert_eq!(buf.stride, 1920 * 4);
            assert_eq!(buf.size, 1920 * 1080 * 4);
            let num_pages = buf.size.div_ceil(4096);
            assert_eq!(buf.frames.len() as u64, num_pages);
            drop(mgr);

            gbm_destroy(id);
            let mgr = GBM_MANAGER.lock();
            assert!(!mgr.buffers.contains_key(&id), "buffer should be removed");
        });
    }

    #[test]
    fn test_gbm_map_returns_phys_addr() {
        with_init_allocator(|| {
            let id = gbm_create(640, 480, 0).expect("should allocate buffer");
            let addr = gbm_map(id).expect("should map buffer");
            assert!(addr > 0, "physical address must be non-zero");
            assert_eq!(addr % 4096, 0, "address must be page-aligned");
            gbm_destroy(id);
        });
    }

    #[test]
    fn test_gbm_destroy_frees_pages() {
        with_init_allocator(|| {
            let id = gbm_create(64, 64, 0).expect("should allocate buffer");
            gbm_destroy(id);

            // After destroy, map should return None
            let addr = gbm_map(id);
            assert!(addr.is_none(), "destroyed buffer should not be mappable");
        });
    }

    #[test]
    fn test_gbm_zero_size_returns_none() {
        with_init_allocator(|| {
            let id = gbm_create(0, 0, 0);
            assert!(id.is_none(), "zero-size buffer should not be created");
        });
    }
}
