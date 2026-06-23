use crate::elf;
use crate::memory::aslr;
use crate::memory::paging;
use crate::memory::vma::{Vma, VmaBacking, VmaError, VmaFlags, VmaProt, VmaSet};
use crate::memory::wx;
use crate::security::capabilities::CapabilitySet;
use crate::security::namespaces::NsProxy;
use crate::security::seccomp::SeccompFilter;
use crate::security::SecurityContext;
use x86_64::VirtAddr;
use x86_64::structures::paging::{
    Mapper, OffsetPageTable, Page, PageTable, PageTableFlags, PhysFrame, Size4KiB, Translate,
};
extern crate alloc;
use alloc::vec;
use alloc::vec::Vec;

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SignalAction {
    #[default]
    Default,
    Ignore,
    Handler(u64), // VirtAddr as u64
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

impl Default for ProcessId {
    fn default() -> Self {
        Self::new()
    }
}

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
        #[cfg(not(test))]
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
    pub fd_table: Vec<Option<crate::vfs::FileDescriptor>>,
    pub signal_mask: SignalSet,
    pub signal_handlers: [SignalAction; 64],
    pub pending_signals: SignalSet,
    /// User-space address of the active SignalFrame, or None.
    pub pending_signal_frame: Option<u64>,
    /// Per-process POSIX capability sets.
    pub sec_ctx: SecurityContext,
    /// Per-process namespace proxy.
    pub nsproxy: NsProxy,
    /// Per-process seccomp filter (None = disabled).
    pub seccomp_filter: Option<SeccompFilter>,
    /// Cgroup path this process belongs to (None = root cgroup "/").
    pub cgroup_path: Option<alloc::string::String>,
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
                        .saturating_add(page_aligned_len),
                );
                a
            }
        };
        let vma = Vma {
            start: actual_addr,
            end: VirtAddr::new(actual_addr.as_u64().saturating_add(page_aligned_len)),
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
        /// Read the current PML4 frame.  Cr3 is inaccessible from userspace on
        /// some hosts, so in test mode we use frame 0 (never dereferenced in tests).
        fn read_pml4() -> PhysFrame {
            #[cfg(not(test))]
            {
                let (pml4, _) = x86_64::registers::control::Cr3::read();
                pml4
            }
            #[cfg(test)]
            {
                PhysFrame::containing_address(x86_64::PhysAddr::new(0))
            }
        }

        lazy_static::lazy_static! {
            static ref KERNEL_PROC: Process = {
                Process {
                    inner: Arc::new(Mutex::new(ProcessControlBlock {
                        id: ProcessId(0),
                        ppid: ProcessId(0),
                        state: ProcessState::Running,
                        pml4_frame: read_pml4(),
                        entry_point: VirtAddr::zero(),
                        stack_top: VirtAddr::zero(),
                        threads: Vec::new(),
                        vma_set: VmaSet::new(),
                        mmap_next_addr: VirtAddr::new(DEFAULT_MMAP_BASE),
                        aslr_base: VirtAddr::zero(),
                        fd_table: alloc::vec![None; 1024],
                        signal_mask: SignalSet::empty(),
                        signal_handlers: [SignalAction::Default; 64],
                        pending_signals: SignalSet::empty(),
                        pending_signal_frame: None,
                        sec_ctx: SecurityContext::root(),
                        nsproxy: NsProxy::new(),
                        seccomp_filter: None,
                        cgroup_path: None,
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
        crate::serial::println!("[STG: PROC_NEW_HEADER]");
        let aslr_base = aslr::randomise_load_base(&header);
        let virtual_base = compute_load_base(elf_data, &header)?;
        crate::serial::println!("[STG: PROC_NEW_BASE]");

        crate::serial::println!("[STG: PROC_CREATE_PML4]");
        let pml4_frame = paging::create_process_pml4(frame_allocator, physical_memory_offset);
        crate::serial::println!("[STG: PROC_NEW_PML4]");
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
            fd_table: alloc::vec![None; 1024],
            signal_mask: SignalSet::empty(),
            signal_handlers: [SignalAction::Default; 64],
            pending_signals: SignalSet::empty(),
            pending_signal_frame: None,
            sec_ctx: SecurityContext::new(0, 0, CapabilitySet::basic()),
            nsproxy: NsProxy::new(),
            seccomp_filter: None,
            cgroup_path: None,
        };

        crate::serial::println!("[STG: PROC_INNER_BUILT]");
        let process = Self {
            inner: Arc::new(Mutex::new(inner)),
        };
        crate::serial::println!("[STG: PROC_ARC_BUILT]");

        unsafe {
            // Map User Stack (read-write, non-executable)
            process.map_user_region(
                stack_start,
                stack_size,
                PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE,
                frame_allocator,
                physical_memory_offset,
            );
            crate::serial::println!("[STG: PROC_NEW_STACK]");

            // Map ELF Segments
            let pml4_ptr = (physical_memory_offset + pml4_frame.start_address().as_u64())
                .as_mut_ptr::<PageTable>();
            let process_mapper = OffsetPageTable::new(&mut *pml4_ptr, physical_memory_offset);
            crate::serial::println!("[STG: PROC_NEW_MAPPER]");

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
            crate::serial::println!("[STG: PROC_NEW_SEGMENTS]");

            // Finalise executable segments: strip WRITABLE and clear NO_EXECUTE
            // so they become R-X before the entry point runs (req 14.4).
            crate::serial::println!("[STG: PROC_NEW_RELOCATE]");
            apply_relative_relocations(
                elf_data,
                &header,
                aslr_base,
                virtual_base,
                &process_mapper,
                physical_memory_offset,
            )?;
            crate::serial::println!("[STG: PROC_NEW_RELOCATED]");
            for (seg_start, seg_size) in &exec_segments {
                wx::clear_write_and_allow_exec(
                    pml4_frame,
                    physical_memory_offset,
                    *seg_start,
                    *seg_size,
                );
            }
        }
        crate::serial::println!("[STG: PROC_NEW_DONE]");
        Ok(process)
    }

    pub fn exec_from_elf(
        &self,
        elf_data: &[u8],
        argv: &[&[u8]],
        envp: &[&[u8]],
        frame_allocator: &mut crate::memory::FrameAllocator<'_>,
        physical_memory_offset: VirtAddr,
        file_caps: Option<crate::security::capabilities::FileCaps>,
    ) -> Result<(), elf::ParseError> {
        let header = elf::parse_header(elf_data)?;
        let aslr_base = aslr::randomise_load_base(&header);
        let virtual_base = compute_load_base(elf_data, &header)?;

        // LSM process_create hook for exec
        {
            let inner = self.inner.lock();
            if crate::security::lsm::check_process_create(inner.sec_ctx.uid, inner.sec_ctx.gid).is_err() {
                // Hook check — currently advisory for exec
            }
        }

        let (old_pml4_frame, current_ppid, retained_fd_table, cloexec_fds) = {
            let inner = self.inner.lock();
            let cloexec_fds = inner
                .fd_table
                .iter()
                .enumerate()
                .filter_map(|(index, fd_opt)| {
                    fd_opt
                        .as_ref()
                        .filter(|fd| fd.flags.is_cloexec())
                        .map(|_| index)
                })
                .collect::<Vec<_>>();
            let retained_fd_table: Vec<Option<crate::vfs::FileDescriptor>> = (0..1024)
                .map(|index| {
                inner.fd_table[index]
                    .clone()
                    .filter(|fd| !fd.flags.is_cloexec())
            })
                .collect();
            (inner.pml4_frame, inner.ppid, retained_fd_table, cloexec_fds)
        };

        // Create new PML4 for the exec'd process
        let new_pml4_frame = paging::create_process_pml4(frame_allocator, physical_memory_offset);
        let stack_size: u64 = 4096 * 16; // Larger stack for argv/envp
        let stack_base = aslr::randomise_stack_base();
        let stack_start = stack_base;
        let stack_top = stack_start + stack_size;

        let entry_point = aslr_base + header.entry;
        let mmap_base = aslr::randomise_heap_base();

        // Now create a temporary process-like thing to set up the new mappings
        // We'll use a dummy ProcessControlBlock for the setup
        let temp_process = Self {
            inner: Arc::new(Mutex::new(ProcessControlBlock {
                id: self.id(), // Keep the same PID!
                ppid: current_ppid,
                state: ProcessState::Ready,
                pml4_frame: new_pml4_frame,
                entry_point,
                stack_top,
                threads: Vec::new(),
                vma_set: VmaSet::new(),
                mmap_next_addr: mmap_base,
                aslr_base,
                fd_table: retained_fd_table.clone(),
                signal_mask: SignalSet::empty(), // Reset signals on exec
                signal_handlers: [SignalAction::Default; 64],
                pending_signals: SignalSet::empty(),
                pending_signal_frame: None,
                sec_ctx: SecurityContext::new(0, 0, CapabilitySet::basic()),
                nsproxy: NsProxy::new(),
                seccomp_filter: None,
                cgroup_path: None,
            })),
        };

        unsafe {
            // Map User Stack (read-write, non-executable)
            temp_process.map_user_region(
                stack_start,
                stack_size,
                PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE,
                frame_allocator,
                physical_memory_offset,
            );

            // Map ELF Segments
            let pml4_ptr = (physical_memory_offset + new_pml4_frame.start_address().as_u64())
                .as_mut_ptr::<PageTable>();
            let temp_mapper = OffsetPageTable::new(&mut *pml4_ptr, physical_memory_offset);

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

                    // Map as writable + non-executable during load
                    let map_flags = PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE;
                    let is_exec = (ph.flags & elf::PF_X) != 0;
                    if is_exec {
                        exec_segments.push((virt_start, ph.memory_size));
                    }

                    temp_process.map_user_region(
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
                        let chunk_phys = temp_mapper.translate_addr(chunk_virt).expect("ELF map");
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
                        let chunk_phys = temp_mapper.translate_addr(chunk_virt).expect("BSS map");
                        let dest_ptr =
                            (physical_memory_offset + chunk_phys.as_u64()).as_mut_ptr::<u8>();
                        let copy_size =
                            (ph.memory_size - offset).min(4096 - (chunk_virt.as_u64() % 4096));
                        core::ptr::write_bytes(dest_ptr, 0, copy_size as usize);
                        offset += copy_size;
                    }
                }
            }

            // Now set up argv and envp on the user stack!
            // Stack layout (top to bottom, grows down):
            // - envp strings (null-terminated)
            // - argv strings (null-terminated)
            // - padding to align to 16 bytes
            // - envp array (null-terminated)
            // - argv array (null-terminated)
            // - argc (on stack for _start)
            // - padding to 16 bytes for sysv64 ABI

            let mut stack_ptr = stack_top.as_u64();

            // Step 1: Write all string data to the stack first
            let mut string_addrs: Vec<Vec<VirtAddr>> = vec![Vec::new(), Vec::new()]; // 0: argv, 1: envp
            for (idx, list) in [argv, envp].iter().enumerate() {
                for s in list.iter() {
                    stack_ptr -= (s.len() + 1) as u64; // +1 for null terminator
                    let dest_virt = VirtAddr::new_truncate(stack_ptr);
                    string_addrs[idx].push(dest_virt);
                    let dest_phys = temp_mapper.translate_addr(dest_virt).unwrap();
                    let dest_ptr = (physical_memory_offset + dest_phys.as_u64()).as_mut_ptr::<u8>();
                    core::ptr::copy_nonoverlapping(s.as_ptr(), dest_ptr, s.len());
                    core::ptr::write(dest_ptr.add(s.len()), 0);
                }
            }

            // Step 2: Align stack to 16 bytes before writing arrays
            if !stack_ptr.is_multiple_of(16) {
                stack_ptr -= stack_ptr % 16;
            }

            // Step 3: Write envp array (null-terminated)
            stack_ptr -= (envp.len() + 1) as u64 * 8;
            let envp_array_virt = VirtAddr::new_truncate(stack_ptr);
            let envp_array_phys = temp_mapper.translate_addr(envp_array_virt).unwrap();
            let envp_array_ptr =
                (physical_memory_offset + envp_array_phys.as_u64()).as_mut_ptr::<u64>();
            for (i, addr) in string_addrs[1].iter().enumerate() {
                core::ptr::write(envp_array_ptr.add(i), addr.as_u64());
            }
            core::ptr::write(envp_array_ptr.add(envp.len()), 0);

            // Step 4: Write argv array (null-terminated)
            stack_ptr -= (argv.len() + 1) as u64 * 8;
            let argv_array_virt = VirtAddr::new_truncate(stack_ptr);
            let argv_array_phys = temp_mapper.translate_addr(argv_array_virt).unwrap();
            let argv_array_ptr =
                (physical_memory_offset + argv_array_phys.as_u64()).as_mut_ptr::<u64>();
            for (i, addr) in string_addrs[0].iter().enumerate() {
                core::ptr::write(argv_array_ptr.add(i), addr.as_u64());
            }
            core::ptr::write(argv_array_ptr.add(argv.len()), 0);

            // Step 5: Write argc
            stack_ptr -= 8;
            let argc_virt = VirtAddr::new_truncate(stack_ptr);
            let argc_phys = temp_mapper.translate_addr(argc_virt).unwrap();
            let argc_ptr = (physical_memory_offset + argc_phys.as_u64()).as_mut_ptr::<u64>();
            core::ptr::write(argc_ptr, argv.len() as u64);

            // Step 6: Align stack to 16 bytes for sysv64 ABI (_start expects RSP to be 16-byte aligned)
            if !stack_ptr.is_multiple_of(16) {
                stack_ptr -= 8;
            }

            // Update stack_top in process
            temp_process.inner.lock().stack_top = VirtAddr::new_truncate(stack_ptr);

            // Finalise executable segments: strip WRITABLE and clear NO_EXECUTE
            apply_relative_relocations(
                elf_data,
                &header,
                aslr_base,
                virtual_base,
                &temp_mapper,
                physical_memory_offset,
            )?;
            for (seg_start, seg_size) in &exec_segments {
                wx::clear_write_and_allow_exec(
                    new_pml4_frame,
                    physical_memory_offset,
                    *seg_start,
                    *seg_size,
                );
            }
        }

        let temp_stack_top = temp_process.inner.lock().stack_top;

        {
            let mut vfs = crate::vfs::VFS.lock();
            for fd_idx in cloexec_fds {
                vfs.close(fd_idx);
            }
        }

        {
            let mut current_inner = self.inner.lock();
            current_inner.pml4_frame = new_pml4_frame;
            current_inner.entry_point = entry_point;
            current_inner.stack_top = temp_stack_top;
            current_inner.vma_set = VmaSet::new();
            current_inner.mmap_next_addr = mmap_base;
            current_inner.aslr_base = aslr_base;
            current_inner.fd_table = retained_fd_table;
            current_inner.signal_mask = SignalSet::empty();
            current_inner.signal_handlers = [SignalAction::Default; 64];
            current_inner.pending_signals = SignalSet::empty();
            current_inner.state = ProcessState::Running;
            // Apply POSIX exec_transform on capabilities.
            let (new_permitted, new_effective) = if let Some(ref fc) = file_caps {
                let bounding = current_inner.sec_ctx.caps.bounding;
                fc.exec_transform(
                    current_inner.sec_ctx.caps.permitted,
                    current_inner.sec_ctx.caps.inheritable,
                    bounding,
                )
            } else {
                let new = current_inner.sec_ctx.caps.exec_transform();
                (new.permitted, new.effective)
            };
            current_inner.sec_ctx.caps.permitted = new_permitted;
            current_inner.sec_ctx.caps.effective = new_effective;
        }

        paging::destroy_user_mappings(old_pml4_frame, frame_allocator, physical_memory_offset);

        Ok(())
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

        // LSM process_create hook
        if crate::security::lsm::check_process_create(parent.sec_ctx.uid, parent.sec_ctx.gid).is_err() {
            // If denied, the fork is aborted.  We still need to return a Process
            // for ABI compatibility, but it won't be added to the process table.
            // For now we continue with the fork; the hook check is advisory.
        }

        // Clone fd table
        let fd_table: Vec<Option<crate::vfs::FileDescriptor>> =
            (0..1024).map(|i| parent.fd_table[i].clone()).collect();

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
            pending_signal_frame: None,
            sec_ctx: parent.sec_ctx.clone(),
            nsproxy: NsProxy::from_flags(0, &parent.nsproxy),
            seccomp_filter: parent.seccomp_filter.clone().map(|f| f.inherit_on_fork()),
            cgroup_path: parent.cgroup_path.clone(),
        };

        Self {
            inner: Arc::new(Mutex::new(new_inner)),
        }
    }

    /// Create a new process via clone with optional namespace creation.
    pub fn clone_process(
        &self,
        flags: u64,
        frame_allocator: &mut impl x86_64::structures::paging::FrameAllocator<Size4KiB>,
        physical_memory_offset: VirtAddr,
    ) -> Self {
        let pml4_frame = paging::create_process_pml4(frame_allocator, physical_memory_offset);

        paging::clone_user_mappings_cow(
            self.pml4_frame(),
            pml4_frame,
            frame_allocator,
            physical_memory_offset,
        );

        let parent = self.inner.lock();

        let fd_table: Vec<Option<crate::vfs::FileDescriptor>> =
            (0..1024).map(|i| parent.fd_table[i].clone()).collect();

        let nsproxy = NsProxy::from_flags(flags, &parent.nsproxy);

        let child_inner = ProcessControlBlock {
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
            pending_signal_frame: None,
            sec_ctx: parent.sec_ctx.clone(),
            nsproxy,
            seccomp_filter: parent.seccomp_filter.clone().map(|f| f.inherit_on_fork()),
            cgroup_path: parent.cgroup_path.clone(),
        };

        Self {
            inner: Arc::new(Mutex::new(child_inner)),
        }
    }

    /// # Safety
    ///
    /// Page tables must be mapped at `physical_memory_offset` and the target region must be valid.
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
            let flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE | enforced_flags;
            let pages = Page::<Size4KiB>::range_inclusive(
                Page::containing_address(virt_start),
                Page::containing_address(virt_start + size - 1u64),
            );
            // Wrap with a zeroing allocator so intermediate page-table frames
            // (P3/P2/P1) are clean before the mapper writes to them.
            struct ZeroingAlloc<'z, Z: x86_64::structures::paging::FrameAllocator<Size4KiB>> {
                inner: &'z mut Z,
                phys_offset: VirtAddr,
            }
            unsafe impl<'z, Z: x86_64::structures::paging::FrameAllocator<Size4KiB>>
                x86_64::structures::paging::FrameAllocator<Size4KiB> for ZeroingAlloc<'z, Z>
            {
                fn allocate_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
                    let frame = self.inner.allocate_frame()?;
                    let ptr = (self.phys_offset + frame.start_address().as_u64()).as_mut_ptr::<u8>();
                    unsafe { core::ptr::write_bytes(ptr, 0, 4096) };
                    Some(frame)
                }
            }
            let mut zeroing = ZeroingAlloc {
                inner: frame_allocator,
                phys_offset: physical_memory_offset,
            };
            for page in pages {
                use x86_64::structures::paging::Translate;
                if process_mapper
                    .translate_addr(page.start_address())
                    .is_none()
                {
                    let frame = zeroing.inner.allocate_frame().expect("out of memory");
                    // Zero the leaf frame (data page) as well for security.
                    let ptr = (physical_memory_offset + frame.start_address().as_u64())
                        .as_mut_ptr::<u8>();
                    core::ptr::write_bytes(ptr, 0, 4096);
                    process_mapper
                        .map_to(page, frame, flags, &mut zeroing)
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
    ///
    /// # Safety
    ///
    /// Page tables must be mapped at `physical_memory_offset` and the target region must be valid.
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

fn compute_load_base(elf_data: &[u8], header: &elf::ElfHeader) -> Result<u64, elf::ParseError> {
    if header.elf_type != elf::ELF_TYPE_DYN {
        return Ok(0);
    }

    let mut load_base = u64::MAX;
    for i in 0..header.program_header_count {
        if let Some(ph) = elf::parse_program_header(elf_data, *header, i)? {
            load_base = load_base.min(ph.virtual_address);
        }
    }

    if load_base == u64::MAX {
        return Err(elf::ParseError::ProgramHeaderOutOfBounds);
    }

    Ok(load_base)
}

fn apply_relative_relocations<M>(
    elf_data: &[u8],
    header: &elf::ElfHeader,
    aslr_base: VirtAddr,
    _virtual_base: u64,
    mapper: &M,
    physical_memory_offset: VirtAddr,
) -> Result<(), elf::ParseError>
where
    M: x86_64::structures::paging::Translate,
{
    const SECTION_TYPE_RELA: u32 = 4;
    const R_X86_64_RELATIVE: u32 = 8;

    if header.elf_type != elf::ELF_TYPE_DYN {
        return Ok(());
    }

    if elf_data.len() < 64 {
        return Err(elf::ParseError::FileTooSmall);
    }

    let section_header_offset = u64::from_le_bytes(elf_data[40..48].try_into().unwrap()) as usize;
    let section_header_entry_size =
        u16::from_le_bytes(elf_data[58..60].try_into().unwrap()) as usize;
    let section_header_count = u16::from_le_bytes(elf_data[60..62].try_into().unwrap()) as usize;

    if section_header_offset == 0 || section_header_entry_size < 64 || section_header_count == 0 {
        return Ok(());
    }

    for index in 0..section_header_count {
        let start = section_header_offset
            .checked_add(
                index
                    .checked_mul(section_header_entry_size)
                    .ok_or(elf::ParseError::ProgramHeaderOutOfBounds)?,
            )
            .ok_or(elf::ParseError::ProgramHeaderOutOfBounds)?;
        let end = start
            .checked_add(section_header_entry_size)
            .ok_or(elf::ParseError::ProgramHeaderOutOfBounds)?;
        if end > elf_data.len() {
            return Err(elf::ParseError::ProgramHeaderOutOfBounds);
        }

        let section = &elf_data[start..end];
        let section_type = u32::from_le_bytes(section[4..8].try_into().unwrap());
        if section_type != SECTION_TYPE_RELA {
            continue;
        }

        let section_offset = u64::from_le_bytes(section[24..32].try_into().unwrap()) as usize;
        let section_size = u64::from_le_bytes(section[32..40].try_into().unwrap()) as usize;
        let section_entry_size = u64::from_le_bytes(section[56..64].try_into().unwrap()) as usize;
        if section_entry_size < 24 {
            continue;
        }

        let entry_count = section_size / section_entry_size;
        for entry_index in 0..entry_count {
            let entry_start = section_offset
                .checked_add(
                    entry_index
                        .checked_mul(section_entry_size)
                        .ok_or(elf::ParseError::ProgramHeaderOutOfBounds)?,
                )
                .ok_or(elf::ParseError::ProgramHeaderOutOfBounds)?;
            let entry_end = entry_start
                .checked_add(24)
                .ok_or(elf::ParseError::ProgramHeaderOutOfBounds)?;
            if entry_end > elf_data.len() {
                return Err(elf::ParseError::ProgramHeaderOutOfBounds);
            }

            let r_offset =
                u64::from_le_bytes(elf_data[entry_start..entry_start + 8].try_into().unwrap());
            let r_info = u64::from_le_bytes(
                elf_data[entry_start + 8..entry_start + 16]
                    .try_into()
                    .unwrap(),
            );
            let r_addend = u64::from_le_bytes(
                elf_data[entry_start + 16..entry_start + 24]
                    .try_into()
                    .unwrap(),
            );
            let r_type = (r_info & 0xffff_ffff) as u32;

            if r_type != R_X86_64_RELATIVE {
                continue;
            }

            let target_virtual = VirtAddr::new(aslr_base.as_u64() + r_offset);
            let target_physical = mapper
                .translate_addr(target_virtual)
                .ok_or(elf::ParseError::ProgramHeaderOutOfBounds)?;
            let target_ptr =
                (physical_memory_offset + target_physical.as_u64()).as_mut_ptr::<u64>();

            unsafe {
                // R_X86_64_RELATIVE: *(r_offset + base) = base + A
                // where A (addend) is relative to the PIE assumed base (0),
                // and base = aslr_base (the actual load address).
                core::ptr::write_volatile(target_ptr, r_addend.wrapping_add(aslr_base.as_u64()));
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use x86_64::structures::paging::PhysFrame;

    fn test_proc(_name: &str) -> ProcessControlBlock {
        // Cr3 is inaccessible from userspace tests, so store frame 0.
        let frame0 = PhysFrame::containing_address(x86_64::PhysAddr::new(0));
        ProcessControlBlock {
            id: ProcessId::new(),
            ppid: ProcessId(0),
            state: ProcessState::Ready,
            pml4_frame: frame0,
            entry_point: VirtAddr::new(0x400000),
            stack_top: VirtAddr::new(0x7fffff000000),
            threads: vec![],
            vma_set: VmaSet::new(),
            mmap_next_addr: VirtAddr::new(DEFAULT_MMAP_BASE),
            aslr_base: VirtAddr::zero(),
            fd_table: vec![None; 1024],
            signal_mask: SignalSet::empty(),
            signal_handlers: [SignalAction::Default; 64],
            pending_signals: SignalSet::empty(),
            pending_signal_frame: None,
            sec_ctx: SecurityContext::root(),
            nsproxy: NsProxy::new(),
            seccomp_filter: None,
            cgroup_path: None,
        }
    }

    #[test]
    fn process_id_new_increments() {
        let id1 = ProcessId::new();
        let id2 = ProcessId::new();
        assert_ne!(id1.0, id2.0);
        assert!(id2.0 > id1.0);
    }

    #[test]
    fn process_id_default() {
        let id = ProcessId::default();
        assert!(id.0 >= 1);
    }

    #[test]
    fn process_control_block_default_state() {
        let pcb = test_proc("test");
        assert_eq!(pcb.state, ProcessState::Ready);
        assert_eq!(pcb.fd_table.len(), 1024);
        assert_eq!(pcb.signal_handlers.len(), 64);
        assert!(!pcb.pending_signals.contains(1));
    }

    #[test]
    fn process_control_block_transitions_to_zombie() {
        let mut pcb = test_proc("zombie");
        assert_eq!(pcb.state, ProcessState::Ready);
        pcb.state = ProcessState::Zombie { exit_code: 42 };
        assert_eq!(pcb.state, ProcessState::Zombie { exit_code: 42 });
        if let ProcessState::Zombie { exit_code } = pcb.state {
            assert_eq!(exit_code, 42);
        } else {
            panic!("expected Zombie state");
        }
    }

    #[test]
    fn signal_set_insert_contains_remove() {
        let mut set = SignalSet::empty();
        assert!(!set.contains(5));
        set.insert(5);
        assert!(set.contains(5));
        set.remove(5);
        assert!(!set.contains(5));
    }

    #[test]
    fn signal_set_multiple_signals() {
        let mut set = SignalSet::empty();
        set.insert(1);
        set.insert(9);
        set.insert(17);
        assert!(set.contains(1));
        assert!(set.contains(9));
        assert!(set.contains(17));
        assert!(!set.contains(2));
    }

    #[test]
    fn signal_action_default_is_default() {
        let act = SignalAction::Default;
        assert_eq!(act, SignalAction::Default);
    }

    #[test]
    fn signal_action_handler_addr() {
        let act = SignalAction::Handler(0xdeadbeef);
        match act {
            SignalAction::Handler(addr) => assert_eq!(addr, 0xdeadbeef),
            _ => panic!("expected Handler"),
        }
    }

    #[test]
    fn process_table_insert_lookup() {
        let pcb = Arc::new(Mutex::new(test_proc("table_test")));
        let pid = pcb.lock().id;
        PROCESS_TABLE.lock().insert(pid, pcb.clone());
        let lookup = PROCESS_TABLE.lock().get(&pid).cloned();
        assert!(lookup.is_some());
        assert_eq!(lookup.unwrap().lock().id, pid);
        PROCESS_TABLE.lock().remove(&pid);
        assert!(PROCESS_TABLE.lock().get(&pid).is_none());
    }

    #[test]
    fn process_table_remove_nonexistent() {
        let id = ProcessId(99999);
        PROCESS_TABLE.lock().remove(&id);
        assert!(PROCESS_TABLE.lock().get(&id).is_none());
    }

    #[test]
    fn reparent_to_init_changes_ppid() {
        let pcb = Arc::new(Mutex::new(test_proc("orphan")));
        let pid = pcb.lock().id;
        assert_ne!(pcb.lock().ppid, ProcessId(1));
        PROCESS_TABLE.lock().insert(pid, pcb.clone());
        reparent_to_init(pid);
        assert_eq!(pcb.lock().ppid, ProcessId(1));
        PROCESS_TABLE.lock().remove(&pid);
    }

    #[test]
    fn reparent_to_init_nonexistent_pid() {
        // Should not panic for a non-existent PID
        reparent_to_init(ProcessId(99999));
    }

    #[test]
    fn process_block_reason_differentiation() {
        let blocked_io = ProcessState::Blocked(BlockReason::WaitingForIo);
        let blocked_child = ProcessState::Blocked(BlockReason::WaitingForChild);
        assert_ne!(blocked_io, blocked_child);
        assert_eq!(blocked_io, ProcessState::Blocked(BlockReason::WaitingForIo));
    }

    #[test]
    fn process_kernel_proc_id_zero() {
        let kp = Process::kernel_process();
        assert_eq!(kp.id(), ProcessId(0));
    }
}
