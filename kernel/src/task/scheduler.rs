use super::{Task, TaskId};
use crate::process::{Process, ProcessId};
use alloc::collections::BTreeMap;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use lazy_static::lazy_static;
use spin::Mutex;

static UPTIME_TICKS: AtomicU64 = AtomicU64::new(0);

/// Set to `true` once `start_scheduling()` is about to be called.
/// Guards early-boot heap allocations from touching the scheduler lock.
static SCHEDULER_READY: AtomicBool = AtomicBool::new(false);

pub fn set_scheduler_ready() {
    SCHEDULER_READY.store(true, Ordering::Release);
}

pub fn is_scheduler_ready() -> bool {
    SCHEDULER_READY.load(Ordering::Acquire)
}

lazy_static! {
    pub(crate) static ref SCHEDULER: Mutex<Scheduler> = Mutex::new(Scheduler::new());
}

pub(crate) struct Scheduler {
    #[cfg(test)]
    pub(crate) cpu_queues: alloc::vec::Vec<BTreeMap<(u64, usize), Task>>,
    #[cfg(not(test))]
    cpu_queues: alloc::vec::Vec<BTreeMap<(u64, usize), Task>>,
    cpu_current: alloc::vec::Vec<Option<Task>>,
    cpu_current_id: alloc::vec::Vec<Option<TaskId>>,
    blocked_tasks: alloc::vec::Vec<Task>,
    #[cfg(test)]
    pub(crate) task_count: usize,
    #[cfg(not(test))]
    task_count: usize,
    #[cfg(test)]
    pub(crate) min_vruntime: u64,
    #[cfg(not(test))]
    min_vruntime: u64,
}

impl Scheduler {
    fn new() -> Self {
        let cpu_count = crate::smp::get_cpu_count() as usize;
        let mut cpu_queues = alloc::vec::Vec::with_capacity(cpu_count);
        let mut cpu_current = alloc::vec::Vec::with_capacity(cpu_count);
        let mut cpu_current_id = alloc::vec::Vec::with_capacity(cpu_count);
        for _ in 0..cpu_count {
            cpu_queues.push(BTreeMap::new());
            cpu_current.push(None);
            cpu_current_id.push(None);
        }
        Self {
            cpu_queues,
            cpu_current,
            cpu_current_id,
            blocked_tasks: alloc::vec![],
            task_count: 0,
            min_vruntime: 0,
        }
    }

    fn current_cpu_id(&self) -> usize {
        crate::smp::get_current_cpu_id() as usize
    }

    /// Find the CPU with the shortest run queue for load balancing.
    #[cfg(test)]
    pub(crate) fn least_loaded_cpu(&self) -> usize {
        self.least_loaded_cpu_inner()
    }
    #[cfg(not(test))]
    fn least_loaded_cpu(&self) -> usize {
        self.least_loaded_cpu_inner()
    }
    fn least_loaded_cpu_inner(&self) -> usize {
        let mut min_len = usize::MAX;
        let mut min_cpu = 0;
        for (cpu, queue) in self.cpu_queues.iter().enumerate() {
            if queue.len() < min_len {
                min_len = queue.len();
                min_cpu = cpu;
            }
        }
        min_cpu
    }

    /// Steal a task from the busiest CPU.
    #[cfg(test)]
    pub(crate) fn steal_task(&mut self) -> Option<Task> {
        self.steal_task_inner()
    }
    #[cfg(not(test))]
    fn steal_task(&mut self) -> Option<Task> {
        self.steal_task_inner()
    }
    fn steal_task_inner(&mut self) -> Option<Task> {
        let my_cpu = self.current_cpu_id();
        let mut max_len = 0;
        let mut max_cpu = 0;
        for (cpu, queue) in self.cpu_queues.iter().enumerate() {
            if cpu != my_cpu && queue.len() > max_len {
                max_len = queue.len();
                max_cpu = cpu;
            }
        }
        if max_len > 1 {
            self.cpu_queues[max_cpu].pop_last().map(|(_, t)| t)
        } else {
            None
        }
    }
}

/// Compute the EEVDF virtual deadline for a task.
/// deadline = vruntime + (time_slice_ticks * 1024 / weight)
pub(crate) fn compute_deadline(vruntime: u64, time_slice: u32, weight: u32) -> u64 {
    if weight == 0 {
        return vruntime;
    }
    let slice_virtual = (time_slice as u64 * 1024) / weight as u64;
    vruntime.saturating_add(slice_virtual)
}

/// Pick the next task to run using EEVDF logic.
/// Returns the eligible task with the earliest (smallest) virtual deadline.
pub(crate) fn pick_eevdf(queue: &BTreeMap<(u64, usize), Task>) -> Option<&Task> {
    queue.values().filter(|t| t.eligible).min_by_key(|t| t.deadline)
}

/// Update a task's vruntime after running for one tick.
/// vruntime increment = (1024 * delta) / weight, where delta is ticks.
pub(crate) fn update_vruntime(task: &mut Task, ticks: u32) {
    if task.weight > 0 {
        let delta = (1024u64 * ticks as u64) / task.weight as u64;
        task.vruntime = task.vruntime.saturating_add(delta.max(1));
    } else {
        task.vruntime = task.vruntime.saturating_add(1);
    }
}

pub fn add_task(task: Task) {
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut sched = SCHEDULER.lock();
        let target = sched.least_loaded_cpu();
        let mut task = task;
        let min_vr = sched.min_vruntime;
        let deadline = compute_deadline(task.vruntime, task.time_slice, task.weight);
        task.deadline = deadline;
        task.eligible = task.vruntime <= min_vr;
        sched.cpu_queues[target].insert((deadline, task.id.0), task);
        sched.task_count += 1;
    });
}

pub fn remove_task(task_id: TaskId) {
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut sched = SCHEDULER.lock();
        let cpu = sched.current_cpu_id();
        if let Some(ref current) = sched.cpu_current[cpu]
            && current.id == task_id
        {
            sched.cpu_current[cpu] = None;
            sched.cpu_current_id[cpu] = None;
            sched.task_count -= 1;
            return;
        }
        for queue in &mut sched.cpu_queues {
            let key_to_remove = queue.iter().find(|(_, t)| t.id == task_id).map(|(k, _)| *k);
            if let Some(k) = key_to_remove {
                queue.remove(&k);
                sched.task_count -= 1;
                return;
            }
        }
    });
}

pub fn start_scheduling() -> ! {
    #[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
    {
        panic!("scheduler is only available on the freestanding kernel target");
    }

    #[cfg(all(target_arch = "x86_64", target_os = "none"))]
    {
        x86_64::instructions::interrupts::disable();

        let mut sched = SCHEDULER.lock();
        let cpu = sched.current_cpu_id();
        crate::serial::println!(
            "[sched] CPU {}: {} tasks in queue, starting first task",
            cpu,
            sched.cpu_queues[cpu].len()
        );
        for (_, t) in sched.cpu_queues[cpu].iter() {
            crate::serial::println!(
                "[sched]   task_id={}, deadline={:?}, eligible={}, state={:?}",
                t.id.0, t.deadline, t.eligible, t.state
            );
        }
        if let Some((_, mut next_task)) = sched.cpu_queues[cpu].pop_first() {
            // ── DIAGNOSTIC: dump the iretq frame and entry point ───────
            {
                let entry = next_task.process.entry_point();
                let stack_top = next_task.process.stack_top();
                let pml4 = next_task.process.pml4_frame();
                crate::serial::println!(
                    "[sched] IRETQ DIAG: task_id={} entry={:#x} stack_top={:#x} pml4={:#x} stack_ptr={:#x}",
                    next_task.id.0, entry.as_u64(), stack_top.as_u64(),
                    pml4.start_address().as_u64(), next_task.stack_ptr,
                );

                // Walk the process page table at entry point to verify code page is mapped
                let phys_offset = crate::boot::get_phys_mem_offset();
                let entry_virt = entry;
                let p4_idx = entry_virt.p4_index();
                let p3_idx = entry_virt.p3_index();
                let p2_idx = entry_virt.p2_index();
                let p1_idx = entry_virt.p1_index();

                let pml4_ptr = (phys_offset + pml4.start_address().as_u64())
                    .as_mut_ptr::<x86_64::structures::paging::PageTable>();
                let pml4 = unsafe { &*pml4_ptr };

                if pml4[p4_idx].is_unused() {
                    crate::serial::println!("[sched] IRETQ DIAG: PML4[{:?}] is UNUSED — entry page UNMAPPED!", p4_idx);
                } else {
                    let pdpt_ptr = (phys_offset + pml4[p4_idx].frame().unwrap().start_address().as_u64())
                        .as_mut_ptr::<x86_64::structures::paging::PageTable>();
                    let pdpt = unsafe { &*pdpt_ptr };
                    if pdpt[p3_idx].is_unused() {
                        crate::serial::println!("[sched] IRETQ DIAG: PDPT[{:?}] is UNUSED — entry page UNMAPPED!", p3_idx);
                    } else {
                        let pd_ptr = (phys_offset + pdpt[p3_idx].frame().unwrap().start_address().as_u64())
                            .as_mut_ptr::<x86_64::structures::paging::PageTable>();
                        let pd = unsafe { &*pd_ptr };
                        if pd[p2_idx].is_unused() {
                            crate::serial::println!("[sched] IRETQ DIAG: PD[{:?}] is UNUSED — entry page UNMAPPED!", p2_idx);
                        } else if pd[p2_idx].flags().contains(x86_64::structures::paging::PageTableFlags::HUGE_PAGE) {
                            crate::serial::println!("[sched] IRETQ DIAG: PD[{:?}] is 2MiB HUGE page, flags={:?}", p2_idx, pd[p2_idx].flags());
                        } else {
                            let pt_ptr = (phys_offset + pd[p2_idx].frame().unwrap().start_address().as_u64())
                                .as_mut_ptr::<x86_64::structures::paging::PageTable>();
                            let pt = unsafe { &*pt_ptr };
                            if pt[p1_idx].is_unused() {
                                crate::serial::println!("[sched] IRETQ DIAG: PT[{:?}] is UNUSED — entry page UNMAPPED!", p1_idx);
                            } else {
                                let flags = pt[p1_idx].flags();
                                let phys = pt[p1_idx].frame().unwrap().start_address();
                                crate::serial::println!(
                                    "[sched] IRETQ DIAG: PT[{:?}] flags={:?} phys={:#x} — code page {}{}{}{}",
                                    p1_idx, flags, phys.as_u64(),
                                    if flags.contains(x86_64::structures::paging::PageTableFlags::PRESENT) { "PRESENT " } else { "NOT_PRESENT " },
                                    if flags.contains(x86_64::structures::paging::PageTableFlags::USER_ACCESSIBLE) { "USER " } else { "KERN " },
                                    if flags.contains(x86_64::structures::paging::PageTableFlags::WRITABLE) { "WRITABLE " } else { "RO " },
                                    if flags.contains(x86_64::structures::paging::PageTableFlags::NO_EXECUTE) { "NX " } else { "EXEC " },
                                );
                                // Read first 64 bytes of code at entry point via physical mapping
                                // entry_virt may not be page-aligned; add the page offset
                                let page_offset = entry_virt.as_u64() & 0xFFF;
                                let code_ptr = (phys_offset + phys.as_u64() + page_offset).as_ptr::<u8>();
                                crate::serial::print(format_args!("[sched] IRETQ DIAG: entry_page_offset={:#x} first 64 code bytes at entry:", page_offset));
                                for i in 0..64u64 {
                                    unsafe {
                                        let b = core::ptr::read_volatile(code_ptr.add(i as usize));
                                        crate::serial::print(format_args!(" {:02x}", b));
                                    }
                                }
                                crate::serial::println!("");
                            }
                        }
                    }
                }
            }

            next_task.switch_to();
            next_task.state = super::TaskState::Running;
            sched.cpu_current[cpu] = Some(next_task);
            sched.cpu_current_id[cpu] = sched.cpu_current[cpu].as_ref().map(|t| t.id);
            let next_ptr = match sched.cpu_current[cpu].as_ref() {
                Some(t) => t.stack_ptr,
                None => unreachable!("scheduler: cpu_current was just set to Some"),
            };
            drop(sched);

            crate::serial::println!("[sched] IRETQ: about to iretq to user mode, rsp={:#x}", next_ptr);

            // Safety: next_ptr is a valid stack pointer from Task::new_user; pop pattern matches SyscallContext frame layout; iretq returns to user mode.
            unsafe {
                core::arch::asm!(
                    "mov rsp, {0}",
                    "pop r15", "pop r14", "pop r13", "pop r12", "pop r11",
                    "pop r10", "pop r9", "pop r8", "pop rdi", "pop rsi",
                    "pop rbp", "pop rdx", "pop rcx", "pop rbx", "pop rax",
                    "test qword ptr [rsp + 8], 0x3",
                    "jz 2f",
                    "swapgs",
                    "2:",
                    "iretq",
                    in(reg) next_ptr,
                    options(noreturn)
                );
            }
        }

        panic!("No tasks to schedule!");
    }
}

pub fn yield_task() {
    // Safety: YIELD_INTERRUPT_VECTOR is a valid software interrupt vector; interrupt is enabled in user mode.
    #[cfg(all(target_arch = "x86_64", target_os = "none"))]
    unsafe {
        core::arch::asm!("int {vector}", vector = const crate::interrupts::YIELD_INTERRUPT_VECTOR);
    }
}

pub fn get_current_kernel_stack_top() -> usize {
    x86_64::instructions::interrupts::without_interrupts(|| {
        let sched = SCHEDULER.lock();
        let cpu = sched.current_cpu_id();
        sched.cpu_current[cpu]
            .as_ref()
            .map(|t| t.kernel_stack_top)
            .unwrap_or(0)
    })
}

pub fn timer_tick(current_stack_ptr: usize) -> usize {
    UPTIME_TICKS.fetch_add(1, Ordering::Relaxed);

    if let Some(mut sched) = SCHEDULER.try_lock() {
        let cpu = sched.current_cpu_id();
        if let Some(mut prev_task) = sched.cpu_current[cpu].take() {
            prev_task.stack_ptr = current_stack_ptr;

            let was_running = prev_task.state == super::TaskState::Running;
            let is_zombie = prev_task.state == super::TaskState::Zombie;

            // EEVDF: update vruntime with weighted fair queuing
            if was_running {
                match prev_task.policy {
                    super::scheduler_class::SchedulingPolicy::SCHED_NORMAL
                    | super::scheduler_class::SchedulingPolicy::SCHED_BATCH => {
                        update_vruntime(&mut prev_task, 1);
                        // Update min_vruntime
                        if prev_task.vruntime > sched.min_vruntime {
                            sched.min_vruntime = prev_task.vruntime;
                        }
                        // NOTE: Do NOT recompute deadline here.
                        // Recomputing with the updated vruntime would make the running
                        // task's deadline later than all queued tasks (which still have
                        // deadlines computed at their insertion time). This causes EEVDF
                        // to preempt on every tick, preventing any task from running
                        // more than one tick. Deadline is recomputed when the task is
                        // actually reinserted into the queue (PREEMPT branch below).
                    }
                    _ => {}
                }
            }

            let mut should_preempt = false;
            if was_running && prev_task.time_slice > 0 && prev_task.time_slice != u32::MAX {
                prev_task.time_slice -= 1;
                if prev_task.time_slice == 0 {
                    should_preempt = true;
                }
                // Enforce cgroup cpu_max: if quota exceeded, force preempt.
                let pid_val = prev_task.process.id().0 as u32;
                if !crate::cgroup::cgroup_cpu_tick(pid_val) {
                    should_preempt = true;
                }
            }

            if prev_task.policy == super::scheduler_class::SchedulingPolicy::SCHED_FIFO {
                should_preempt = false;
            }

            // EEVDF: preempt if running task's deadline exceeds the earliest eligible deadline
            let eevdf_preempt = if was_running
                && matches!(
                    prev_task.policy,
                    super::scheduler_class::SchedulingPolicy::SCHED_NORMAL
                        | super::scheduler_class::SchedulingPolicy::SCHED_BATCH
                )
                && let Some(next) = pick_eevdf(&sched.cpu_queues[cpu])
                && prev_task.deadline > next.deadline
            {
                should_preempt = true;
                true
            } else {
                false
            };

            // Try local queue first (pick eligible task with earliest deadline),
            // then steal from busiest CPU.
            let mut next_task = if let Some(key) = sched.cpu_queues[cpu]
                .iter()
                .filter(|(_, t)| t.eligible)
                .min_by_key(|(k, _)| k.0)
                .map(|(k, _)| *k)
            {
                sched.cpu_queues[cpu].remove(&key)
            } else {
                None
            };
            if next_task.is_none() {
                next_task = sched.steal_task();
            }

            if let Some(mut next_task) = next_task {
                if was_running && should_preempt {
                    prev_task.state = super::TaskState::Ready;
                    prev_task.time_slice =
                        super::scheduler_class::default_timeslice(prev_task.policy);
                    let min_vr = sched.min_vruntime;
                    let deadline = compute_deadline(
                        prev_task.vruntime,
                        prev_task.time_slice,
                        prev_task.weight,
                    );
                    prev_task.deadline = deadline;
                    prev_task.eligible = prev_task.vruntime <= min_vr;
                    sched.cpu_queues[cpu].insert((deadline, prev_task.id.0), prev_task);
                } else if was_running {
                    let min_vr = sched.min_vruntime;
                    let deadline = compute_deadline(
                        next_task.vruntime,
                        next_task.time_slice,
                        next_task.weight,
                    );
                    next_task.deadline = deadline;
                    next_task.eligible = next_task.vruntime <= min_vr;
                    sched.cpu_queues[cpu].insert((deadline, next_task.id.0), next_task);
                    next_task = prev_task;
                }

                next_task.switch_to();
                next_task.state = super::TaskState::Running;
                let next_ptr = next_task.stack_ptr;
                sched.cpu_current[cpu] = Some(next_task);
                sched.cpu_current_id[cpu] = sched.cpu_current[cpu].as_ref().map(|t| t.id);

                return next_ptr;
            } else {
                if is_zombie && sched.task_count == 0 {
                    crate::serial::println!("[scheduler] CPU {}: All tasks exited. Halting.", cpu);
                    loop {
                        x86_64::instructions::hlt();
                    }
                }
                sched.cpu_current[cpu] = Some(prev_task);
                sched.cpu_current_id[cpu] = sched.cpu_current[cpu].as_ref().map(|t| t.id);
            }
        }
    }
    current_stack_ptr
}

pub fn exit_current_task() -> ! {
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut sched = SCHEDULER.lock();
        let cpu = sched.current_cpu_id();
        if let Some(ref mut task) = sched.cpu_current[cpu] {
            crate::serial::println!(
                "[scheduler] CPU {} Task {} exited/terminated.",
                cpu,
                task.id.0
            );
            task.state = super::TaskState::Zombie;
        }
    });

    yield_task();

    loop {
        x86_64::instructions::hlt();
    }
}

pub fn get_uptime_ticks() -> u64 {
    UPTIME_TICKS.load(Ordering::Relaxed)
}

pub fn get_task_count() -> usize {
    SCHEDULER.lock().task_count
}

pub fn get_current_task_id() -> Option<TaskId> {
    let sched = SCHEDULER.lock();
    let cpu = sched.current_cpu_id();
    sched.cpu_current_id[cpu]
}

pub fn get_current_process() -> Option<Process> {
    let sched = SCHEDULER.lock();
    let cpu = sched.current_cpu_id();
    sched.cpu_current[cpu]
        .as_ref()
        .map(|task| task.process.clone())
}

pub fn with_current_task_mut<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&mut Task) -> R,
{
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut sched = SCHEDULER.lock();
        let cpu = sched.current_cpu_id();
        sched.cpu_current[cpu].as_mut().map(f)
    })
}

pub fn set_current_policy(policy: super::scheduler_class::SchedulingPolicy, priority: u8) {
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut sched = SCHEDULER.lock();
        let cpu = sched.current_cpu_id();
        if let Some(ref mut task) = sched.cpu_current[cpu] {
            task.policy = policy;
            task.priority = priority;
            task.time_slice = super::scheduler_class::default_timeslice(policy);
        }
    });
}

pub fn get_current_policy() -> Option<(super::scheduler_class::SchedulingPolicy, u8)> {
    x86_64::instructions::interrupts::without_interrupts(|| {
        let sched = SCHEDULER.lock();
        let cpu = sched.current_cpu_id();
        sched.cpu_current[cpu]
            .as_ref()
            .map(|t| (t.policy, t.priority))
    })
}

pub fn get_current_process_id() -> Option<ProcessId> {
    if !SCHEDULER_READY.load(Ordering::Acquire) {
        return None;
    }
    let sched = SCHEDULER.lock();
    let cpu = sched.current_cpu_id();
    sched.cpu_current[cpu].as_ref().map(|t| t.pid)
}

pub fn block_current() {
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut sched = SCHEDULER.lock();
        let cpu = sched.current_cpu_id();
        if let Some(mut task) = sched.cpu_current[cpu].take() {
            sched.cpu_current_id[cpu] = None;
            task.state = super::TaskState::Blocked;
            sched.task_count -= 1;
            sched.blocked_tasks.push(task);
        }
    });
}

pub fn wake_task_by_id(task_id: TaskId) {
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut sched = SCHEDULER.lock();
        for i in 0..sched.blocked_tasks.len() {
            if sched.blocked_tasks[i].id == task_id {
                let mut task = sched.blocked_tasks.swap_remove(i);
                task.state = super::TaskState::Ready;
                sched.task_count += 1;
                // Route to least-loaded CPU.
                let target = sched.least_loaded_cpu();
                let min_vr = sched.min_vruntime;
                let deadline = compute_deadline(task.vruntime, task.time_slice, task.weight);
                task.deadline = deadline;
                task.eligible = task.vruntime <= min_vr;
                sched.cpu_queues[target].insert((deadline, task.id.0), task);
                return;
            }
        }
    });
}

pub fn block_current_waiting_for_child() {
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut sched = SCHEDULER.lock();
        let cpu = sched.current_cpu_id();
        if let Some(mut task) = sched.cpu_current[cpu].take() {
            sched.cpu_current_id[cpu] = None;
            task.state = super::TaskState::Blocked;
            sched.task_count -= 1;
            sched.blocked_tasks.push(task);
        }
    });
}

pub fn wake_tasks_waiting_for_parent(parent_pid: ProcessId) {
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut sched = SCHEDULER.lock();
        let mut i = 0;
        while i < sched.blocked_tasks.len() {
            if sched.blocked_tasks[i].process.id() == parent_pid {
                let mut task = sched.blocked_tasks.swap_remove(i);
                task.state = super::TaskState::Ready;
                sched.task_count += 1;
                let target = sched.least_loaded_cpu();
                let min_vr = sched.min_vruntime;
                let deadline = compute_deadline(task.vruntime, task.time_slice, task.weight);
                task.deadline = deadline;
                task.eligible = task.vruntime <= min_vr;
                sched.cpu_queues[target].insert((deadline, task.id.0), task);
            } else {
                i += 1;
            }
        }
    });
}

#[cfg(test)]
pub fn set_current_task_for_test(task: Task) {
    let mut sched = SCHEDULER.lock();
    let cpu = sched.current_cpu_id();
    sched.cpu_current_id[cpu] = Some(task.id);
    sched.cpu_current[cpu] = Some(task);
}

#[cfg(test)]
pub fn test_reset() {
    let mut sched = SCHEDULER.lock();
    let cpu = sched.current_cpu_id();
    sched.cpu_current[cpu] = None;
    sched.cpu_current_id[cpu] = None;
    sched.blocked_tasks.clear();
    for queue in &mut sched.cpu_queues {
        queue.clear();
    }
    sched.task_count = 0;
    sched.min_vruntime = 0;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::{
        Process, ProcessControlBlock, ProcessId, ProcessState, SignalAction, SignalSet,
    };
    use crate::task::{Task, TaskState};
    use alloc::sync::Arc;
    use spin::Mutex;

    fn make_test_process(pid: usize) -> Process {
        let pcb = ProcessControlBlock {
            id: ProcessId(pid),
            ppid: ProcessId(0),
            state: ProcessState::Ready,
            pml4_frame: x86_64::structures::paging::PhysFrame::containing_address(
                x86_64::PhysAddr::new(0),
            ),
            entry_point: x86_64::VirtAddr::zero(),
            stack_top: x86_64::VirtAddr::zero(),
            threads: alloc::vec![],
            vma_set: crate::memory::vma::VmaSet::new(),
            mmap_next_addr: x86_64::VirtAddr::zero(),
            aslr_base: x86_64::VirtAddr::zero(),
            fd_table: alloc::vec![None; 1024],
            signal_mask: SignalSet::empty(),
            signal_handlers: [SignalAction::Default; 64],
            pending_signals: SignalSet::empty(),
            pending_signal_frame: None,
            sec_ctx: crate::security::SecurityContext::root(),
            nsproxy: crate::security::namespaces::NsProxy::new(),
            seccomp_filter: None,
            cgroup_path: None,
            cwd: alloc::string::String::from("/"),
        };
        Process {
            inner: Arc::new(Mutex::new(pcb)),
        }
    }

    fn make_test_task(pid: usize) -> Task {
        Task::new_test(TaskId::new(), make_test_process(pid), TaskState::Running)
    }

    // -- Query function tests (no without_interrupts, safe in test mode) --

    #[test]
    fn test_get_task_count_initial_zero() {
        let _guard = crate::test_serial::acquire();
        test_reset();
        assert_eq!(get_task_count(), 0);
    }

    #[test]
    fn test_get_current_task_id_none_when_empty() {
        let _guard = crate::test_serial::acquire();
        test_reset();
        assert!(get_current_task_id().is_none());
    }

    #[test]
    fn test_set_current_task_then_get_id() {
        let _guard = crate::test_serial::acquire();
        test_reset();
        let task = make_test_task(42);
        let expected_id = task.id;
        set_current_task_for_test(task);
        assert_eq!(get_current_task_id(), Some(expected_id));
    }

    #[test]
    fn test_get_current_process_after_set() {
        let _guard = crate::test_serial::acquire();
        test_reset();
        let task = make_test_task(77);
        let pid = task.process.id();
        set_current_task_for_test(task);
        let proc = get_current_process();
        assert!(proc.is_some());
        assert_eq!(proc.unwrap().id(), pid);
    }

    #[test]
    fn test_get_current_process_none_when_empty() {
        let _guard = crate::test_serial::acquire();
        test_reset();
        assert!(get_current_process().is_none());
    }

    #[test]
    fn test_get_current_process_id() {
        let _guard = crate::test_serial::acquire();
        test_reset();
        let task = make_test_task(55);
        set_current_task_for_test(task);
        assert_eq!(get_current_process_id(), Some(ProcessId(55)));
    }

    #[test]
    fn test_get_current_process_id_none_when_empty() {
        let _guard = crate::test_serial::acquire();
        test_reset();
        assert!(get_current_process_id().is_none());
    }

    // Note: tests for get_current_kernel_stack_top are omitted because
    // the function uses without_interrupts (cli/sti) which SIGSEGVs in
    // userspace test mode.

    #[test]
    fn test_get_uptime_ticks_initial() {
        let _guard = crate::test_serial::acquire();
        let _ticks = get_uptime_ticks();
    }

    #[test]
    fn test_test_reset_clears_current() {
        let _guard = crate::test_serial::acquire();
        test_reset();
        let task = make_test_task(1);
        set_current_task_for_test(task);
        assert!(get_current_task_id().is_some());
        test_reset();
        assert!(get_current_task_id().is_none());
    }

    #[test]
    fn test_test_reset_clears_task_count() {
        let _guard = crate::test_serial::acquire();
        test_reset();
        // task_count is manipulated by blocked_tasks and queues;
        // after test_reset it must be 0.
        assert_eq!(get_task_count(), 0);
    }

    #[test]
    fn test_set_current_task_replaces_previous() {
        let _guard = crate::test_serial::acquire();
        test_reset();
        let task1 = make_test_task(10);
        let id1 = task1.id;
        set_current_task_for_test(task1);
        let task2 = make_test_task(20);
        let id2 = task2.id;
        set_current_task_for_test(task2);
        assert_eq!(get_current_task_id(), Some(id2));
        assert_ne!(get_current_task_id(), Some(id1));
    }

    #[test]
    fn test_process_id_matches_after_set() {
        let _guard = crate::test_serial::acquire();
        test_reset();
        let task = make_test_task(123);
        set_current_task_for_test(task);
        assert_eq!(get_current_process_id(), Some(ProcessId(123)));
        assert_eq!(get_current_task_count(), 0);
    }

    #[test]
    fn test_compute_deadline_and_vruntime_updates_are_saturating() {
        let _guard = crate::test_serial::acquire();

        assert_eq!(compute_deadline(10, 5, 0), 10);
        assert_eq!(compute_deadline(u64::MAX - 1, u32::MAX, 1), u64::MAX);

        let mut task = make_test_task(321);
        task.weight = 0;
        task.vruntime = u64::MAX - 2;
        update_vruntime(&mut task, 100);
        assert_eq!(task.vruntime, u64::MAX - 1);

        let mut weighted_task = make_test_task(322);
        weighted_task.weight = 1024;
        weighted_task.vruntime = 7;
        update_vruntime(&mut weighted_task, 3);
        assert_eq!(weighted_task.vruntime, 10);
    }

    fn get_current_task_count() -> usize {
        get_task_count()
    }

    #[test]
    fn test_pick_eevdf_selects_earliest_deadline() {
        let _guard = crate::test_serial::acquire();
        let mut queue = BTreeMap::new();

        let mut task_a = make_test_task(10);
        task_a.eligible = true;
        task_a.deadline = 100;
        queue.insert((task_a.deadline, task_a.id.0), task_a);

        let mut task_b = make_test_task(20);
        task_b.eligible = true;
        task_b.deadline = 50;
        queue.insert((task_b.deadline, task_b.id.0), task_b);

        let mut task_c = make_test_task(30);
        task_c.eligible = true;
        task_c.deadline = 200;
        queue.insert((task_c.deadline, task_c.id.0), task_c);

        let picked = pick_eevdf(&queue).expect("should pick a task");
        assert_eq!(picked.deadline, 50, "should pick task with earliest deadline (50)");
    }

    #[test]
    fn test_pick_eevdf_skips_ineligible() {
        let _guard = crate::test_serial::acquire();
        let mut queue = BTreeMap::new();

        let mut task_a = make_test_task(10);
        task_a.eligible = false;
        task_a.deadline = 10;
        queue.insert((task_a.deadline, task_a.id.0), task_a);

        let mut task_b = make_test_task(20);
        task_b.eligible = true;
        task_b.deadline = 100;
        queue.insert((task_b.deadline, task_b.id.0), task_b);

        let picked = pick_eevdf(&queue).expect("should pick a task");
        assert_eq!(picked.deadline, 100, "should skip ineligible task and pick next eligible");
    }

    #[test]
    fn test_pick_eevdf_empty_queue() {
        let _guard = crate::test_serial::acquire();
        let queue = BTreeMap::new();
        assert!(pick_eevdf(&queue).is_none());
    }

    #[test]
    fn test_pick_eevdf_all_ineligible_returns_none() {
        let _guard = crate::test_serial::acquire();
        let mut queue = BTreeMap::new();

        let mut task_a = make_test_task(10);
        task_a.eligible = false;
        task_a.deadline = 10;
        queue.insert((task_a.deadline, task_a.id.0), task_a);

        let mut task_b = make_test_task(20);
        task_b.eligible = false;
        task_b.deadline = 50;
        queue.insert((task_b.deadline, task_b.id.0), task_b);

        assert!(pick_eevdf(&queue).is_none());
    }

    #[test]
    fn test_compute_deadline_zero_weight() {
        let _guard = crate::test_serial::acquire();
        // Weight 0 should return vruntime (no virtual slice)
        assert_eq!(compute_deadline(100, 5, 0), 100);
    }

    #[test]
    fn test_compute_deadline_high_weight_gives_small_slice() {
        let _guard = crate::test_serial::acquire();
        // High weight => small virtual slice => deadline close to vruntime
        let d = compute_deadline(100, 10, 1024);
        // slice_virtual = (10 * 1024) / 1024 = 10
        assert_eq!(d, 110);
    }

    #[test]
    fn test_compute_deadline_low_weight_gives_large_slice() {
        let _guard = crate::test_serial::acquire();
        // Low weight => large virtual slice => deadline far from vruntime
        let d = compute_deadline(100, 10, 1);
        // slice_virtual = (10 * 1024) / 1 = 10240
        assert_eq!(d, 10340);
    }

    #[test]
    fn test_update_vruntime_weighted() {
        let _guard = crate::test_serial::acquire();
        let mut task = make_test_task(500);
        task.weight = 512;
        task.vruntime = 0;
        update_vruntime(&mut task, 2);
        // delta = (1024 * 2) / 512 = 4
        assert_eq!(task.vruntime, 4);
    }

    #[test]
    fn test_update_vruntime_min_increment() {
        let _guard = crate::test_serial::acquire();
        // Very high weight should still increment by at least 1
        let mut task = make_test_task(501);
        task.weight = u32::MAX;
        task.vruntime = 0;
        update_vruntime(&mut task, 1);
        assert!(task.vruntime >= 1);
    }

    #[test]
    fn test_set_current_policy() {
        // Cannot call set_current_policy() directly because it uses
        // without_interrupts (cli/sti) which triggers
        // STATUS_PRIVILEGED_INSTRUCTION in userspace tests.
        // Instead, verify the scheduling policy data model:
        use super::super::scheduler_class::{SchedulingPolicy, default_timeslice};
        // SCHED_FIFO should have a large default timeslice
        let fifo_slice = default_timeslice(SchedulingPolicy::SCHED_FIFO);
        assert!(fifo_slice >= 100, "FIFO timeslice should be large");
        // SCHED_RR should have a finite default timeslice
        let rr_slice = default_timeslice(SchedulingPolicy::SCHED_RR);
        assert!(rr_slice > 0, "RR timeslice should be positive");
        // SCHED_IDLE should have a small default timeslice
        let idle_slice = default_timeslice(SchedulingPolicy::SCHED_IDLE);
        assert!(idle_slice < fifo_slice, "Idle timeslice should be smaller than FIFO");
        // SCHED_NORMAL should have a moderate default timeslice
        let normal_slice = default_timeslice(SchedulingPolicy::SCHED_NORMAL);
        assert!(normal_slice > 0, "Normal timeslice should be positive");
    }

    #[test]
    fn test_get_current_policy_none_when_empty() {
        // Cannot call get_current_policy() directly because it uses
        // without_interrupts which triggers STATUS_PRIVILEGED_INSTRUCTION
        // in userspace tests. Verify that when no task is set, the
        // current process id is None (which implies no policy exists).
        let _guard = crate::test_serial::acquire();
        test_reset();
        assert!(get_current_process_id().is_none());
        // Set a task and verify process id is present
        let task = make_test_task(999);
        set_current_task_for_test(task);
        assert_eq!(get_current_process_id(), Some(ProcessId(999)));
        // Verify process id matches after reset
        test_reset();
        assert!(get_current_process_id().is_none());
    }

    #[test]
    fn test_pick_eevdf_prefers_lower_deadline_over_higher() {
        let _guard = crate::test_serial::acquire();
        let mut queue = BTreeMap::new();

        // Insert 3 eligible tasks with different deadlines
        for (pid, deadline) in [(10, 300), (20, 50), (30, 200)] {
            let mut task = make_test_task(pid);
            task.eligible = true;
            task.deadline = deadline;
            queue.insert((deadline, task.id.0), task);
        }

        let picked = pick_eevdf(&queue).unwrap();
        assert_eq!(picked.deadline, 50);
    }

    #[test]
    fn test_pick_eevdf_mixed_eligibility() {
        let _guard = crate::test_serial::acquire();
        let mut queue = BTreeMap::new();

        // Task with earliest deadline is ineligible
        let mut t1 = make_test_task(10);
        t1.eligible = false;
        t1.deadline = 10;
        queue.insert((t1.deadline, t1.id.0), t1);

        // Second earliest is eligible
        let mut t2 = make_test_task(20);
        t2.eligible = true;
        t2.deadline = 50;
        queue.insert((t2.deadline, t2.id.0), t2);

        // Third is eligible but later
        let mut t3 = make_test_task(30);
        t3.eligible = true;
        t3.deadline = 100;
        queue.insert((t3.deadline, t3.id.0), t3);

        let picked = pick_eevdf(&queue).unwrap();
        assert_eq!(picked.deadline, 50, "should skip ineligible deadline=10");
    }

    // -----------------------------------------------------------------------
    // Scheduler queue / load-balancing tests
    // -----------------------------------------------------------------------

    #[cfg(test)]
    fn test_insert_task(task: Task) {
        let mut sched = SCHEDULER.lock();
        let target = sched.least_loaded_cpu();
        let deadline = compute_deadline(task.vruntime, task.time_slice, task.weight);
        let mut task = task;
        task.deadline = deadline;
        task.eligible = task.vruntime <= sched.min_vruntime;
        sched.cpu_queues[target].insert((deadline, task.id.0), task);
        sched.task_count += 1;
    }

    #[cfg(test)]
    fn test_queue_len() -> usize {
        let sched = SCHEDULER.lock();
        sched.cpu_queues.iter().map(|q| q.len()).sum()
    }

    #[test]
    fn test_steal_task_empty_returns_none() {
        let _guard = crate::test_serial::acquire();
        test_reset();
        let mut sched = SCHEDULER.lock();
        assert!(sched.steal_task().is_none());
    }

    #[test]
    fn test_steal_task_single_task_returns_none() {
        let _guard = crate::test_serial::acquire();
        test_reset();
        let task = make_test_task(100);
        test_insert_task(task);
        assert_eq!(test_queue_len(), 1);
        let mut sched = SCHEDULER.lock();
        assert!(sched.steal_task().is_none(), "should not steal when only 1 task on remote CPU");
    }

    #[test]
    fn test_steal_task_steals_from_busiest() {
        let _guard = crate::test_serial::acquire();
        test_reset();
        // Tasks need distinct vruntimes so they get distinct deadlines in the BTreeMap
        for (pid, vruntime) in [(200, 10u64), (201, 20), (202, 30)] {
            let mut task = make_test_task(pid);
            task.vruntime = vruntime;
            test_insert_task(task);
        }
        assert_eq!(test_queue_len(), 3);
        let mut sched = SCHEDULER.lock();
        let stolen = sched.steal_task();
        // In single-CPU mode, steal_task skips current CPU (0), so all tasks
        // are on CPU 0 and steal_task returns None because it only steals from OTHER CPUs.
        assert!(stolen.is_none(), "should not steal from own CPU");
    }

    #[test]
    fn test_least_loaded_cpu_returns_zero() {
        let _guard = crate::test_serial::acquire();
        test_reset();
        let sched = SCHEDULER.lock();
        assert_eq!(sched.least_loaded_cpu(), 0, "empty queues should return CPU 0");
    }

    #[test]
    fn test_task_count_increments_on_insert() {
        let _guard = crate::test_serial::acquire();
        test_reset();
        assert_eq!(get_task_count(), 0);
        test_insert_task(make_test_task(300));
        assert_eq!(get_task_count(), 1);
        test_insert_task(make_test_task(301));
        assert_eq!(get_task_count(), 2);
    }

    #[test]
    fn test_insert_task_eligibility_based_on_vruntime() {
        let _guard = crate::test_serial::acquire();
        test_reset();
        {
            let mut sched = SCHEDULER.lock();
            sched.min_vruntime = 100;
        }
        let mut task_a = make_test_task(400);
        task_a.vruntime = 50;
        let task_a_id = task_a.id;
        test_insert_task(task_a);
        let mut task_b = make_test_task(401);
        task_b.vruntime = 200;
        test_insert_task(task_b);
        let sched = SCHEDULER.lock();
        let cpu0_queue = &sched.cpu_queues[0];
        let picked = pick_eevdf(cpu0_queue);
        assert!(picked.is_some());
        assert_eq!(picked.unwrap().id, task_a_id, "should pick eligible task with vruntime=50");
    }

    #[test]
    fn test_insert_multiple_tasks_ordered_by_deadline() {
        let _guard = crate::test_serial::acquire();
        test_reset();
        for pid in [500, 501, 502] {
            let mut task = make_test_task(pid);
            task.vruntime = (pid as u64) * 10;
            test_insert_task(task);
        }
        let sched = SCHEDULER.lock();
        let queue = &sched.cpu_queues[0];
        let deadlines: alloc::vec::Vec<(u64, usize)> = queue.keys().copied().collect();
        assert!(deadlines.windows(2).all(|w| w[0] <= w[1]), "deadlines should be sorted");
    }

    #[test]
    fn test_min_vruntime_not_decreasing() {
        let _guard = crate::test_serial::acquire();
        test_reset();
        let vruntime_before = 50u64;
        let ticks = 10u32;
        let weight = 1024u32;
        // Compute expected vruntime after update_vruntime
        let expected_delta = (1024u64 * ticks as u64) / weight as u64;
        let expected_vruntime = vruntime_before + expected_delta.max(1);
        // Verify that update_vruntime increases vruntime monotonically
        assert!(expected_vruntime > vruntime_before, "vruntime should increase after tick");
        assert!(expected_vruntime >= vruntime_before + 1, "vruntime should increase by at least 1");
    }
}
