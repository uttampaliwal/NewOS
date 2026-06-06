use crate::elf;
use crate::memory::aslr;
use crate::memory::paging;
use crate::memory::vma::{VmaSet, Vma, VmaProt, VmaFlags, VmaBacking, VmaError};
use crate::memory::wx;
use x86_64::VirtAddr;
use x86_64::structures::paging::{
    Mapper, OffsetPageTable, Page, PageTable, PageTableFlags, PhysFrame, Size4KiB,
};

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::Mutex;

// ---------------------------------------------------------------------------
// Signal types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignalSet(pub u64);

impl SignalSet {
    pub const fn empty() -> Self {
        SignalSet(0)
    }

    pub fn contains(&self, sig: u8) -> bool {
        self.0 & (1u64 << sig) != 0
    }

    pub fn insert(&mut self, sig: u8) {
        self.0 |= 1u64 << sig;
    }

    pub fn remove(&mut self, sig: u8) {
        self.0 &= !(1u64 << sig);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalAction {
    Default,
    Ignore,
    Handler(u64), // VirtAddr as u64
}

impl Default for SignalAction {
    fn default() -> Self {
        SignalAction::Default
    }
}

// ---------------------------------------------------------------------------
// Process state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockReason {
    WaitingForChild,
    WaitingForIo,
    WaitingForLock,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessState {
    Running,
    Ready,
    Blocked(BlockReason),
    Zombie { exit_code: i32 },
    Stopped,
}

// ---------------------------------------------------------------------------
// ProcessId
// ---------------------------------------------------------------------------

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

pub const DEFAULT_MMAP_BASE: u64 = 0x0000_2000_0000_0000;

// ---------------------------------------------------------------------------
// Global process table
// ---------------------------------------------------------------------------

lazy_static::lazy_static! {
    pub static ref PROCESS_TABLE: Mutex<BTreeMap<ProcessId, Arc<Mutex<ProcessControlBlock>>>> =
        Mutex::new(BTreeMap::new());
}

/// Reparent an orphaned process to init (PID 1).
///
/// Called when a parent exits before its child. Looks up `orphan_pid` in
/// `PROCESS_TABLE` and sets its `ppid` to `ProcessId(1)`.
pub fn reparent_to_init(orphan_pid: ProcessId) {
    let table = PROCESS_TABLE.lock();
    if let Some(pcb) = table.get(&orphan_pid) {
        pcb.lock().ppid = ProcessId(1);
        crate::serial::println!("[process] reparented PID {:?} to init", orphan_pid);
    }
}

// ---------------------------------------------------------------------------
// ProcessControlBlock  (was ProcessInner)
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct ProcessControlBlock {
    pub id: ProcessId,
    pub ppid: ProcessId,
    pub state: ProcessState,
    pub pml4_frame: PhysFrame<Size4KiB>,
    pub entry_point: VirtAddr,
    pub stack_top: VirtAddr,
    pub threads: Vec<TaskId>,
    pub vma_set: VmaSet,
    pub mmap_next_addr: VirtAddr,
    pub aslr_base: VirtAddr,
    /// Per-process file-descriptor table (1024 entries).
    pub fd_table: [Option<crate::vfs::FileDescriptor>; 1024],
    pub signal_mask: SignalSet,
    pub signal_handlers: [SignalAction; 64],
    pub pending_signals: SignalSet,
}

// ---------------------------------------------------------------------------
// Process wrapper (Arc<Mutex<ProcessControlBlock>>)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Process {
    pub inner: Arc<Mutex<ProcessControlBlock>>,
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

    pub fn with_vma_set<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&VmaSet) -> R,
    {
        let inner = self.inner.lock();
        f(&inner.vma_set)
    }

    pub fn with_vma_set_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut VmaSet) -> R,
    {
        let mut inner = self.inner.lock();
        f(&mut inner.vma_set)
    }

    pub fn mmap_anon(
        &self,
        addr: Option<VirtAddr>,
        length: u64,
        prot: VmaProt,
        flags: VmaFlags,
    ) -> Result<VirtAddr, VmaError> {
        let mut inner = self.inner.lock();
        let page_aligned_len = length.max(4096).next_multiple_of(4096);
        let actual_addr = match addr {
            Some(a) => a,
            None => {
                let a = inner.mmap_next_addr;
                inner.mmap_next_addr = VirtAddr::new(
                    inner
                        .mmap_next_addr
                        .as_u64()
                        .checked_add(page_aligned_len)
                        .unwrap_or(u64::MAX),
                );
                a
            }
        };
        let vma = Vma {
            start: actual_addr,
            end: VirtAddr::new(
                actual_addr
                    .as_u64()
                    .checked_add(page_aligned_len)
                    .unwrap_or(u64::MAX),
            ),
            prot,
            backing: VmaBacking::Anonymous,
            flags,
        };
        inner.vma_set.insert(vma)?;
        Ok(actual_addr)
    }

    pub fn munmap_range(&self, addr: VirtAddr, _length: u64) -> Result<(), VmaError> {
        let mut inner = self.inner.lock();
        inner.vma_set.remove(addr).ok_or(VmaError::Conflict)?;
        Ok(())
    }

    pub fn kernel_process() -> Self {
        lazy_static::lazy_static! {
            static ref KERNEL_PROC: Process = {
                let (pml4, _) = x86_64::registers::control::Cr3::read();
                Process {
                    inner: Arc::new(Mutex::new(ProcessControlBlock {
                        id: ProcessId(0),
                        ppid: ProcessId(0),
                        state: ProcessState::Running,
                        pml4_frame: pml4,
                        entry_point: VirtAddr::zero(),
                        stack_top: VirtAddr::zero(),
                        threads: Vec::new(),
                        vma_set: VmaSet::new(),
                        mmap_next_addr: VirtAddr::new(DEFAULT_MMAP_BASE),
                        aslr_base: VirtAddr::zero(),
                        fd_table: core::array::from_fn(|_| None),
                        signal_mask: SignalSet::empty(),
                        signal_handlers: [SignalAction::Default; 64],
                        pending_signals: SignalSet::empty(),
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
        let aslr_base = aslr::randomise_load_base(&header);

        let pml4_frame = paging::create_process_pml4(frame_allocator, physical_memory_offset);
        let stack_size: u64 = 4096 * 8;
        let stack_base = aslr::randomise_stack_base();
        let stack_start = stack_base;
        let stack_top = stack_start + stack_size;

        let entry_point = aslr_base + header.entry;
        let mmap_base = aslr::randomise_heap_base();

        let inner = ProcessControlBlock {
            id: ProcessId::new(),
            ppid: ProcessId(0),
            state: ProcessState::Ready,
            pml4_frame,
            entry_point,
            stack_top,
            threads: Vec::new(),
            vma_set: VmaSet::new(),
            mmap_next_addr: mmap_base,
            aslr_base,
            fd_table: core::array::from_fn(|_| None),
            signal_mask: SignalSet::empty(),
            signal_handlers: [SignalAction::Default; 64],
            pending_signals: SignalSet::empty(),
        };

        let process = Self {
            inner: Arc::new(Mutex::new(inner)),
        };

        unsafe {
            // Map User Stack (read-write, non-executable)
            process.map_user_region(
                stack_start,
                stack_size,
                PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE,
                frame_allocator,
                physical_memory_offset,
            );

            // Map ELF Segments
            let pml4_ptr = (physical_memory_offset + pml4_frame.start_address().as_u64())
                .as_mut_ptr::<PageTable>();
            let process_mapper = OffsetPageTable::new(&mut *pml4_ptr, physical_memory_offset);

            // Track executable segments for post-load write revocation
            let mut exec_segments: Vec<(VirtAddr, u64)> = Vec::new();

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

                    let virt_start = aslr_base + ph.virtual_address;

                    // Map as writable + non-executable during load (req 14.4:
                    // writable-only, never W+X, even transiently). Executable
                    // segments will be promoted to R-X after data copy.
                    let map_flags = PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE;
                    let is_exec = (ph.flags & elf::PF_X) != 0;
                    if is_exec {
                        exec_segments.push((virt_start, ph.memory_size));
                    }

                    process.map_user_region(
                        virt_start,
                        ph.memory_size,
                        map_flags,
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

            // Finalise executable segments: strip WRITABLE and clear NO_EXECUTE
            // so they become R-X before the entry point runs (req 14.4).
            for (seg_start, seg_size) in &exec_segments {
                wx::clear_write_and_allow_exec(
                    pml4_frame,
                    physical_memory_offset,
                    *seg_start,
                    *seg_size,
                );
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

        // Clone user address space with Copy-on-Write
        paging::clone_user_mappings_cow(
            self.pml4_frame(),
            pml4_frame,
            frame_allocator,
            physical_memory_offset,
        );

        let parent = self.inner.lock();
        
        // Clone fd table
        let fd_table: [Option<crate::vfs::FileDescriptor>; 1024] = core::array::from_fn(|i| parent.fd_table[i].clone());

        let new_inner = ProcessControlBlock {
            id: ProcessId::new(),
            ppid: parent.id,
            state: ProcessState::Ready,
            pml4_frame,
            entry_point: parent.entry_point,
            stack_top: aslr::randomise_stack_base(),
            threads: Vec::new(),
            vma_set: parent.vma_set.clone(),
            mmap_next_addr: aslr::randomise_heap_base(),
            aslr_base: parent.aslr_base,
            fd_table,
            signal_mask: parent.signal_mask,
            signal_handlers: parent.signal_handlers,
            pending_signals: SignalSet::empty(),
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
            // Apply W^X enforcement: strip WRITABLE if both WRITABLE and executable
            let mut enforced_flags = extra_flags;
            wx::enforce_wx_on_flags(&mut enforced_flags);
            let flags =
                PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE | enforced_flags;
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

            // Ensure the PML4 entry itself has the USER_ACCESSIBLE bit for lower-half addresses
            let p4_idx = virt_start.p4_index();
            let pml4 = &mut *pml4_ptr;
            let f4 = pml4[p4_idx].flags();
            pml4[p4_idx].set_flags(f4 | PageTableFlags::USER_ACCESSIBLE);
        }
    }

    /// Maps a kernel-only region into this process's address space.
    pub unsafe fn map_kernel_region(
        &self,
        virt_start: VirtAddr,
        size: u64,
        flags: PageTableFlags,
        frame_allocator: &mut impl x86_64::structures::paging::FrameAllocator<Size4KiB>,
        physical_memory_offset: VirtAddr,
    ) {
        let pml4_frame = self.pml4_frame();
        let pml4_ptr = (physical_memory_offset + pml4_frame.start_address().as_u64())
            .as_mut_ptr::<PageTable>();
        unsafe {
            let mut process_mapper = OffsetPageTable::new(&mut *pml4_ptr, physical_memory_offset);
            let pages = Page::<Size4KiB>::range_inclusive(
                Page::containing_address(virt_start),
                Page::containing_address(virt_start + size - 1u64),
            );

            for page in pages {
                let frame = frame_allocator.allocate_frame().expect("out of memory");
                process_mapper
                    .map_to(
                        page,
                        frame,
                        flags | PageTableFlags::PRESENT,
                        frame_allocator,
                    )
                    .expect("failed to map kernel page")
                    .ignore();
            }
        }
    }
}
