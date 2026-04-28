use crate::memory::paging;
use crate::elf;
use x86_64::VirtAddr;
use x86_64::structures::paging::{PhysFrame, Size4KiB, Page, PageTableFlags, Mapper, PageTable, OffsetPageTable};

#[derive(Debug)]
pub struct Process {
    pub pml4_frame: PhysFrame<Size4KiB>,
    pub entry_point: VirtAddr,
    pub stack_top: VirtAddr,
}

impl Process {
    pub fn new_from_elf(
        elf_data: &[u8],
        frame_allocator: &mut impl x86_64::structures::paging::FrameAllocator<Size4KiB>,
        physical_memory_offset: VirtAddr,
    ) -> Result<Self, elf::ParseError> {
        // 1. Parse ELF
        let header = elf::parse_header(elf_data)?;

        // 2. Create a new address space (clones higher-half kernel as Supervisor-only)
        let pml4_frame = paging::create_process_pml4(frame_allocator, physical_memory_offset);

        // 3. Define User Stack
        let stack_start = VirtAddr::new(0x0000_7000_0000_0000);
        let stack_size: u64 = 4096 * 8; // 32 KB stack
        let stack_top = stack_start + stack_size;

        let mut process = Self {
            pml4_frame,
            entry_point: VirtAddr::new(header.entry),
            stack_top,
        };

        unsafe {
            // 4. Map User Stack (marked USER)
            process.map_user_region(stack_start, stack_size, PageTableFlags::WRITABLE, frame_allocator, physical_memory_offset);

            // Access the new PML4
            let pml4_ptr = (physical_memory_offset + pml4_frame.start_address().as_u64()).as_mut_ptr::<PageTable>();
            let mut process_mapper = OffsetPageTable::new(&mut *pml4_ptr, physical_memory_offset);

            // 5. Map and Copy ELF Segments
            for i in 0..header.program_header_count {
                if let Some(ph) = elf::parse_program_header(elf_data, header, i)? {
                    if ph.memory_size == 0 { continue; }

                    let virt_start = VirtAddr::new(ph.virtual_address);
                    let mut extra_flags = PageTableFlags::empty();
                    if ph.flags & elf::PF_W != 0 { extra_flags |= PageTableFlags::WRITABLE; }
                    if ph.flags & elf::PF_X == 0 { extra_flags |= PageTableFlags::NO_EXECUTE; }

                    process.map_user_region(virt_start, ph.memory_size, extra_flags, frame_allocator, physical_memory_offset);

                    // Copy data from ELF to newly mapped user memory
                    use x86_64::structures::paging::Translate;

                    let mut offset = 0u64;
                    while offset < ph.file_size {
                        let chunk_virt = virt_start + offset;
                        let chunk_phys = process_mapper.translate_addr(chunk_virt).expect("user page not mapped during ELF load");
                        let dest_ptr = (physical_memory_offset + chunk_phys.as_u64()).as_mut_ptr::<u8>();

                        let remaining_in_segment = ph.file_size - offset;
                        let remaining_in_page = 4096 - (chunk_virt.as_u64() % 4096);
                        let copy_size = remaining_in_segment.min(remaining_in_page);

                        let src_ptr = &elf_data[ph.file_offset as usize + offset as usize] as *const u8;
                        core::ptr::copy_nonoverlapping(src_ptr, dest_ptr, copy_size as usize);

                        offset += copy_size;
                    }

                    // Zero out remaining memory size (BSS)
                    while offset < ph.memory_size {
                        let chunk_virt = virt_start + offset;
                        let chunk_phys = process_mapper.translate_addr(chunk_virt).expect("user page not mapped during ELF load BSS");
                        let dest_ptr = (physical_memory_offset + chunk_phys.as_u64()).as_mut_ptr::<u8>();

                        let remaining_in_segment = ph.memory_size - offset;
                        let remaining_in_page = 4096 - (chunk_virt.as_u64() % 4096);
                        let copy_size = remaining_in_segment.min(remaining_in_page);

                        core::ptr::write_bytes(dest_ptr, 0, copy_size as usize);
                        offset += copy_size;
                    }
                }
            }
        }

        Ok(process)
    }

    pub fn new(
        entry_point_fn: extern "sysv64" fn(),
        frame_allocator: &mut impl x86_64::structures::paging::FrameAllocator<Size4KiB>,
        physical_memory_offset: VirtAddr,
    ) -> Self {
        // 1. Create a new address space (clones higher-half kernel as Supervisor-only)
        let pml4_frame = paging::create_process_pml4(frame_allocator, physical_memory_offset);
        
        // 2. Define User Space Layout
        let stack_start = VirtAddr::new(0x0000_7000_0000_0000);
        let stack_size: u64 = 4096 * 4;
        let stack_top = stack_start + stack_size;
        let code_start = VirtAddr::new(0x0000_0000_0040_0000);
        let code_size: u64 = 4096;

        let mut process = Self {
            pml4_frame,
            entry_point: code_start,
            stack_top,
        };

        unsafe {
            // 3. Map User Stack (marked USER)
            process.map_user_region(stack_start, stack_size, PageTableFlags::WRITABLE, frame_allocator, physical_memory_offset);

            // 4. Map and Copy Code (marked USER)
            process.map_user_region(code_start, code_size, PageTableFlags::empty(), frame_allocator, physical_memory_offset);

            // Access the new PML4 to copy the code
            let pml4_ptr = (physical_memory_offset + pml4_frame.start_address().as_u64()).as_mut_ptr::<PageTable>();
            let process_mapper = OffsetPageTable::new(&mut *pml4_ptr, physical_memory_offset);

            use x86_64::structures::paging::Translate;
            let phys_code = process_mapper.translate_addr(code_start).expect("failed to translate user code address");
            let dest_ptr = (physical_memory_offset + phys_code.as_u64()).as_mut_ptr::<u8>();

            // Copy the test function bytes to the user space page
            core::ptr::copy_nonoverlapping(entry_point_fn as *const u8, dest_ptr, 1024);
        }

        process
    }
    /// Maps a region of memory as user-accessible in the lower-half address space.
    pub unsafe fn map_user_region(
        &mut self,
        virt_start: VirtAddr,
        size: u64,
        extra_flags: PageTableFlags,
        frame_allocator: &mut impl x86_64::structures::paging::FrameAllocator<Size4KiB>,
        physical_memory_offset: VirtAddr,
    ) {
        let pml4_ptr = (physical_memory_offset + self.pml4_frame.start_address().as_u64()).as_mut_ptr::<PageTable>();
        
        unsafe {
            let mut process_mapper = OffsetPageTable::new(&mut *pml4_ptr, physical_memory_offset);
            let flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE | extra_flags;
            
            let pages = Page::<Size4KiB>::range_inclusive(
                Page::containing_address(virt_start),
                Page::containing_address(virt_start + size - 1u64),
            );

            for page in pages {
                let frame = frame_allocator.allocate_frame().expect("out of memory");
                process_mapper.map_to(page, frame, flags, frame_allocator).expect("failed to map user page").ignore();
            }

            // Ensure the PML4 entry itself has the USER_ACCESSIBLE bit for lower-half addresses
            let p4_idx = virt_start.p4_index();
            let pml4 = &mut *pml4_ptr;
            let f4 = pml4[p4_idx].flags();
            pml4[p4_idx].set_flags(f4 | PageTableFlags::USER_ACCESSIBLE);
        }
    }

    /// Maps a kernel-only region into this process's address space.
    pub unsafe fn map_kernel_region(
        &mut self,
        virt_start: VirtAddr,
        size: u64,
        flags: PageTableFlags,
        frame_allocator: &mut impl x86_64::structures::paging::FrameAllocator<Size4KiB>,
        physical_memory_offset: VirtAddr,
    ) {
        let pml4_ptr = (physical_memory_offset + self.pml4_frame.start_address().as_u64()).as_mut_ptr::<PageTable>();
        unsafe {
            let mut process_mapper = OffsetPageTable::new(&mut *pml4_ptr, physical_memory_offset);
            let pages = Page::<Size4KiB>::range_inclusive(
                Page::containing_address(virt_start),
                Page::containing_address(virt_start + size - 1u64),
            );

            for page in pages {
                let frame = frame_allocator.allocate_frame().expect("out of memory");
                process_mapper.map_to(page, frame, flags | PageTableFlags::PRESENT, frame_allocator).expect("failed to map kernel page").ignore();
            }
        }
    }

    pub fn kernel_process() -> Self {
        let (pml4_frame, _) = x86_64::registers::control::Cr3::read();
        Self {
            pml4_frame,
            entry_point: VirtAddr::new(0), // Not used for kernel tasks
            stack_top: VirtAddr::new(0),    // Not used for kernel tasks
        }
    }
}
