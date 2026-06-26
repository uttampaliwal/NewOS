# System Call Interface

Turnix exposes 87 system calls through a ring 3 to ring 0 transition mechanism defined
in `kernel/src/syscall/handler.rs` and the shared ABI crate (`shared/abi/`).

## Transition Mechanism

User-space code invokes syscalls via the `libturnix` shims, which load the syscall number
and arguments into registers and execute a dedicated `syscall` instruction. The kernel
entry point saves user registers, validates the syscall number, and dispatches to the
appropriate handler.

## Syscall Categories

- **Process**: `fork`, `exec`, `clone`, `exit`, `wait`, `waitpid`, `getpid`, `getuid`,
  `getgid`, `setuid`, `setgid`, `yielder`, `prctl`, `capget`, `capset`
- **Memory**: `brk`, `mmap`, `mmap2`, `munmap`, `mmap_framebuffer`, `ftruncate`
- **File I/O**: `open`, `close`, `read`, `write`, `seek`, `stat`, `ls`, `write_file`,
  `mkdir`, `unlink`, `xattrget`, `xattrset`
- **IPC**: `pipe`, `socket`, `bind`, `listen`, `accept`, `connect`, `sendto`, `recvfrom`,
  `shutdown`, `epoll_create`, `epoll_ctl`, `epoll_wait`, `futex`
- **POSIX IPC**: `shm_open`, `shm_unlink`, `mq_open`, `mq_close`, `mq_unlink`, `mq_send`, `mq_receive`
- **Signals**: `kill`, `sigaction`, `sigprocmask`, `sigreturn`
- **Scheduling**: `sched_set_scheduler`, `sched_get_scheduler`
- **cgroups**: `cgroup_create`, `cgroup_add_process`, `cgroup_set_cpu_max`,
  `cgroup_set_memory_max`, `cgroup_set_pids_max`
- **System**: `uptime`, `dmesg`, `chdir`, `mount`, `umount`
- **Network**: `net_set_addr`, `net_set_route`, `net_query`
- **GPU**: `drm_page_flip`, `gbm_create`, `gbm_map`, `gbm_destroy`, `input_read`
- **eventfd**: `eventfd_create`, `eventfd_read`, `eventfd_write`
- **timerfd**: `timerfd_create`, `timerfd_settime`, `timerfd_gettime`
- **io_uring**: `io_uring_setup`, `io_uring_submit`, `io_uring_complete`

## Argument Validation

Every handler validates pointer arguments for correct alignment, user-space origin, and
bounds before dereferencing. Seccomp-BPF filters (`kernel/src/security/`) can restrict
which syscalls a process is allowed to invoke.
