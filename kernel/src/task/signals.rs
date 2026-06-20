use alloc::sync::Arc;
use spin::Mutex;

use crate::arch::x86_64::syscall_arch::SyscallFrame;
use crate::process::{ProcessControlBlock, ProcessId, SignalAction};
use crate::task::scheduler;

// ---------------------------------------------------------------------------
// POSIX signal numbers (1–31)
// ---------------------------------------------------------------------------

pub const SIGHUP: u8 = 1;
pub const SIGINT: u8 = 2;
pub const SIGQUIT: u8 = 3;
pub const SIGILL: u8 = 4;
pub const SIGTRAP: u8 = 5;
pub const SIGABRT: u8 = 6;
pub const SIGBUS: u8 = 7;
pub const SIGFPE: u8 = 8;
pub const SIGKILL: u8 = 9;
pub const SIGUSR1: u8 = 10;
pub const SIGSEGV: u8 = 11;
pub const SIGUSR2: u8 = 12;
pub const SIGPIPE: u8 = 13;
pub const SIGALRM: u8 = 14;
pub const SIGTERM: u8 = 15;
pub const SIGSTKFLT: u8 = 16;
pub const SIGCHLD: u8 = 17;
pub const SIGCONT: u8 = 18;
pub const SIGSTOP: u8 = 19;
pub const SIGTSTP: u8 = 20;
pub const SIGTTIN: u8 = 21;
pub const SIGTTOU: u8 = 22;
pub const SIGURG: u8 = 23;
pub const SIGXCPU: u8 = 24;
pub const SIGXFSZ: u8 = 25;
pub const SIGVTALRM: u8 = 26;
pub const SIGPROF: u8 = 27;
pub const SIGWINCH: u8 = 28;
pub const SIGIO: u8 = 29;
pub const SIGPWR: u8 = 30;
pub const SIGSYS: u8 = 31;

/// Signals that terminate the process by default.
fn default_terminates(sig: u8) -> bool {
    matches!(
        sig,
        SIGHUP | SIGINT | SIGQUIT | SIGILL | SIGTRAP | SIGABRT | SIGBUS
            | SIGFPE | SIGKILL | SIGUSR1 | SIGSEGV | SIGUSR2 | SIGPIPE
            | SIGALRM | SIGTERM | SIGSTKFLT | SIGXCPU | SIGXFSZ
            | SIGVTALRM | SIGPROF | SIGIO | SIGPWR | SIGSYS
    )
}

/// Signals that are ignored by default.
fn default_ignores(sig: u8) -> bool {
    matches!(sig, SIGCHLD | SIGURG | SIGWINCH)
}

/// Signals that stop the process by default.
fn default_stops(sig: u8) -> bool {
    matches!(sig, SIGSTOP | SIGTSTP | SIGTTIN | SIGTTOU)
}

/// Signals that continue a stopped process by default.
fn default_continues(sig: u8) -> bool {
    matches!(sig, SIGCONT)
}

// ---------------------------------------------------------------------------
// Signal frame — saved register state on the user stack
// ---------------------------------------------------------------------------

/// Layout of saved context pushed onto the user stack before invoking a
/// signal handler.  The kernel stores this at `user_rsp - sizeof(SignalFrame)`
/// and sets the handler's RSP to point to it.
#[repr(C)]
pub struct SignalFrame {
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rbp: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rbx: u64,
    pub rax: u64,
    pub user_rip: u64,
    pub user_cs: u64,
    pub user_rflags: u64,
    pub user_rsp: u64,
    pub user_ss: u64,
    pub sig_num: u64,
}

// ---------------------------------------------------------------------------
// Signal delivery
// ---------------------------------------------------------------------------

/// Perform the default action for `sig` on `proc`.
fn default_action(sig: u8, proc: &Arc<Mutex<ProcessControlBlock>>) {
    if default_terminates(sig) || sig == SIGKILL {
        // Terminate the process — set zombie and switch away.
        let ppid;
        {
            let mut inner = proc.lock();
            ppid = inner.ppid;
            for slot in inner.fd_table.iter_mut() {
                *slot = None;
            }
            inner.state = crate::process::ProcessState::Zombie {
                exit_code: 128 + sig as i32,
            };
            inner.vma_set = crate::memory::vma::VmaSet::new();
            let my_pid = inner.id;
            // Reparent children to init.
            let table = crate::process::PROCESS_TABLE.lock();
            for (_pid, pcb_arc) in table.iter() {
                let mut pcb = pcb_arc.lock();
                if pcb.ppid == my_pid {
                    pcb.ppid = crate::process::ProcessId(1);
                }
            }
            drop(table);
        }
        // Deliver SIGCHLD to parent and wake waiters.
        {
            let table = crate::process::PROCESS_TABLE.lock();
            if let Some(parent_pcb_arc) = table.get(&ppid) {
                let mut parent = parent_pcb_arc.lock();
                parent.pending_signals.insert(SIGCHLD);
            }
        }
        scheduler::wake_tasks_waiting_for_parent(ppid);
        scheduler::exit_current_task();
    } else if default_stops(sig) {
        let mut inner = proc.lock();
        inner.state = crate::process::ProcessState::Stopped;
        inner.pending_signals.remove(sig);
    } else if default_continues(sig) {
        let mut inner = proc.lock();
        if matches!(inner.state, crate::process::ProcessState::Stopped) {
            inner.state = crate::process::ProcessState::Ready;
        }
        inner.pending_signals.remove(sig);
    } else if default_ignores(sig) {
        proc.lock().pending_signals.remove(sig);
    }
}

/// Check for pending signals on the current process and deliver one if
/// possible.  Modifies `frame` in place so that `iretq` returns to the
/// signal handler instead of the original instruction.
pub fn check_pending_signals(frame: &mut SyscallFrame) {
    let current = match scheduler::get_current_process() {
        Some(p) => p,
        None => return,
    };

    let sig_num = {
        let inner = current.inner.lock();
        let unblocked = inner.pending_signals.0 & !inner.signal_mask.0;
        if unblocked == 0 {
            return;
        }
        // Pick the lowest-numbered unblocked signal.
        let sig = unblocked.trailing_zeros() as u8;
        // SIGKILL and SIGSTOP can't be caught, ignored, or masked.
        if sig == SIGKILL || sig == SIGSTOP {
            drop(inner);
            default_action(sig, &current.inner);
            return;
        }
        sig
    };

    let action = {
        let inner = current.inner.lock();
        inner.signal_handlers[sig_num as usize]
    };

    match action {
        SignalAction::Default => {
            default_action(sig_num, &current.inner);
        }
        SignalAction::Ignore => {
            current.inner.lock().pending_signals.remove(sig_num);
        }
        SignalAction::Handler(addr) => {
            // Remove the signal from pending.
            current.inner.lock().pending_signals.remove(sig_num);

            let current_rsp = frame.user_rsp;

            // Build the SignalFrame on the user stack.
            let sig_frame = SignalFrame {
                r15: frame.r15,
                r14: frame.r14,
                r13: frame.r13,
                r12: frame.r12,
                r11: frame.r11,
                r10: frame.r10,
                r9: frame.r9,
                r8: frame.r8,
                rdi: frame.rdi,
                rsi: frame.rsi,
                rbp: frame.rbp,
                rdx: frame.rdx,
                rcx: frame.rcx,
                rbx: frame.rbx,
                rax: frame.rax,
                user_rip: frame.user_rip,
                user_cs: frame.user_cs,
                user_rflags: frame.user_rflags,
                user_rsp: frame.user_rsp,
                user_ss: frame.user_ss,
                sig_num: sig_num as u64,
            };

            let sig_frame_size = core::mem::size_of::<SignalFrame>() as u64;
            // Align down to 16 bytes.
            let frame_addr = (current_rsp - sig_frame_size) & !15u64;

            // Write the signal frame to user space.
            unsafe {
                (frame_addr as *mut SignalFrame).write(sig_frame);
            }

            // Store the frame address so sigreturn can find it.
            current.inner.lock().pending_signal_frame = Some(frame_addr);

            // Modify the return frame to jump to the handler.
            frame.user_rip = addr;
            frame.user_rsp = frame_addr;
            frame.rdi = sig_num as u64;
        }
    }
}

/// Restore register state from the pending SignalFrame on sigreturn.
pub fn handle_sigreturn_with_frame(frame: &mut SyscallFrame) -> u64 {
    let current = match scheduler::get_current_process() {
        Some(p) => p,
        None => return 0,
    };

    let (sig_frame, have_frame) = {
        let inner = current.inner.lock();
        (inner.pending_signal_frame, inner.pending_signal_frame.is_some())
    };

    if !have_frame {
        return 0;
    }

    if let Some(addr) = sig_frame {
        // Read the signal frame from user space.
        let saved: SignalFrame = unsafe { (addr as *const SignalFrame).read() };

        // Restore all registers.
        frame.r15 = saved.r15;
        frame.r14 = saved.r14;
        frame.r13 = saved.r13;
        frame.r12 = saved.r12;
        frame.r11 = saved.r11;
        frame.r10 = saved.r10;
        frame.r9 = saved.r9;
        frame.r8 = saved.r8;
        frame.rdi = saved.rdi;
        frame.rsi = saved.rsi;
        frame.rbp = saved.rbp;
        frame.rdx = saved.rdx;
        frame.rcx = saved.rcx;
        frame.rbx = saved.rbx;
        frame.rax = saved.rax;
        frame.user_rip = saved.user_rip;
        frame.user_cs = saved.user_cs;
        frame.user_rflags = saved.user_rflags;
        frame.user_rsp = saved.user_rsp;
        frame.user_ss = saved.user_ss;

        // Clear the pending frame.
        current.inner.lock().pending_signal_frame = None;
    }

    0
}

/// Send signal `sig` to process `pid`.  Returns true if the process was found.
pub fn send_signal(pid: ProcessId, sig: u8) -> bool {
    let table = crate::process::PROCESS_TABLE.lock();
    if let Some(pcb) = table.get(&pid) {
        pcb.lock().pending_signals.insert(sig);
        true
    } else {
        false
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::{Process, ProcessControlBlock, ProcessId, SignalAction};
    use crate::task::{Task, TaskId};

    struct TestEnv {
        process: Process,
        stack_layout: core::alloc::Layout,
        stack_ptr: u64,
    }

    fn setup_test_env() -> TestEnv {
        scheduler::test_reset();
        let stack_layout = core::alloc::Layout::from_size_align(4096, 16).unwrap();
        let stack_ptr = unsafe { std::alloc::alloc(stack_layout) } as u64;

        let pcb = ProcessControlBlock {
            id: ProcessId(999),
            ppid: ProcessId(1),
            state: crate::process::ProcessState::Ready,
            pml4_frame: x86_64::structures::paging::PhysFrame::containing_address(
                x86_64::PhysAddr::new(0)),
            entry_point: x86_64::VirtAddr::zero(),
            stack_top: x86_64::VirtAddr::zero(),
            threads: alloc::vec![],
            vma_set: crate::memory::vma::VmaSet::new(),
            mmap_next_addr: x86_64::VirtAddr::zero(),
            aslr_base: x86_64::VirtAddr::zero(),
            fd_table: alloc::vec![None; 1024],
            signal_mask: crate::process::SignalSet::empty(),
            signal_handlers: [SignalAction::Default; 64],
            pending_signals: crate::process::SignalSet::empty(),
            pending_signal_frame: None,
            sec_ctx: crate::security::SecurityContext::root(),
        };
        let process = Process {
            inner: Arc::new(Mutex::new(pcb)),
        };
        let task = Task {
            id: TaskId::new(),
            stack_ptr: 0,
            kernel_stack_top: 0,
            process: process.clone(),
            state: crate::task::TaskState::Running,
        };
        scheduler::set_current_task_for_test(task);
        TestEnv { process, stack_layout, stack_ptr }
    }

    fn make_frame(env: &TestEnv) -> SyscallFrame {
        SyscallFrame {
            r15: 0, r14: 0, r13: 0, r12: 0,
            r11: 0, r10: 0, r9: 0, r8: 0,
            rdi: 0, rsi: 0, rbp: 0, rdx: 0,
            rcx: 0, rbx: 0, rax: 0,
            user_rip: 0x400000,
            user_cs: 0x2b,
            user_rflags: 0x202,
            user_rsp: env.stack_ptr + 2048,
            user_ss: 0x23,
        }
    }

    fn cleanup(env: TestEnv) {
        unsafe { std::alloc::dealloc(env.stack_ptr as *mut u8, env.stack_layout); }
        scheduler::test_reset();
    }

    // ------------------------------------------------------------------
    // Property 19 — Signal Handler Delivery
    // ------------------------------------------------------------------
    //
    // For any handler address and signal number:
    //   1. Registering a handler via the signal_handlers array stores it.
    //   2. Pending the signal and calling check_pending_signals modifies
    //      the frame: user_rip → handler, rdi → signal number.
    //   3. sigreturn restores the original frame.

    proptest::proptest! {
        #[test]
        fn signal_handler_delivery(
            sig_num in 1u8..=31,
            handler_addr in 0x1_0000_0000u64..0x2_0000_0000u64,
        ) {
            let _guard = crate::test_serial::acquire();
            if sig_num == SIGKILL || sig_num == SIGSTOP {
                return Ok(());
            }

            let env = setup_test_env();
            {
                let mut inner = env.process.inner.lock();
                inner.signal_handlers[sig_num as usize] = SignalAction::Handler(handler_addr);
                inner.pending_signals.insert(sig_num);
            }

            let mut frame = make_frame(&env);
            let original_rip = frame.user_rip;
            let original_rsp = frame.user_rsp;
            let original_rdi = frame.rdi;

            check_pending_signals(&mut frame);

            {
                let inner = env.process.inner.lock();
                assert!(!inner.pending_signals.contains(sig_num),
                    "signal {} should be cleared after delivery", sig_num);
                assert!(inner.pending_signal_frame.is_some(),
                    "pending_signal_frame must be set");
            }

            // Frame should be redirected to the handler.
            assert_eq!(frame.user_rip, handler_addr,
                "RIP must be set to handler address");
            assert_eq!(frame.rdi, sig_num as u64,
                "RDI must be set to signal number");
            assert!(frame.user_rsp < original_rsp,
                "RSP must be decremented for signal frame");

            // Now test sigreturn restores the original context.
            handle_sigreturn_with_frame(&mut frame);
            assert_eq!(frame.user_rip, original_rip,
                "sigreturn must restore original RIP");
            assert_eq!(frame.user_rsp, original_rsp,
                "sigreturn must restore original RSP");
            assert_eq!(frame.rdi, original_rdi,
                "sigreturn must restore original RDI");

            {
                let inner = env.process.inner.lock();
                assert!(inner.pending_signal_frame.is_none(),
                    "pending_signal_frame must be cleared after sigreturn");
            }

            cleanup(env);
        }
    }

    proptest::proptest! {
        #[test]
        fn signal_mask_blocking(
            sig_num in 1u8..=31,
        ) {
            let _guard = crate::test_serial::acquire();
            if sig_num == SIGKILL || sig_num == SIGSTOP {
                return Ok(());
            }

            let env = setup_test_env();
            let handler_addr = 0x1_0000_0000u64;

            {
                let mut inner = env.process.inner.lock();
                inner.signal_handlers[sig_num as usize] = SignalAction::Handler(handler_addr);
                inner.pending_signals.insert(sig_num);
                inner.signal_mask.insert(sig_num);
            }

            let mut frame = make_frame(&env);
            let original_rip = frame.user_rip;

            // Signal is masked — should NOT be delivered.
            check_pending_signals(&mut frame);
            assert_eq!(frame.user_rip, original_rip,
                "RIP must not change when signal is masked");

            // Unmask the signal.
            {
                let mut inner = env.process.inner.lock();
                inner.signal_mask.remove(sig_num);
            }

            // Signal should now be delivered.
            check_pending_signals(&mut frame);
            assert_eq!(frame.user_rip, handler_addr,
                "RIP must be set to handler after unmasking");

            cleanup(env);
        }
    }

    #[test]
    fn signal_default_ignore_clears_pending() {
        let _guard = crate::test_serial::acquire();
        let env = setup_test_env();
        {
            let mut inner = env.process.inner.lock();
            inner.pending_signals.insert(SIGCHLD);
        }

        let mut frame = make_frame(&env);
        let original_rip = frame.user_rip;

        check_pending_signals(&mut frame);

        assert_eq!(frame.user_rip, original_rip,
            "RIP must not change for ignored signal");

        {
            let inner = env.process.inner.lock();
            assert!(!inner.pending_signals.contains(SIGCHLD),
                "SIGCHLD must be cleared after default ignore action");
        }

        cleanup(env);
    }

    #[test]
    fn sigaction_stores_handler() {
        let _guard = crate::test_serial::acquire();
        let env = setup_test_env();
        {
            let mut inner = env.process.inner.lock();
            inner.signal_handlers[10] = SignalAction::Handler(0xdeadbeef);
        }
        {
            let inner = env.process.inner.lock();
            assert_eq!(
                inner.signal_handlers[10],
                SignalAction::Handler(0xdeadbeef)
            );
        }
        cleanup(env);
    }

    #[test]
    fn sigprocmask_blocks_and_unblocks() {
        let _guard = crate::test_serial::acquire();
        let env = setup_test_env();
        {
            let mut inner = env.process.inner.lock();
            inner.signal_mask.insert(10);
            assert!(inner.signal_mask.contains(10));
        }
        {
            let mut inner = env.process.inner.lock();
            inner.signal_mask.remove(10);
            assert!(!inner.signal_mask.contains(10));
        }
        cleanup(env);
    }

    #[test]
    fn kill_nonexistent_pid_returns_false() {
        assert!(!send_signal(ProcessId(99999), 10));
    }

    #[test]
    fn signal_mask_blocking_sig26() {
        let _guard = crate::test_serial::acquire();
        let sig_num: u8 = 26;
        let env = setup_test_env();
        let handler_addr = 0x1_0000_0000u64;

        {
            let mut inner = env.process.inner.lock();
            inner.signal_handlers[sig_num as usize] = SignalAction::Handler(handler_addr);
            inner.pending_signals.insert(sig_num);
            inner.signal_mask.insert(sig_num);
        }

        let mut frame = make_frame(&env);
        let original_rip = frame.user_rip;

        // While masked — no delivery.
        check_pending_signals(&mut frame);
        assert_eq!(frame.user_rip, original_rip);

        // Unmask.
        {
            let mut inner = env.process.inner.lock();
            inner.signal_mask.remove(sig_num);
        }

        // Now should be delivered.
        check_pending_signals(&mut frame);
        assert_eq!(frame.user_rip, handler_addr,
            "RIP must be set to handler after unmasking sig={}", sig_num);

        cleanup(env);
    }
}
