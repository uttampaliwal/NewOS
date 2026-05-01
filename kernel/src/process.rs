use crate::elf;
use crate::memory::paging;
use x86_64::VirtAddr;
use x86_64::structures::paging::{
    Mapper, OffsetPageTable, Page, PageTable, PageTableFlags, PhysFrame, Size4KiB,
};

use alloc::sync::Arc;
use spin::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ProcessId(pub usize);

impl ProcessId {
    pub fn new() -> Self {
        use core::sync::atomic::{AtomicUsize, Ordering};
        static NEXT_ID: AtomicUsize = AtomicUsize::new(1);
        ProcessId(NEXT_ID.fetch_add(1, Ordering::Relaxed))
    }
}

use crate::task::TaskId;
use alloc::vec::Vec;

#[derive(Debug)]
pub struct ProcessInner {
    pub id: ProcessId,
    pub pml4_frame: PhysFrame<Size4KiB>,
    pub entry_point: VirtAddr,
    pub stack_top: VirtAddr,
    pub threads: Vec<TaskId>,
}

#[derive(Debug, Clone)]
pub struct Process {
    inner: Arc<Mutex<ProcessInner>>,
}

impl Process {
    pub fn id(&self) -> ProcessId {
        self.inner.lock().id
    }

    pub fn pml4_frame(&self) -> PhysFrame<Size4KiB> {
        self.inner.lock().pml4_frame
    }

    pub fn entry_point(&self) -> VirtAddr {
        self.inner.lock().entry_point
    }

    pub fn stack_top(&self) -> VirtAddr {
        self.inner.lock().stack_top
    }

    pub fn add_thread(&self, thread_id: TaskId) {
        self.inner.lock().threads.push(thread_id);
    }

    pub fn threads(&self) -> Vec<TaskId> {
        self.inner.lock().threads.clone()
    }

    pub fn kernel_process() -> Self {
        lazy_static::lazy_static! {
            static ref KERNEL_PROC: Process = {
                let (pml4, _) = x86_64::registers::control::Cr3::read();
                Process {
                    inner: Arc::new(Mutex::new(ProcessInner {
                        id: ProcessId(0),
                        pml4_frame: pml4,
                        entry_point: VirtAddr::zero(),
                        stack_top: VirtAddr::zero(),
                        threads: Vec::new(),
                    }))
                }
            };
        }
        KERNEL_PROC.clone()
    }

    pub fn new_from_elf(
        elf_data: &[u8],
        frame_allocator: &mut impl x86_64::structures::paging::FrameAllocator<Size4KiB>,
        physical_memory_offset: VirtAddr,
    ) -> Result<Self, elf::ParseError> {
        let header = elf::parse_header(elf_data)?;
        let pml4_frame = paging::create_process_pml4(frame_allocator, physical_memory_offset);
        let stack_start = VirtAddr::new(0x0000_7000_0000_0000);
        let stack_size: u64 = 4096 * 8;
        let stack_top = stack_start + stack_size;

        let inner = ProcessInner {
            id: ProcessId::new(),
            pml4_frame,
            entry_point: VirtAddr::new(header.entry),
            stack_top,
            threads: Vec::new(),
        };

        let process = Self {
            inner: Arc::new(Mutex::new(inner)),
        };

        unsafe {
            // Map User Stack
            process.map_user_region(
                stack_start,
                stack_size,
                PageTableFlags::WRITABLE,
                frame_allocator,
                physical_memory_offset,
            );

            // Map ELF Segments
            let pml4_ptr = (physical_memory_offset + pml4_frame.start_address().as_u64())
                .as_mut_ptr::<PageTable>();
            let process_mapper = OffsetPageTable::new(&mut *pml4_ptr, physical_memory_offset);

            for i in 0..header.program_header_count {
                if let Some(ph) = elf::parse_program_header(elf_data, header, i)? {
                    if ph.memory_size == 0 {
                        continue;
                    }
                    let file_start = ph.file_offset as usize;
                    let file_size = ph.file_size as usize;
                    let file_end = file_start
                        .checked_add(file_size)
                        .ok_or(elf::ParseError::ProgramHeaderOutOfBounds)?;
                    if file_end > elf_data.len() {
                        return Err(elf::ParseError::ProgramHeaderOutOfBounds);
                    }

                    let virt_start = VirtAddr::new(ph.virtual_address);
                    let mut flags = PageTableFlags::empty();
                    if ph.flags & elf::PF_W != 0 {
                        flags |= PageTableFlags::WRITABLE;
                    }
                    if ph.flags & elf::PF_X == 0 {
                        flags |= PageTableFlags::NO_EXECUTE;
                    }

                    process.map_user_region(
                        virt_start,
                        ph.memory_size,
                        flags,
                        frame_allocator,
                        physical_memory_offset,
                    );

                    // Copy data
                    use x86_64::structures::paging::Translate;
                    let mut offset = 0u64;
                    while offset < ph.file_size {
                        let chunk_virt = virt_start + offset;
                        let chunk_phys =
                            process_mapper.translate_addr(chunk_virt).expect("ELF map");
                        let dest_ptr =
                            (physical_memory_offset + chunk_phys.as_u64()).as_mut_ptr::<u8>();
                        let copy_size =
                            (ph.file_size - offset).min(4096 - (chunk_virt.as_u64() % 4096));
                        core::ptr::copy_nonoverlapping(
                            &elf_data[ph.file_offset as usize + offset as usize],
                            dest_ptr,
                            copy_size as usize,
                        );
                        offset += copy_size;
                    }
                    // BSS
                    while offset < ph.memory_size {
                        let chunk_virt = virt_start + offset;
                        let chunk_phys =
                            process_mapper.translate_addr(chunk_virt).expect("BSS map");
                        let dest_ptr =
                            (physical_memory_offset + chunk_phys.as_u64()).as_mut_ptr::<u8>();
                        let copy_size =
                            (ph.memory_size - offset).min(4096 - (chunk_virt.as_u64() % 4096));
                        core::ptr::write_bytes(dest_ptr, 0, copy_size as usize);
                        offset += copy_size;
                    }
                }
            }
        }
        Ok(process)
    }

    pub fn fork(
        &self,
        frame_allocator: &mut impl x86_64::structures::paging::FrameAllocator<Size4KiB>,
        physical_memory_offset: VirtAddr,
    ) -> Self {
        let pml4_frame = paging::create_process_pml4(frame_allocator, physical_memory_offset);

        // Deep copy user-mode address space
        paging::clone_user_mappings(
            self.pml4_frame(),
            pml4_frame,
            frame_allocator,
            physical_memory_offset,
        );

        let inner = self.inner.lock();
        let new_inner = ProcessInner {
            id: ProcessId::new(),
            pml4_frame,
            entry_point: inner.entry_point,
            stack_top: inner.stack_top,
            threads: Vec::new(),
        };

        Self {
            inner: Arc::new(Mutex::new(new_inner)),
        }
    }

    pub unsafe fn map_user_region(
        &self,
        virt_start: VirtAddr,
        size: u64,
        extra_flags: PageTableFlags,
        frame_allocator: &mut impl x86_64::structures::paging::FrameAllocator<Size4KiB>,
        physical_memory_offset: VirtAddr,
    ) {
        let pml4_frame = self.pml4_frame();
        let pml4_ptr = (physical_memory_offset + pml4_frame.start_address().as_u64())
            .as_mut_ptr::<PageTable>();

        unsafe {
            let mut process_mapper = OffsetPageTable::new(&mut *pml4_ptr, physical_memory_offset);
            let flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE | extra_flags;
            let pages = Page::<Size4KiB>::range_inclusive(
                Page::containing_address(virt_start),
                Page::containing_address(virt_start + size - 1u64),
            );
            for page in pages {
                use x86_64::structures::paging::Translate;
                if process_mapper
                    .translate_addr(page.start_address())
                    .is_none()
                {
                    let frame = frame_allocator.allocate_frame().expect("out of memory");
                    process_mapper
                        .map_to(page, frame, flags, frame_allocator)
                        .expect("map")
                        .ignore();
                }
            }
        }
    }
}
