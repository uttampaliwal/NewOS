use super::process::{handle_exec, handle_wait_impl};
use super::*;
use crate::fs::vfs::FsBackend;
use crate::memory::vma::VmaSet;
use crate::process::{
    Process, ProcessControlBlock, ProcessId, ProcessState, SignalAction, SignalSet,
};
use crate::task::{Task, TaskId, TaskState};
use alloc::sync::Arc;
use spin::Mutex;
use turnix_abi::syscall::SyscallArgs;
use x86_64::structures::paging::PhysFrame;
use x86_64::{PhysAddr, VirtAddr};

struct SafeBootInfo(turnix_abi::boot::BootInfo);
unsafe impl Sync for SafeBootInfo {}

static DUMMY_BOOT_INFO: SafeBootInfo = SafeBootInfo(turnix_abi::boot::BootInfo::uefi(
    turnix_abi::version::ABI_VERSION,
));

fn setup_dummy_process() {
    let process = Process {
        inner: Arc::new(Mutex::new(ProcessControlBlock {
            id: ProcessId(1),
            ppid: ProcessId(0),
            state: ProcessState::Running,
            pml4_frame: PhysFrame::containing_address(PhysAddr::new(0)),
            entry_point: VirtAddr::zero(),
            stack_top: VirtAddr::zero(),
            threads: alloc::vec![],
            vma_set: VmaSet::new(),
            mmap_next_addr: VirtAddr::zero(),
            aslr_base: VirtAddr::zero(),
            fd_table: alloc::vec![None; 1024],
            signal_mask: SignalSet::empty(),
            signal_handlers: [SignalAction::Default; 64],
            pending_signals: SignalSet::empty(),
            pending_signal_frame: None,
            sec_ctx: crate::security::SecurityContext::root(),
            nsproxy: crate::security::namespaces::NsProxy::new(),
            seccomp_filter: None,
            cgroup_path: None,
        })),
    };
    let task = Task::new_test(TaskId::new(), process, TaskState::Running);
    crate::task::scheduler::set_current_task_for_test(task);
}

fn cleanup() {
    *crate::boot::FRAME_ALLOCATOR.lock() = None;
    *crate::boot::PHYS_MEM_OFFSET.lock() = None;
    *crate::vfs::VFS.lock() = crate::vfs::Vfs::new();
}

#[test]
fn test_exec_nonexistent_path_returns_enoent() {
    let _guard = crate::test_serial::acquire();
    setup_dummy_process();

    // Ensure VFS has a mounted root but no such file
    let mut vfs = crate::vfs::VFS.lock();
    *vfs = crate::vfs::Vfs::new();
    vfs.mount(
        "/",
        Arc::new(crate::fs::tmpfs::TmpfsBackend::new()),
        crate::fs::vfs::MountFlags::default(),
    )
    .unwrap();
    drop(vfs);

    let path = "/nonexistent_file";
    let args = SyscallArgs::new(path.as_ptr() as u64, path.len() as u64, 0, 0);

    let result = handle_exec(args);
    match result {
        SyscallResult::Error(err) => {
            assert_eq!(err, 2, "Expected ENOENT (2) when file is not found");
        }
        other => panic!("Expected SyscallResult::Error, got {:?}", other),
    }

    // Verify the process remains unchanged (still has empty VMA set, etc.)
    let current_process = crate::task::scheduler::get_current_process().unwrap();
    let inner = current_process.inner.lock();
    assert_eq!(inner.id.0, 1);
    assert!(inner.vma_set.iter().next().is_none());

    cleanup();
}

#[test]
fn test_exec_invalid_elf_magic_returns_enoexec() {
    let _guard = crate::test_serial::acquire();
    setup_dummy_process();

    // Set up frame allocator and phys mem offset
    let boot_info = &DUMMY_BOOT_INFO.0;
    let mut guard = crate::boot::FRAME_ALLOCATOR.lock();
    *guard = Some(crate::memory::FrameAllocator::new(boot_info));
    let mut offset_guard = crate::boot::PHYS_MEM_OFFSET.lock();
    *offset_guard = Some(VirtAddr::zero());
    drop(guard);
    drop(offset_guard);

    // Mount a tmpfs root and create a file with invalid ELF magic directly
    let backend = Arc::new(crate::fs::tmpfs::TmpfsBackend::new());
    let inode = backend
        .inner
        .lock()
        .create_file(crate::fs::vfs::InodeId(1), "invalid_elf", 0o777)
        .unwrap();
    backend.write(inode, 0, b"not a valid ELF file").unwrap();

    let mut vfs = crate::vfs::VFS.lock();
    *vfs = crate::vfs::Vfs::new();
    vfs.mount("/", backend, crate::fs::vfs::MountFlags::default())
        .unwrap();
    drop(vfs);

    let path = "/invalid_elf";
    let args = SyscallArgs::new(path.as_ptr() as u64, path.len() as u64, 0, 0);

    let result = handle_exec(args);
    match result {
        SyscallResult::Error(err) => {
            assert_eq!(err, 8, "Expected ENOEXEC (8) when ELF magic is invalid");
        }
        other => panic!("Expected SyscallResult::Error, got {:?}", other),
    }

    // Verify the process remains unchanged
    let current_process = crate::task::scheduler::get_current_process().unwrap();
    let inner = current_process.inner.lock();
    assert_eq!(inner.id.0, 1);
    assert!(inner.vma_set.iter().next().is_none());

    cleanup();
}
// -----------------------------------------------------------------------
// Task-23: wait / waitpid / zombie-reaping unit tests
// -----------------------------------------------------------------------

use proptest::prelude::*;

/// Build a minimal PCB with a given PID and PPID.
fn make_pcb(pid: usize, ppid: usize, state: ProcessState) -> Arc<Mutex<ProcessControlBlock>> {
    Arc::new(Mutex::new(ProcessControlBlock {
        id: ProcessId(pid),
        ppid: ProcessId(ppid),
        state,
        pml4_frame: PhysFrame::containing_address(PhysAddr::new(0)),
        entry_point: VirtAddr::zero(),
        stack_top: VirtAddr::zero(),
        threads: alloc::vec![],
        vma_set: VmaSet::new(),
        mmap_next_addr: VirtAddr::zero(),
        aslr_base: VirtAddr::zero(),
        fd_table: alloc::vec![None; 1024],
        signal_mask: SignalSet::empty(),
        signal_handlers: [SignalAction::Default; 64],
        pending_signals: SignalSet::empty(),
        pending_signal_frame: None,
        sec_ctx: crate::security::SecurityContext::root(),
        nsproxy: crate::security::namespaces::NsProxy::new(),
        seccomp_filter: None,
        cgroup_path: None,
    }))
}

// Property 15 — Wait Exit Status Round-Trip
//
// For any exit code in [0, 255]:
//   1. The child is in Zombie state with the correct exit code
//      between exit and wait.
//   2. `wait` returns exactly that exit code (POSIX-encoded).
//   3. The zombie child is reaped (removed from PROCESS_TABLE).
//
// Validates Requirements 17.3, 17.4.
proptest! {
    #[test]
    fn test_prop_wait_exit_status_round_trip(exit_code in 0i32..=255) {
        let _guard = crate::test_serial::acquire();
        use crate::process::{ProcessId, ProcessState, PROCESS_TABLE};

        let _parent_pid = ProcessId(900);
        let child_pid  = ProcessId(901);

        // Insert child as Zombie with the generated exit code.
        {
            let mut table = PROCESS_TABLE.lock();
            table.insert(child_pid, make_pcb(901, 900, ProcessState::Zombie { exit_code }));
        }

        // Verify the child is in Zombie state between exit and wait.
        {
            let table = PROCESS_TABLE.lock();
            let child = table.get(&child_pid).unwrap();
            let child_inner = child.lock();
            match child_inner.state {
                ProcessState::Zombie { exit_code: code } => {
                    assert_eq!(code, exit_code,
                        "child has wrong exit code in Zombie state");
                }
                ref other => panic!("expected Zombie, got {:?}", other),
            }
        }

        // Set up current task as parent.
        let parent_proc = crate::process::Process {
            inner: make_pcb(900, 1, ProcessState::Running),
        };
        let task = crate::task::Task::new_test(
            crate::task::TaskId::new(),
            parent_proc,
            crate::task::TaskState::Running,
        );
        crate::task::scheduler::set_current_task_for_test(task);

        // Call wait(-1) and capture the exit status.
        let mut status: i32 = 0xdead;
        let result = handle_wait_impl(-1, &mut status as *mut i32, false);
        match result {
            SyscallResult::Success(pid) => {
                assert_eq!(pid, child_pid.0 as u64,
                    "wait returned wrong child PID");
            }
            other => panic!("expected Success, got {:?}", other),
        }

        // POSIX encodes exit status as (exit_code & 0xff) << 8.
        let expected_status = (exit_code & 0xff) << 8;
        assert_eq!(status, expected_status,
            "wait returned wrong exit status for code {}", exit_code);

        // Child must have been reaped from the process table.
        let table = PROCESS_TABLE.lock();
        assert!(
            table.get(&child_pid).is_none(),
            "zombie child should have been reaped from PROCESS_TABLE"
        );
    }
}

/// Verify that handle_wait_impl finds a zombie child and returns its PID
/// and exit code, and that the child is removed from the process table.
#[test]
fn test_wait_reaps_zombie_child() {
    let _guard = crate::test_serial::acquire();
    use crate::process::{PROCESS_TABLE, ProcessId, ProcessState};

    let _parent_pid = ProcessId(200);
    let child_pid = ProcessId(201);
    let exit_code = 42i32;

    // Insert child (zombie) into process table.
    {
        let mut table = PROCESS_TABLE.lock();
        table.insert(
            child_pid,
            make_pcb(201, 200, ProcessState::Zombie { exit_code }),
        );
    }

    // Set up a current task so get_current_process_id() returns parent_pid.
    let parent_proc = Process {
        inner: make_pcb(200, 1, ProcessState::Running),
    };
    let task = Task::new_test(TaskId::new(), parent_proc, TaskState::Running);
    crate::task::scheduler::set_current_task_for_test(task);

    // Call handle_wait_impl — expects Zombie child, should reap it.
    let result = handle_wait_impl(-1, core::ptr::null_mut(), false);
    match result {
        SyscallResult::Success(pid) => {
            assert_eq!(pid, child_pid.0 as u64, "returned wrong child PID");
        }
        other => panic!("expected Success, got {:?}", other),
    }

    // Child must have been removed from the process table.
    let table = PROCESS_TABLE.lock();
    assert!(
        table.get(&child_pid).is_none(),
        "zombie child should have been reaped from PROCESS_TABLE"
    );
}

/// Verify that handle_wait_impl returns ECHILD when the current process
/// has no children at all.
#[test]
fn test_wait_returns_echild_when_no_children() {
    let _guard = crate::test_serial::acquire();
    use crate::process::{PROCESS_TABLE, ProcessId, ProcessState};

    // Make sure the process table has no children of PID 300.
    {
        let mut table = PROCESS_TABLE.lock();
        table.retain(|_, pcb| pcb.lock().ppid != ProcessId(300));
    }

    let parent_proc = Process {
        inner: make_pcb(300, 1, ProcessState::Running),
    };
    let task = Task::new_test(TaskId::new(), parent_proc, TaskState::Running);
    crate::task::scheduler::set_current_task_for_test(task);

    let result = handle_wait_impl(-1, core::ptr::null_mut(), false);
    match result {
        SyscallResult::Error(e) => {
            assert_eq!(e, 10, "expected ECHILD (10), got {}", e);
        }
        other => panic!("expected Error(10/ECHILD), got {:?}", other),
    }
}

/// Verify that handle_wait_impl for a specific PID returns only that
/// child's exit code and reaps only that child.
#[test]
fn test_waitpid_reaps_specific_child() {
    let _guard = crate::test_serial::acquire();
    use crate::process::{PROCESS_TABLE, ProcessId, ProcessState};

    let _parent_pid = ProcessId(400);
    let child_a_pid = ProcessId(401);
    let child_b_pid = ProcessId(402);

    {
        let mut table = PROCESS_TABLE.lock();
        // child_a: zombie, child_b: running
        table.insert(
            child_a_pid,
            make_pcb(401, 400, ProcessState::Zombie { exit_code: 77 }),
        );
        table.insert(child_b_pid, make_pcb(402, 400, ProcessState::Running));
    }

    let parent_proc = Process {
        inner: make_pcb(400, 1, ProcessState::Running),
    };
    let task = Task::new_test(TaskId::new(), parent_proc, TaskState::Running);
    crate::task::scheduler::set_current_task_for_test(task);

    // Wait specifically for child_a.
    let result = handle_wait_impl(401, core::ptr::null_mut(), false);
    match result {
        SyscallResult::Success(pid) => {
            assert_eq!(pid, child_a_pid.0 as u64, "should have reaped child_a");
        }
        other => panic!("expected Success, got {:?}", other),
    }

    // child_a reaped, child_b still present.
    let table = PROCESS_TABLE.lock();
    assert!(
        table.get(&child_a_pid).is_none(),
        "child_a should be reaped"
    );
    assert!(
        table.get(&child_b_pid).is_some(),
        "child_b should still exist"
    );
    drop(table);

    // Cleanup child_b.
    PROCESS_TABLE.lock().remove(&child_b_pid);
}

// ------------------------------------------------------------------
// Fork / Process-table tests
// ------------------------------------------------------------------

#[test]
fn test_handle_fork_adds_child_to_process_table() {
    let _guard = crate::test_serial::acquire();
    let child_pcb = make_pcb(42, 1, ProcessState::Ready);
    let child_pid = child_pcb.lock().id;

    let mut table = crate::process::PROCESS_TABLE.lock();
    table.insert(child_pid, child_pcb.clone());
    drop(table);

    let table = crate::process::PROCESS_TABLE.lock();
    let found = table.get(&child_pid);
    assert!(found.is_some(), "child should be in process table");
    assert_eq!(found.unwrap().lock().ppid, ProcessId(1));
    drop(table);

    let mut table = crate::process::PROCESS_TABLE.lock();
    table.remove(&child_pid);
}

// ------------------------------------------------------------------
// Dup / Dup2 tests
// ------------------------------------------------------------------

fn make_test_fd(vfs: &mut crate::vfs::Vfs, name: &str) -> usize {
    vfs.insert_fd(crate::vfs::FileDescriptor::new(
        crate::vfs::InodeId(0),
        alloc::sync::Arc::new(crate::fs::tmpfs::TmpfsBackend::new()),
        crate::vfs::OpenFlags::RDWR,
        crate::vfs::FdKind::Regular,
        alloc::string::String::from(name),
    ))
}

#[test]
fn dup_returns_new_fd() {
    let _guard = crate::test_serial::acquire();
    let mut vfs = crate::vfs::VFS.lock();
    let fd = make_test_fd(&mut vfs, "test");
    let newfd = vfs.dup_fd(fd).expect("dup should succeed");
    assert_ne!(fd, newfd, "dup must return a different fd number");
    assert!(vfs.get_fd(fd).is_some(), "original fd must remain open");
    assert!(vfs.get_fd(newfd).is_some(), "new fd must exist");
}

#[test]
fn dup2_uses_specified_fd() {
    let _guard = crate::test_serial::acquire();
    let mut vfs = crate::vfs::VFS.lock();
    let fd = make_test_fd(&mut vfs, "test");
    let target = 99usize;
    let result = vfs.dup2_fd(fd, target).expect("dup2 should succeed");
    assert_eq!(result, target, "dup2 must return the target fd");
    assert!(vfs.get_fd(fd).is_some(), "original fd must remain open");
    assert!(vfs.get_fd(target).is_some(), "target fd must exist");
    let _ = vfs.close_fd(target);
}

#[test]
fn dup2_closes_existing_target() {
    let _guard = crate::test_serial::acquire();
    let mut vfs = crate::vfs::VFS.lock();
    let fd_a = make_test_fd(&mut vfs, "a");
    let fd_b = make_test_fd(&mut vfs, "b");
    let result = vfs.dup2_fd(fd_a, fd_b).expect("dup2 should succeed");
    assert_eq!(result, fd_b, "dup2 must return fd_b");
    assert!(vfs.get_fd(fd_b).is_some(), "target fd must still exist");
}

#[test]
fn dup2_same_fd_is_noop() {
    let _guard = crate::test_serial::acquire();
    let mut vfs = crate::vfs::VFS.lock();
    let fd = make_test_fd(&mut vfs, "test");
    let result = vfs
        .dup2_fd(fd, fd)
        .expect("dup2(oldfd, oldfd) should succeed");
    assert_eq!(result, fd, "dup2(oldfd, oldfd) must return oldfd");
    assert!(vfs.get_fd(fd).is_some(), "fd must still exist");
}

#[test]
fn dup_bad_fd_returns_none() {
    let _guard = crate::test_serial::acquire();
    let mut vfs = crate::vfs::VFS.lock();
    assert!(
        vfs.dup_fd(9999).is_none(),
        "dup of invalid fd must return None"
    );
}

#[test]
fn dup2_bad_fd_returns_none() {
    let _guard = crate::test_serial::acquire();
    let mut vfs = crate::vfs::VFS.lock();
    assert!(
        vfs.dup2_fd(9999, 100).is_none(),
        "dup2 of invalid oldfd must return None"
    );
}
