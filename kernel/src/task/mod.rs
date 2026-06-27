use crate::process::{Process, ProcessId};
#[cfg(target_arch = "x86_64")]
use x86_64::VirtAddr;
#[cfg(target_arch = "x86_64")]
use x86_64::structures::paging::{
    FrameAllocator, Mapper, OffsetPageTable, Page, PageTable, PageTableFlags, Size4KiB, Translate,
};

pub mod scheduler;
pub mod scheduler_class;
pub mod scheduler_extra_tests;
pub mod signals;

#[cfg(target_arch = "x86_64")]
pub type TaskEntry = extern "sysv64" fn() -> !;
#[cfg(not(target_arch = "x86_64"))]
pub type TaskEntry = extern "C" fn() -> !;

/// Errors that can occur during task creation.
#[derive(Debug)]
pub enum TaskError {
    /// Frame allocator exhausted — out of memory for kernel stack pages.
    OutOfMemory,
    /// Page table mapping failed.
    MappingFailed,
}

const KERNEL_STACK_REGION_BASE: u64 = 0xFFFF_FE00_0000_0000;
const KERNEL_STACK_PAGES: u64 = 32;
pub const KERNEL_STACK_SIZE: u64 = KERNEL_STACK_PAGES * 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Ready,
    Running,
    Blocked,
    Zombie,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TaskId(pub usize);

impl Default for TaskId {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskId {
    pub fn new() -> Self {
        use core::sync::atomic::{AtomicUsize, Ordering};
        static NEXT_ID: AtomicUsize = AtomicUsize::new(1);
        TaskId(NEXT_ID.fetch_add(1, Ordering::Relaxed))
    }

    pub fn as_usize(&self) -> usize {
        self.0
    }
}

pub struct Task {
    pub id: TaskId,
    pub(crate) stack_ptr: usize,
    pub(crate) kernel_stack_top: usize,
    pub(crate) process: Process,
    /// Cached process ID — avoids re-locking `process.inner` (spin::Mutex is not re-entrant).
    pub(crate) pid: ProcessId,
    pub state: TaskState,
    pub policy: scheduler_class::SchedulingPolicy,
    pub priority: u8,
    pub time_slice: u32,
    pub vruntime: u64,
    pub eligible: bool,
    pub deadline: u64,
    pub lag: i64,
    pub weight: u32,
}

impl Task {
    /// Create a minimal test task with default scheduling fields.
    pub fn new_test(id: TaskId, process: Process, state: TaskState) -> Self {
        let pid = process.id();
        Task {
            id,
            stack_ptr: 0,
            kernel_stack_top: 0,
            process,
            pid,
            state,
            policy: scheduler_class::SchedulingPolicy::SCHED_NORMAL,
            priority: scheduler_class::base_priority(
                scheduler_class::SchedulingPolicy::SCHED_NORMAL,
            ),
            time_slice: scheduler_class::DEFAULT_TIMESLICE,
            vruntime: 0,
            eligible: true,
            deadline: 0,
            lag: 0,
            weight: 1024,
        }
    }
}

// SAFETY: Task owns its stack and contains no borrowed state.
unsafe impl Send for Task {}
unsafe impl Sync for Task {}

pub fn init_kernel_stack_region(
    mapper: &mut (impl Mapper<Size4KiB> + Translate),
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) {
    let anchor_page = Page::<Size4KiB>::containing_address(VirtAddr::new(KERNEL_STACK_REGION_BASE));
    if mapper.translate_addr(anchor_page.start_address()).is_some() {
        return;
    }

    let frame = frame_allocator
        .allocate_frame()
        .expect("out of memory for kernel stack region anchor");

    // Safety: anchor_page is not already mapped (checked above); frame is freshly allocated.
    unsafe {
        match mapper.map_to(
            anchor_page,
            frame,
            PageTableFlags::PRESENT | PageTableFlags::WRITABLE,
            frame_allocator,
        ) {
            Ok(flush) => flush.flush(),
            Err(_) => {
                // The anchor only exists to force the kernel-stack PML4 branch
                // to be present before process address spaces are cloned.
            }
        }
    }
}

impl Task {
    pub fn new(
        entry: TaskEntry,
        mapper: &mut impl Mapper<Size4KiB>,
        frame_allocator: &mut impl FrameAllocator<Size4KiB>,
    ) -> Result<Self, TaskError> {
        const STACK_PAGES: u64 = 32;
        const GUARD_PAGES: u64 = 1;
        const STACK_SIZE: u64 = STACK_PAGES * 4096;
        const STACK_STRIDE: u64 = (STACK_PAGES + GUARD_PAGES + 1) * 4096;

        let id = TaskId::new();
        let stack_region_base =
            VirtAddr::new(KERNEL_STACK_REGION_BASE + (id.0 as u64) * STACK_STRIDE);
        let usable_stack_start = stack_region_base + (GUARD_PAGES * 4096);
        let stack_top_virt = usable_stack_start + STACK_SIZE;

        // Safety: pages are in the kernel stack region; frame_allocator provides valid frames.
        unsafe {
            let pages = Page::<Size4KiB>::range_inclusive(
                Page::containing_address(usable_stack_start),
                Page::containing_address(stack_top_virt - 1u64),
            );

            for page in pages {
                let frame = frame_allocator
                    .allocate_frame()
                    .ok_or(TaskError::OutOfMemory)?;
                mapper
                    .map_to(
                        page,
                        frame,
                        PageTableFlags::PRESENT | PageTableFlags::WRITABLE,
                        frame_allocator,
                    )
                    .map_err(|_| TaskError::MappingFailed)?
                    .flush();
            }
        }

        let mut stack_ptr = stack_top_virt.as_mut_ptr::<usize>();

        // Safety: stack_ptr points to valid writable kernel stack memory; frame layout matches iretq expectations.
        unsafe {
            //
            // When a task is NOT running, its stack looks like this (from high to low address):
            // 1. [CPU FRAME] SS
            // 2. [CPU FRAME] RSP
            // 3. [CPU FRAME] RFLAGS
            // 4. [CPU FRAME] CS
            // 5. [CPU FRAME] RIP
            // 6. RAX, RBX, RCX, RDX, RBP, RSI, RDI, R8, R9, R10, R11, R12, R13, R14, R15 (General Purpose)
            //
            // We use 'iretq' to return to both kernel threads and user processes, so we must
            // ensure the stack always contains a valid CPU frame.

            stack_ptr = stack_ptr.sub(1);

            stack_ptr.write(0x10);
            stack_ptr = stack_ptr.sub(1);
            stack_ptr.write(stack_top_virt.as_u64() as usize);
            stack_ptr = stack_ptr.sub(1);
            stack_ptr.write(0x202);
            stack_ptr = stack_ptr.sub(1);
            stack_ptr.write(0x08);
            stack_ptr = stack_ptr.sub(1);
            stack_ptr.write(entry as usize);

            for _ in 0..15 {
                stack_ptr = stack_ptr.sub(1);
                stack_ptr.write(0);
            }

            // Write stack canary at the bottom of the usable stack
            let canary = crate::security::ima::canary_value();
            (usable_stack_start.as_mut_ptr::<u64>()).write(canary);
        }

        let process = Process::kernel_process();
        let pid = process.id();
        let task = Self {
            id,
            stack_ptr: stack_ptr as usize,
            kernel_stack_top: stack_top_virt.as_u64() as usize,
            process: process.clone(),
            pid,
            state: TaskState::Ready,
            policy: scheduler_class::SchedulingPolicy::SCHED_NORMAL,
            priority: scheduler_class::base_priority(
                scheduler_class::SchedulingPolicy::SCHED_NORMAL,
            ),
            time_slice: scheduler_class::DEFAULT_TIMESLICE,
            vruntime: 0,
            eligible: true,
            deadline: 0,
            lag: 0,
            weight: 1024,
        };
        process.add_thread(id);
        Ok(task)
    }

    pub fn new_user(
        process: Process,
        mapper: &mut impl Mapper<Size4KiB>,
        frame_allocator: &mut impl FrameAllocator<Size4KiB>,
        physical_memory_offset: x86_64::VirtAddr,
    ) -> Result<Self, TaskError> {
        const STACK_PAGES: u64 = 32;
        const GUARD_PAGES: u64 = 1;
        const STACK_SIZE: u64 = STACK_PAGES * 4096;
        const STACK_STRIDE: u64 = (STACK_PAGES + GUARD_PAGES + 1) * 4096;

        let id = TaskId::new();
        let stack_region_base =
            VirtAddr::new(KERNEL_STACK_REGION_BASE + (id.0 as u64) * STACK_STRIDE);
        let usable_stack_start = stack_region_base + (GUARD_PAGES * 4096);
        let stack_top_virt = usable_stack_start + STACK_SIZE;

        // Safety: pages are in the kernel stack region; frames are freshly allocated.
        unsafe {
            let pages = Page::<Size4KiB>::range_inclusive(
                Page::containing_address(usable_stack_start),
                Page::containing_address(stack_top_virt - 1u64),
            );

            for page in pages {
                let frame = frame_allocator
                    .allocate_frame()
                    .ok_or(TaskError::OutOfMemory)?;

                // 1. Map the kernel stack into the CURRENT (kernel) address space.
                // This allows us to initialize the stack contents below.
                if mapper
                    .map_to(
                        page,
                        frame,
                        PageTableFlags::PRESENT | PageTableFlags::WRITABLE,
                        frame_allocator,
                    )
                    .is_err()
                {
                    // Already mapped, ignore
                } else {
                    use x86_64::instructions::tlb;
                    tlb::flush(page.start_address());
                }

                // 2. Map the kernel stack into the NEW process address space.
                let pml4_ptr = (physical_memory_offset
                    + process.pml4_frame().start_address().as_u64())
                .as_mut_ptr::<PageTable>();
                let mut process_mapper =
                    OffsetPageTable::new(&mut *pml4_ptr, physical_memory_offset);

                if process_mapper
                    .map_to(
                        page,
                        frame,
                        PageTableFlags::PRESENT | PageTableFlags::WRITABLE,
                        frame_allocator,
                    )
                    .is_err()
                {
                    // Already mapped, ignore
                }
            }
        }

        let mut stack_ptr = stack_top_virt.as_mut_ptr::<usize>();

        // Safety: stack_ptr points to valid writable kernel stack memory; layout matches iretq frame for user entry.
        unsafe {
            // SS (User Data 0x23)
            stack_ptr = stack_ptr.sub(1);
            stack_ptr.write(0x23);
            // RSP
            stack_ptr = stack_ptr.sub(1);
            stack_ptr.write(process.stack_top().as_u64() as usize);
            // RFLAGS
            stack_ptr = stack_ptr.sub(1);
            stack_ptr.write(0x202);
            // CS (User Code 0x2b)
            stack_ptr = stack_ptr.sub(1);
            stack_ptr.write(0x2b);
            // RIP
            stack_ptr = stack_ptr.sub(1);
            stack_ptr.write(process.entry_point().as_u64() as usize);

            // General Purpose Registers (15 zeros)
            for _ in 0..15 {
                stack_ptr = stack_ptr.sub(1);
                stack_ptr.write(0);
            }

            // Write stack canary at the bottom of the usable stack
            let canary = crate::security::ima::canary_value();
            (usable_stack_start.as_mut_ptr::<u64>()).write(canary);
        }

        process.add_thread(id);
        let pid = process.id();
        Ok(Self {
            id,
            stack_ptr: stack_ptr as usize,
            kernel_stack_top: stack_top_virt.as_u64() as usize,
            process,
            pid,
            state: TaskState::Ready,
            policy: scheduler_class::SchedulingPolicy::SCHED_NORMAL,
            priority: scheduler_class::base_priority(
                scheduler_class::SchedulingPolicy::SCHED_NORMAL,
            ),
            time_slice: scheduler_class::DEFAULT_TIMESLICE,
            vruntime: 0,
            eligible: true,
            deadline: 0,
            lag: 0,
            weight: 1024,
        })
    }

    /// Create a task for a forked child process.
    ///
    /// The child's kernel stack is initialised with a copy of `parent_frame`
    /// (the parent's saved syscall context) but with `rax = 0` so that
    /// `fork()` returns 0 in the child.
    #[cfg(target_arch = "x86_64")]
    pub fn new_forked_user(
        process: Process,
        parent_frame: &crate::arch::x86_64::syscall_arch::SyscallFrame,
        mapper: &mut impl Mapper<Size4KiB>,
        frame_allocator: &mut impl FrameAllocator<Size4KiB>,
        physical_memory_offset: VirtAddr,
    ) -> Result<Self, TaskError> {
        const STACK_PAGES: u64 = 32;
        const GUARD_PAGES: u64 = 1;
        const STACK_SIZE: u64 = STACK_PAGES * 4096;
        const STACK_STRIDE: u64 = (STACK_PAGES + GUARD_PAGES + 1) * 4096;

        let id = TaskId::new();
        let stack_region_base =
            VirtAddr::new(KERNEL_STACK_REGION_BASE + (id.0 as u64) * STACK_STRIDE);
        let usable_stack_start = stack_region_base + (GUARD_PAGES * 4096);
        let stack_top_virt = usable_stack_start + STACK_SIZE;

        // Safety: pages are in the kernel stack region; frames are freshly allocated.
        unsafe {
            let pages = Page::<Size4KiB>::range_inclusive(
                Page::containing_address(usable_stack_start),
                Page::containing_address(stack_top_virt - 1u64),
            );

            for page in pages {
                let frame = frame_allocator
                    .allocate_frame()
                    .ok_or(TaskError::OutOfMemory)?;

                // Map into the current (kernel) address space for initialization.
                if mapper
                    .map_to(
                        page,
                        frame,
                        PageTableFlags::PRESENT | PageTableFlags::WRITABLE,
                        frame_allocator,
                    )
                    .is_err()
                {
                    // Already mapped — ignore.
                } else {
                    use x86_64::instructions::tlb;
                    tlb::flush(page.start_address());
                }

                // Also map into the child process's address space.
                let pml4_ptr = (physical_memory_offset
                    + process.pml4_frame().start_address().as_u64())
                .as_mut_ptr::<PageTable>();
                let mut process_mapper =
                    OffsetPageTable::new(&mut *pml4_ptr, physical_memory_offset);

                if process_mapper
                    .map_to(
                        page,
                        frame,
                        PageTableFlags::PRESENT | PageTableFlags::WRITABLE,
                        frame_allocator,
                    )
                    .is_err()
                {
                    // Already mapped — ignore.
                }
            }
        }

        // Build the child's kernel stack as a copy of the parent's SyscallFrame
        // but with rax = 0 (fork returns 0 to child).
        //
        // The frame layout on the kernel stack (low → high):
        //   [r15, r14, r13, r12, r11, r10, r9, r8, rdi, rsi, rbp, rdx, rcx, rbx, rax] (15 × 8 bytes)
        //   [user_rip, user_cs, user_rflags, user_rsp, user_ss] ( 5 × 8 bytes)
        //
        // The scheduler restores using the pop sequence in timer_tick / start_scheduling.

        // Sanitize RFLAGS: keep only safe bits (IF, AC, ID) and force bit 1 (reserved, must be 1).
        // Clear IOPL (bits 12-13), NT (bit 14), VM (bit 17), and other dangerous flags.
        const SAFE_RFLAGS_MASK: u64 = 0x202; // IF=1, reserved bit 1=1
        let safe_rflags = (parent_frame.user_rflags & 0x3C7FFD) | SAFE_RFLAGS_MASK;

        // Validate CS and SS are user-mode selectors
        let user_cs = if parent_frame.user_cs & 0x3 == 0x3 {
            parent_frame.user_cs
        } else {
            crate::serial::println!(
                "[fork] WARN: parent CS={:#x} not user-mode, using 0x2b",
                parent_frame.user_cs
            );
            0x2b // USER_CODE_SEGMENT
        };
        let user_ss = if parent_frame.user_ss & 0x3 == 0x3 {
            parent_frame.user_ss
        } else {
            crate::serial::println!(
                "[fork] WARN: parent SS={:#x} not user-mode, using 0x23",
                parent_frame.user_ss
            );
            0x23 // USER_DATA_SEGMENT
        };

        crate::serial::println!(
            "[fork] child frame: RIP={:#x} CS={:#x} RFLAGS={:#x} RSP={:#x} SS={:#x}",
            parent_frame.user_rip,
            user_cs,
            safe_rflags,
            parent_frame.user_rsp,
            user_ss
        );

        let mut stack_ptr = stack_top_virt.as_mut_ptr::<u64>();

        // Safety: stack_ptr points to valid writable kernel stack memory; frame matches parent's SyscallFrame layout.
        unsafe {
            // IRETQ frame (high addresses first — pushed last).
            stack_ptr = stack_ptr.sub(1);
            stack_ptr.write(user_ss);
            stack_ptr = stack_ptr.sub(1);
            stack_ptr.write(parent_frame.user_rsp);
            stack_ptr = stack_ptr.sub(1);
            stack_ptr.write(safe_rflags);
            stack_ptr = stack_ptr.sub(1);
            stack_ptr.write(user_cs);
            stack_ptr = stack_ptr.sub(1);
            stack_ptr.write(parent_frame.user_rip);

            // General-purpose registers (pushed in reverse order of pop sequence).
            // rax = 0 so fork() returns 0 to child; all others copied from parent.
            stack_ptr = stack_ptr.sub(1);
            stack_ptr.write(0u64); // rax = 0
            stack_ptr = stack_ptr.sub(1);
            stack_ptr.write(parent_frame.rbx);
            stack_ptr = stack_ptr.sub(1);
            stack_ptr.write(parent_frame.rcx);
            stack_ptr = stack_ptr.sub(1);
            stack_ptr.write(parent_frame.rdx);
            stack_ptr = stack_ptr.sub(1);
            stack_ptr.write(parent_frame.rbp);
            stack_ptr = stack_ptr.sub(1);
            stack_ptr.write(parent_frame.rsi);
            stack_ptr = stack_ptr.sub(1);
            stack_ptr.write(parent_frame.rdi);
            stack_ptr = stack_ptr.sub(1);
            stack_ptr.write(parent_frame.r8);
            stack_ptr = stack_ptr.sub(1);
            stack_ptr.write(parent_frame.r9);
            stack_ptr = stack_ptr.sub(1);
            stack_ptr.write(parent_frame.r10);
            stack_ptr = stack_ptr.sub(1);
            stack_ptr.write(parent_frame.r11);
            stack_ptr = stack_ptr.sub(1);
            stack_ptr.write(parent_frame.r12);
            stack_ptr = stack_ptr.sub(1);
            stack_ptr.write(parent_frame.r13);
            stack_ptr = stack_ptr.sub(1);
            stack_ptr.write(parent_frame.r14);
            stack_ptr = stack_ptr.sub(1);
            stack_ptr.write(parent_frame.r15);

            // Write stack canary at the bottom of the usable stack
            let canary = crate::security::ima::canary_value();
            (usable_stack_start.as_mut_ptr::<u64>()).write(canary);
        }

        process.add_thread(id);
        let pid = process.id();
        Ok(Self {
            id,
            stack_ptr: stack_ptr as usize,
            kernel_stack_top: stack_top_virt.as_u64() as usize,
            process,
            pid,
            state: TaskState::Ready,
            policy: scheduler_class::SchedulingPolicy::SCHED_NORMAL,
            priority: scheduler_class::base_priority(
                scheduler_class::SchedulingPolicy::SCHED_NORMAL,
            ),
            time_slice: scheduler_class::DEFAULT_TIMESLICE,
            vruntime: 0,
            eligible: true,
            deadline: 0,
            lag: 0,
            weight: 1024,
        })
    }

    /// Create a task for an exec'd process.
    ///
    /// Sets up a fresh kernel stack that jumps to the new entry point, with a new user stack.
    #[cfg(target_arch = "x86_64")]
    pub fn new_exec_user(
        process: Process,
        mapper: &mut impl Mapper<Size4KiB>,
        frame_allocator: &mut impl FrameAllocator<Size4KiB>,
        physical_memory_offset: VirtAddr,
    ) -> Result<Self, TaskError> {
        const STACK_PAGES: u64 = 32;
        const GUARD_PAGES: u64 = 1;
        const STACK_SIZE: u64 = STACK_PAGES * 4096;
        const STACK_STRIDE: u64 = (STACK_PAGES + GUARD_PAGES + 1) * 4096;

        let id = TaskId::new();
        let stack_region_base =
            VirtAddr::new(KERNEL_STACK_REGION_BASE + (id.0 as u64) * STACK_STRIDE);
        let usable_stack_start = stack_region_base + (GUARD_PAGES * 4096);
        let stack_top_virt = usable_stack_start + STACK_SIZE;

        // Safety: pages are in the kernel stack region; frames are freshly allocated.
        unsafe {
            let pages = Page::<Size4KiB>::range_inclusive(
                Page::containing_address(usable_stack_start),
                Page::containing_address(stack_top_virt - 1u64),
            );

            for page in pages {
                let frame = frame_allocator
                    .allocate_frame()
                    .ok_or(TaskError::OutOfMemory)?;

                // Map into current (kernel) address space
                if mapper
                    .map_to(
                        page,
                        frame,
                        PageTableFlags::PRESENT | PageTableFlags::WRITABLE,
                        frame_allocator,
                    )
                    .is_err()
                {
                    // Already mapped
                } else {
                    use x86_64::instructions::tlb;
                    tlb::flush(page.start_address());
                }

                // Map into new process address space
                let pml4_ptr = (physical_memory_offset
                    + process.pml4_frame().start_address().as_u64())
                .as_mut_ptr::<PageTable>();
                let mut process_mapper =
                    OffsetPageTable::new(&mut *pml4_ptr, physical_memory_offset);

                if process_mapper
                    .map_to(
                        page,
                        frame,
                        PageTableFlags::PRESENT | PageTableFlags::WRITABLE,
                        frame_allocator,
                    )
                    .is_err()
                {
                    // Already mapped
                }
            }
        }

        let mut stack_ptr = stack_top_virt.as_mut_ptr::<usize>();

        // Safety: stack_ptr points to valid writable kernel stack memory; layout matches iretq frame for exec entry.
        unsafe {
            // SS (user data)
            stack_ptr = stack_ptr.sub(1);
            stack_ptr.write(0x23);
            // RSP (user stack top)
            stack_ptr = stack_ptr.sub(1);
            stack_ptr.write(process.stack_top().as_u64() as usize);
            // RFLAGS
            stack_ptr = stack_ptr.sub(1);
            stack_ptr.write(0x202);
            // CS (user code)
            stack_ptr = stack_ptr.sub(1);
            stack_ptr.write(0x2b);
            // RIP (entry point)
            stack_ptr = stack_ptr.sub(1);
            stack_ptr.write(process.entry_point().as_u64() as usize);

            // General purpose registers (all zeros)
            for _ in 0..15 {
                stack_ptr = stack_ptr.sub(1);
                stack_ptr.write(0);
            }

            // Write stack canary at the bottom of the usable stack
            let canary = crate::security::ima::canary_value();
            (usable_stack_start.as_mut_ptr::<u64>()).write(canary);
        }

        process.add_thread(id);
        let pid = process.id();
        Ok(Self {
            id,
            stack_ptr: stack_ptr as usize,
            kernel_stack_top: stack_top_virt.as_u64() as usize,
            process,
            pid,
            state: TaskState::Ready,
            policy: scheduler_class::SchedulingPolicy::SCHED_NORMAL,
            priority: scheduler_class::base_priority(
                scheduler_class::SchedulingPolicy::SCHED_NORMAL,
            ),
            time_slice: scheduler_class::DEFAULT_TIMESLICE,
            vruntime: 0,
            eligible: true,
            deadline: 0,
            lag: 0,
            weight: 1024,
        })
    }

    pub fn switch_to(&self) {
        #[cfg(target_arch = "x86_64")]
        {
            // Check kernel stack canary before switching.
            if self.kernel_stack_top > KERNEL_STACK_SIZE as usize {
                let canary_addr = self.kernel_stack_top - KERNEL_STACK_SIZE as usize;
                let expected = crate::security::ima::canary_value();
                // Safety: canary_addr points to the bottom of the kernel stack; reading u64 is valid.
            let actual = unsafe { core::ptr::read_unaligned(canary_addr as *const u64) };
                if actual != expected {
                    crate::serial::println!(
                        "[PANIC] Kernel stack canary corrupted for task {}! expected=0x{:016x}, actual=0x{:016x}",
                        self.id.0,
                        expected,
                        actual,
                    );
                    panic!("Kernel stack canary corruption detected");
                }
            }

            crate::gdt::set_interrupt_stack(x86_64::VirtAddr::new(self.kernel_stack_top as u64));

            let (current_pml4, _) = x86_64::registers::control::Cr3::read();
            if current_pml4 != self.process.pml4_frame() {
                // Safety: pml4_frame is a valid process page table; writing Cr3 switches address space.
                unsafe {
                    x86_64::registers::control::Cr3::write(
                        self.process.pml4_frame(),
                        x86_64::registers::control::Cr3Flags::empty(),
                    );
                }
            }
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            let _ = self;
            // AArch64 switch_to implementation
        }
    }
}
