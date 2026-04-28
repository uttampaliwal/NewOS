use x86_64::VirtAddr;
use x86_64::structures::paging::{FrameAllocator, Mapper, Page, PageTableFlags, Size4KiB};

pub mod scheduler;

pub type TaskEntry = extern "sysv64" fn() -> !;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Ready,
    Running,
    Blocked,
    Zombie,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TaskId(usize);

impl TaskId {
    pub fn new() -> Self {
        use core::sync::atomic::{AtomicUsize, Ordering};
        static NEXT_ID: AtomicUsize = AtomicUsize::new(1);
        TaskId(NEXT_ID.fetch_add(1, Ordering::Relaxed))
    }
}

pub struct Task {
    #[allow(dead_code)]
    id: TaskId,
    pub(crate) stack_ptr: usize,
    pub(crate) kernel_stack_top: usize,
    state: TaskState,
}

// SAFETY: Task owns its stack and contains no borrowed state.
unsafe impl Send for Task {}
unsafe impl Sync for Task {}

impl Task {
    pub fn new(
        entry: TaskEntry,
        mapper: &mut impl Mapper<Size4KiB>,
        frame_allocator: &mut impl FrameAllocator<Size4KiB>,
    ) -> Self {
        const STACK_PAGES: u64 = 4;
        const GUARD_PAGES: u64 = 1;
        const STACK_SIZE: u64 = STACK_PAGES * 4096;
        const STACK_STRIDE: u64 = (STACK_PAGES + GUARD_PAGES + 1) * 4096;

        let id = TaskId::new();
        let stack_region_base =
            VirtAddr::new(0xFFFF_FE00_0000_0000 + (id.0 as u64) * STACK_STRIDE);
        let usable_stack_start = stack_region_base + (GUARD_PAGES * 4096);
        let stack_top_virt = usable_stack_start + STACK_SIZE;

        unsafe {
            let pages = Page::<Size4KiB>::range_inclusive(
                Page::containing_address(usable_stack_start),
                Page::containing_address(stack_top_virt - 1u64),
            );

            for page in pages {
                let frame = frame_allocator.allocate_frame().expect("out of memory for kernel stack");
                mapper
                    .map_to(
                        page,
                        frame,
                        PageTableFlags::PRESENT | PageTableFlags::WRITABLE,
                        frame_allocator,
                    )
                    .expect("failed to map kernel stack page")
                    .flush();
            }
        }

        let mut stack_ptr = stack_top_virt.as_mut_ptr::<usize>();

        unsafe {
            // THE NEWOS CONTEXT FRAME
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
        }

        Self {
            id,
            stack_ptr: stack_ptr as usize,
            kernel_stack_top: stack_top_virt.as_u64() as usize,
            state: TaskState::Ready,
        }
    }

    pub fn switch_to(&self) {
        crate::gdt::set_interrupt_stack(VirtAddr::new(self.kernel_stack_top as u64));
    }
}
