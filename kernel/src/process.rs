use alloc::vec::Vec;
use x86_64::VirtAddr;
use x86_64::structures::paging::{Page, PageTableFlags, Size4KiB, Mapper, FrameAllocator as X86FrameAllocator, PhysFrame, PageSize};
use crate::elf;

const PAGE_SIZE: u64 = 4096;

pub struct UserProcess {
    pub entry: u64,
    pub stack_addr: u64,
    pub pages_mapped: Vec<PhysFrame<Size4KiB>>,
}

impl UserProcess {
    pub fn load_elf(
        data: &[u8],
        _mapper: &mut impl Mapper<Size4KiB>,
        frame_allocator: &mut impl X86FrameAllocator<Size4KiB>,
    ) -> Result<Self, ()> {
        let header = elf::parse_header(data).map_err(|_| ())?;

        let mut user_pages = Vec::new();

        for i in 0..header.program_header_count {
            if let Ok(Some(ph)) = elf::parse_program_header(data, header, i) {
                if ph.memory_size == 0 {
                    continue;
                }

                let pages_needed = ((ph.memory_size as u64) + PAGE_SIZE - 1) / PAGE_SIZE;
                for _j in 0..pages_needed {
                    let frame = match frame_allocator.allocate_frame() {
                        Some(f) => f,
                        None => return Err(()),
                    };
                    user_pages.push(frame);
                }
            }
        }

        for _i in 0..16 {
            let frame = frame_allocator.allocate_frame().ok_or(())?;
            user_pages.push(frame);
        }

        Ok(Self {
            entry: header.entry,
            stack_addr: 0x7fff_f000,
            pages_mapped: user_pages,
        })
    }
}