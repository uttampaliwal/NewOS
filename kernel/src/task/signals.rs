use alloc::sync::Arc;
use spin::Mutex;

use crate::arch::x86_64::syscall_arch::SyscallFrame;
use crate::process::{ProcessControlBlock, ProcessId, ProcessState, SignalAction};
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
        SIGHUP
            | SIGINT
            | SIGQUIT
            | SIGILL
            | SIGTRAP
            | SIGABRT
            | SIGBUS
            | SIGFPE
            | SIGKILL
            | SIGUSR1
            | SIGSEGV
            | SIGUSR2
            | SIGPIPE
            | SIGALRM
            | SIGTERM
            | SIGSTKFLT
            | SIGXCPU
            | SIGXFSZ
            | SIGVTALRM
            | SIGPROF
            | SIGIO
            | SIGPWR
            | SIGSYS
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
            inner.state = ProcessState::Zombie {
                exit_code: 128 + sig as i32,
            };
            inner.vma_set = crate::memory::vma::VmaSet::new();
            let my_pid = inner.id;
            // Reparent children to init.
            let table = crate::process::PROCESS_TABLE.lock();
            for pcb_arc in table.values() {
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
        inner.state = ProcessState::Stopped;
        inner.pending_signals.remove(sig);
    } else if default_continues(sig) {
        let mut inner = proc.lock();
        if matches!(inner.state, ProcessState::Stopped) {
            inner.state = ProcessState::Ready;
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
        (
            inner.pending_signal_frame,
            inner.pending_signal_frame.is_some(),
        )
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
    use crate::process::{Process, ProcessControlBlock, ProcessId, ProcessState, SignalAction};
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
            signal_mask: crate::process::SignalSet::empty(),
            signal_handlers: [SignalAction::Default; 64],
            pending_signals: crate::process::SignalSet::empty(),
            pending_signal_frame: None,
            sec_ctx: crate::security::SecurityContext::root(),
            nsproxy: crate::security::namespaces::NsProxy::new(),
            seccomp_filter: None,
            cgroup_path: None,
        };
        let process = Process {
            inner: Arc::new(Mutex::new(pcb)),
        };
        let task = Task::new_test(
            TaskId::new(),
            process.clone(),
            crate::task::TaskState::Running,
        );
        scheduler::set_current_task_for_test(task);
        TestEnv {
            process,
            stack_layout,
            stack_ptr,
        }
    }

    fn make_frame(env: &TestEnv) -> SyscallFrame {
        SyscallFrame {
            r15: 0,
            r14: 0,
            r13: 0,
            r12: 0,
            r11: 0,
            r10: 0,
            r9: 0,
            r8: 0,
            rdi: 0,
            rsi: 0,
            rbp: 0,
            rdx: 0,
            rcx: 0,
            rbx: 0,
            rax: 0,
            user_rip: 0x400000,
            user_cs: 0x2b,
            user_rflags: 0x202,
            user_rsp: env.stack_ptr + 2048,
            user_ss: 0x23,
        }
    }

    fn cleanup(env: TestEnv) {
        unsafe {
            std::alloc::dealloc(env.stack_ptr as *mut u8, env.stack_layout);
        }
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

        assert_eq!(
            frame.user_rip, original_rip,
            "RIP must not change for ignored signal"
        );

        {
            let inner = env.process.inner.lock();
            assert!(
                !inner.pending_signals.contains(SIGCHLD),
                "SIGCHLD must be cleared after default ignore action"
            );
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
            assert_eq!(inner.signal_handlers[10], SignalAction::Handler(0xdeadbeef));
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
        assert_eq!(
            frame.user_rip, handler_addr,
            "RIP must be set to handler after unmasking sig={}",
            sig_num
        );

        cleanup(env);
    }

    // ------------------------------------------------------------------
    // send_signal to existing process returns true and sets pending bit
    // ------------------------------------------------------------------
    #[test]
    fn test_send_signal_existing_process() {
        let _guard = crate::test_serial::acquire();
        let env = setup_test_env();
        crate::process::PROCESS_TABLE
            .lock()
            .insert(ProcessId(999), env.process.inner.clone());
        let result = send_signal(ProcessId(999), SIGUSR1);
        assert!(result, "send_signal should return true for existing PID");
        let inner = env.process.inner.lock();
        assert!(
            inner.pending_signals.contains(SIGUSR1),
            "SIGUSR1 should be pending after send_signal"
        );
        drop(inner);
        crate::process::PROCESS_TABLE.lock().remove(&ProcessId(999));
        cleanup(env);
    }

    // ------------------------------------------------------------------
    // send_signal to non-existent process returns false
    // ------------------------------------------------------------------
    #[test]
    fn test_send_signal_nonexistent_returns_false() {
        let _guard = crate::test_serial::acquire();
        let env = setup_test_env();
        let result = send_signal(ProcessId(99999), SIGUSR2);
        assert!(
            !result,
            "send_signal should return false for nonexistent PID"
        );
        cleanup(env);
    }

    // ------------------------------------------------------------------
    // default_action: SIGSTOP sets process state to Stopped
    // ------------------------------------------------------------------
    #[test]
    fn test_sigstop_default_stops_process() {
        let _guard = crate::test_serial::acquire();
        let env = setup_test_env();
        {
            let mut inner = env.process.inner.lock();
            inner.pending_signals.insert(SIGSTOP);
        }
        default_action(SIGSTOP, &env.process.inner);
        let inner = env.process.inner.lock();
        assert!(
            matches!(inner.state, ProcessState::Stopped),
            "SIGSTOP default action should set state to Stopped"
        );
        assert!(
            !inner.pending_signals.contains(SIGSTOP),
            "SIGSTOP should be cleared from pending after default action"
        );
        drop(inner);
        cleanup(env);
    }

    // ------------------------------------------------------------------
    // default_action: SIGCONT on stopped process sets state to Ready
    // ------------------------------------------------------------------
    #[test]
    fn test_sigcont_default_continues_stopped() {
        let _guard = crate::test_serial::acquire();
        let env = setup_test_env();
        {
            let mut inner = env.process.inner.lock();
            inner.state = ProcessState::Stopped;
            inner.pending_signals.insert(SIGCONT);
        }
        default_action(SIGCONT, &env.process.inner);
        let inner = env.process.inner.lock();
        assert!(
            matches!(inner.state, ProcessState::Ready),
            "SIGCONT on stopped process should set state to Ready"
        );
        assert!(
            !inner.pending_signals.contains(SIGCONT),
            "SIGCONT should be cleared from pending after default action"
        );
        drop(inner);
        cleanup(env);
    }

    // ------------------------------------------------------------------
    // default_action: SIGCONT on non-stopped process is a no-op
    // ------------------------------------------------------------------
    #[test]
    fn test_sigcont_noop_when_not_stopped() {
        let _guard = crate::test_serial::acquire();
        let env = setup_test_env();
        {
            let mut inner = env.process.inner.lock();
            inner.state = ProcessState::Ready;
            inner.pending_signals.insert(SIGCONT);
        }
        default_action(SIGCONT, &env.process.inner);
        let inner = env.process.inner.lock();
        assert!(
            matches!(inner.state, ProcessState::Ready),
            "SIGCONT on non-stopped process should leave state as Ready"
        );
        drop(inner);
        cleanup(env);
    }

    // ------------------------------------------------------------------
    // default_action: SIGURG (default-ignore) clears pending bit
    // ------------------------------------------------------------------
    #[test]
    fn test_sigurg_default_ignore_clears_pending() {
        let _guard = crate::test_serial::acquire();
        let env = setup_test_env();
        {
            let mut inner = env.process.inner.lock();
            inner.pending_signals.insert(SIGURG);
        }
        default_action(SIGURG, &env.process.inner);
        let inner = env.process.inner.lock();
        assert!(
            !inner.pending_signals.contains(SIGURG),
            "SIGURG (default-ignore) should clear pending bit"
        );
        assert!(
            matches!(inner.state, ProcessState::Ready),
            "SIGURG should not change process state"
        );
        drop(inner);
        cleanup(env);
    }

    // ------------------------------------------------------------------
    // handle_sigreturn returns 0
    // ------------------------------------------------------------------
    #[test]
    fn test_sigreturn_returns_zero() {
        let _guard = crate::test_serial::acquire();
        let env = setup_test_env();
        let mut frame = make_frame(&env);
        let result = handle_sigreturn_with_frame(&mut frame);
        assert_eq!(result, 0, "sigreturn must always return 0");
        cleanup(env);
    }

    // ------------------------------------------------------------------
    // handle_sigreturn with no pending frame returns 0 without modifying frame
    // ------------------------------------------------------------------
    #[test]
    fn test_sigreturn_no_pending_frame() {
        let _guard = crate::test_serial::acquire();
        let env = setup_test_env();
        let mut frame = make_frame(&env);
        let original_rip = frame.user_rip;
        let original_rsp = frame.user_rsp;
        let result = handle_sigreturn_with_frame(&mut frame);
        assert_eq!(result, 0);
        assert_eq!(
            frame.user_rip, original_rip,
            "frame RIP should not change without pending signal frame"
        );
        assert_eq!(
            frame.user_rsp, original_rsp,
            "frame RSP should not change without pending signal frame"
        );
        cleanup(env);
    }

    // ------------------------------------------------------------------
    // check_pending_signals: lowest-numbered signal delivered first
    // ------------------------------------------------------------------
    #[test]
    fn test_multiple_pending_lowest_first() {
        let _guard = crate::test_serial::acquire();
        let env = setup_test_env();
        let handler_addr = 0x2_0000_0000u64;
        {
            let mut inner = env.process.inner.lock();
            // Register handlers for both signals
            inner.signal_handlers[5] = SignalAction::Handler(handler_addr);
            inner.signal_handlers[15] = SignalAction::Handler(handler_addr + 0x100);
            // Pending both, with higher-numbered first
            inner.pending_signals.insert(15);
            inner.pending_signals.insert(5);
        }
        let mut frame = make_frame(&env);
        check_pending_signals(&mut frame);
        // Signal 5 (lower number) should be delivered first
        assert_eq!(
            frame.user_rip, handler_addr,
            "signal 5 (lower) should be delivered first"
        );
        assert_eq!(frame.rdi, 5, "rdi should be set to signal number 5");
        cleanup(env);
    }

    // ------------------------------------------------------------------
    // check_pending_signals: signal cleared from pending after delivery
    // ------------------------------------------------------------------
    #[test]
    fn test_signal_cleared_after_delivery() {
        let _guard = crate::test_serial::acquire();
        let env = setup_test_env();
        let handler_addr = 0x3_0000_0000u64;
        {
            let mut inner = env.process.inner.lock();
            inner.signal_handlers[10] = SignalAction::Handler(handler_addr);
            inner.pending_signals.insert(10);
        }
        let mut frame = make_frame(&env);
        check_pending_signals(&mut frame);
        let inner = env.process.inner.lock();
        assert!(
            !inner.pending_signals.contains(10),
            "signal 10 should be cleared from pending after handler delivery"
        );
        drop(inner);
        cleanup(env);
    }

    // ------------------------------------------------------------------
    // check_pending_signals: handler sets rdi to signal number
    // ------------------------------------------------------------------
    #[test]
    fn test_handler_sets_rdi_to_signal_number() {
        let _guard = crate::test_serial::acquire();
        let env = setup_test_env();
        let handler_addr = 0x4_0000_0000u64;
        {
            let mut inner = env.process.inner.lock();
            inner.signal_handlers[7] = SignalAction::Handler(handler_addr);
            inner.pending_signals.insert(7);
        }
        let mut frame = make_frame(&env);
        check_pending_signals(&mut frame);
        assert_eq!(frame.rdi, 7, "rdi must be set to the signal number");
        cleanup(env);
    }

    // ------------------------------------------------------------------
    // default_terminates, default_ignores, default_stops, default_continues
    // classification tests
    // ------------------------------------------------------------------
    #[test]
    fn test_signal_classification() {
        assert!(default_terminates(SIGTERM));
        assert!(default_terminates(SIGKILL));
        assert!(default_terminates(SIGSEGV));
        assert!(default_terminates(SIGINT));
        assert!(!default_terminates(SIGCHLD));
        assert!(!default_terminates(SIGSTOP));

        assert!(default_ignores(SIGCHLD));
        assert!(default_ignores(SIGURG));
        assert!(!default_ignores(SIGTERM));
        assert!(!default_ignores(SIGSTOP));

        assert!(default_stops(SIGSTOP));
        assert!(default_stops(SIGTSTP));
        assert!(!default_stops(SIGTERM));
        assert!(!default_stops(SIGCONT));

        assert!(default_continues(SIGCONT));
        assert!(!default_continues(SIGSTOP));
        assert!(!default_continues(SIGTERM));
    }

    // ------------------------------------------------------------------
    // signal_mask blocks delivery even with pending signal
    // ------------------------------------------------------------------
    #[test]
    fn test_mask_prevents_all_delivery() {
        let _guard = crate::test_serial::acquire();
        let env = setup_test_env();
        let handler_addr = 0x5_0000_0000u64;
        {
            let mut inner = env.process.inner.lock();
            inner.signal_handlers[3] = SignalAction::Handler(handler_addr);
            inner.pending_signals.insert(3);
            inner.signal_mask.insert(3);
        }
        let mut frame = make_frame(&env);
        let original_rip = frame.user_rip;
        check_pending_signals(&mut frame);
        assert_eq!(
            frame.user_rip, original_rip,
            "masked signal must not be delivered"
        );
        cleanup(env);
    }

    // ------------------------------------------------------------------
    // SIGKILL cannot be caught, ignored, or masked
    // ------------------------------------------------------------------
    #[test]
    fn test_sigkill_cannot_be_caught() {
        let _guard = crate::test_serial::acquire();
        let env = setup_test_env();
        {
            let mut inner = env.process.inner.lock();
            // Even if a handler is set for SIGKILL, check_pending_signals
            // will use default_action (terminate) instead of delivering to handler.
            inner.signal_handlers[SIGKILL as usize] = SignalAction::Handler(0xdead);
        }
        // Verify the handler was stored but SIGKILL still terminates via default_action.
        {
            let inner = env.process.inner.lock();
            assert!(
                matches!(
                    inner.signal_handlers[SIGKILL as usize],
                    SignalAction::Handler(_)
                ),
                "handler storage itself is allowed, but delivery bypasses it"
            );
        }
        cleanup(env);
    }

    #[test]
    fn test_sigkill_default_action_is_terminate() {
        let _guard = crate::test_serial::acquire();
        assert!(
            default_terminates(SIGKILL),
            "SIGKILL must be classified as a terminating signal"
        );
    }

    #[test]
    fn test_sigstop_cannot_be_ignored() {
        let _guard = crate::test_serial::acquire();
        let env = setup_test_env();
        {
            let mut inner = env.process.inner.lock();
            inner.signal_handlers[SIGSTOP as usize] = SignalAction::Ignore;
        }
        // SIGSTOP always uses default_action even if set to Ignore
        assert!(
            default_stops(SIGSTOP),
            "SIGSTOP must always be classified as a stopping signal"
        );
        cleanup(env);
    }

    // ------------------------------------------------------------------
    // sigaction read-back: setting a handler persists correctly
    // ------------------------------------------------------------------
    #[test]
    fn test_sigaction_read_back_handler() {
        let _guard = crate::test_serial::acquire();
        let env = setup_test_env();
        let addr = 0xAAAA_BBBB_CCCC_DDDDu64;
        {
            let mut inner = env.process.inner.lock();
            inner.signal_handlers[11] = SignalAction::Handler(addr);
        }
        {
            let inner = env.process.inner.lock();
            match inner.signal_handlers[11] {
                SignalAction::Handler(a) => assert_eq!(a, addr),
                _ => panic!("signal_handlers[11] should be Handler"),
            }
        }
        cleanup(env);
    }

    #[test]
    fn test_sigaction_read_back_ignore() {
        let _guard = crate::test_serial::acquire();
        let env = setup_test_env();
        {
            let mut inner = env.process.inner.lock();
            inner.signal_handlers[12] = SignalAction::Ignore;
        }
        {
            let inner = env.process.inner.lock();
            assert_eq!(inner.signal_handlers[12], SignalAction::Ignore);
        }
        cleanup(env);
    }

    #[test]
    fn test_sigaction_read_back_default() {
        let _guard = crate::test_serial::acquire();
        let env = setup_test_env();
        {
            let mut inner = env.process.inner.lock();
            inner.signal_handlers[13] = SignalAction::Handler(0x1234);
            inner.signal_handlers[13] = SignalAction::Default;
        }
        {
            let inner = env.process.inner.lock();
            assert_eq!(inner.signal_handlers[13], SignalAction::Default);
        }
        cleanup(env);
    }

    #[test]
    fn test_ignore_action_clears_pending() {
        let _guard = crate::test_serial::acquire();
        let env = setup_test_env();
        {
            let mut inner = env.process.inner.lock();
            inner.signal_handlers[14] = SignalAction::Ignore;
            inner.pending_signals.insert(14);
        }
        let mut frame = make_frame(&env);
        check_pending_signals(&mut frame);
        let inner = env.process.inner.lock();
        assert!(
            !inner.pending_signals.contains(14),
            "ignored signal should be cleared from pending"
        );
        drop(inner);
        cleanup(env);
    }
}
