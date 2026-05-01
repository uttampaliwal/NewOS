use alloc::vec::Vec;
use x86_64::VirtAddr;
use x86_64::structures::paging::{
    FrameAllocator as X86FrameAllocator, Mapper, Page, PageSize, PageTableFlags, PhysFrame,
    Size4KiB,
};

pub const USER_STACK_BASE: u64 = 0x7fff_f000;
pub const USER_STACK_SIZE_PAGES: usize = 16;

pub struct UserSpace {
    pub page_tables: Vec<PhysFrame<Size4KiB>>,
}

impl UserSpace {
    pub fn new() -> Self {
        Self {
            page_tables: Vec::new(),
        }
    }

    pub fn alloc_user_stack(
        &mut self,
        mapper: &mut impl Mapper<Size4KiB>,
        frame_allocator: &mut impl X86FrameAllocator<Size4KiB>,
    ) -> Result<u64, ()> {
        let page_size = Size4KiB::SIZE as u64;
        let stack_base = USER_STACK_BASE;
        for i in 0..USER_STACK_SIZE_PAGES {
            let frame = frame_allocator.allocate_frame().ok_or(())?;
            let virt = VirtAddr::new(stack_base - (i as u64 * page_size));
            let page = Page::from_start_address(virt).map_err(|_| ())?;
            let flags = PageTableFlags::PRESENT
                | PageTableFlags::WRITABLE
                | PageTableFlags::USER_ACCESSIBLE;
            unsafe {
                let _ = mapper
                    .map_to(page, frame, flags, frame_allocator)
                    .map_err(|_| ())?;
            }
            self.page_tables.push(frame);
        }
        Ok(stack_base)
    }

    pub fn switch_to_user(&self, user_pml4: PhysFrame<Size4KiB>) {
        use x86_64::registers::control::Cr3;
        unsafe {
            Cr3::write(user_pml4, x86_64::registers::control::Cr3Flags::empty());
        }
    }
}

impl Default for UserSpace {
    fn default() -> Self {
        Self::new()
    }
}
