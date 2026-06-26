# Turnix Syscall ABI

## Calling Convention
turnix uses the `syscall` and `sysret` instructions for system calls on x86-64.

### Register Usage
| Register | Purpose |
| --- | --- |
| `rax` | Syscall Number (Input) / Return Value (Output) |
| `rdi` | Argument 0 |
| `rsi` | Argument 1 |
| `rdx` | Argument 2 |
| `r10` | Argument 3 |
| `r8` | Argument 4 |
| `r9` | Argument 5 |
| `rcx` | Destroyed by `syscall` (Stores return RIP) |
| `r11` | Destroyed by `syscall` (Stores return RFLAGS) |

## Syscall Table

| ID | Name | Description |
| --- | --- | --- |
| 1 | `Write` | Write to serial/file descriptor |
| 2 | `Exit` | Terminate current process |
| 3 | `Read` | Read from file descriptor |
| 4 | `Open` | Open a file by path |
| 5 | `Close` | Close a file descriptor |
| 6 | `Exec` | Replace process with ELF binary |
| 7 | `Fork` | Create child process (returns to both parent and child) |
| 8 | `Wait` | Wait for child process to exit |
| 9 | `Yielder` | Yield CPU to scheduler |
| 10 | `Uptime` | Get monotonic uptime in microseconds |
| 11 | `Ls` | List directory contents |
| 12 | `Stat` | Get file status |
| 13 | `GetPid` | Get current process ID |
| 14 | `Seek` | Seek file descriptor |
| 15 | `WriteFile` | Write to file by path |
| 16 | `GetUid` | Get user ID |
| 17 | `GetGid` | Get group ID |
| 18 | `Brk` | Set program break (heap) |
| 19 | `Mkdir` | Create directory |
| 20 | `Unlink` | Remove file |
| 21 | `MmapFramebuffer` | Map GPU framebuffer |
| 22 | `Mmap` | Map memory (anonymous or file-backed) |
| 23 | `Munmap` | Unmap memory region |
| 24 | `Mount` | Mount filesystem |
| 25 | `Umount` | Unmount filesystem |
| 26 | `Waitpid` | Wait for specific child process |
| 27 | `Pipe` | Create a pipe pair |
| 28 | `Socket` | Create a socket |
| 29 | `Bind` | Bind socket to address |
| 30 | `Listen` | Mark socket as passive (server) |
| 31 | `Accept` | Accept incoming connection |
| 32 | `Connect` | Connect to remote socket |
| 33 | `Sigaction` | Set signal action handler |
| 34 | `Sigprocmask` | Get/set signal mask |
| 35 | `Sigreturn` | Return from signal handler |
| 36 | `Kill` | Send signal to process |
| 37 | `Dup` | Duplicate file descriptor |
| 38 | `Dup2` | Duplicate file descriptor to specific number |
| 39 | `Shutdown` | Shutdown socket |
| 40 | `ReadShutdownSignal` | Read shutdown signal |
| 41 | `Capget` | Get process capabilities |
| 42 | `Capset` | Set process capabilities |
| 43 | `Clone` | Clone process |
| 44 | `Prctl` | Process control |
| 45 | `InputRead` | Read input event |
| 46 | `GbmCreate` | Create GBM buffer |
| 47 | `GbmMap` | Map GBM buffer |
| 48 | `GbmDestroy` | Destroy GBM buffer |
| 49 | `DrmPageFlip` | DRM page flip |
| 50 | `SetUid` | Set user ID |
| 51 | `SetGid` | Set group ID |
| 52 | `Chdir` | Change current working directory |
| 53 | `Dmesg` | Read kernel log ring buffer |
| 54 | `XattrGet` | Get extended attribute |
| 55 | `XattrSet` | Set extended attribute |
| 56 | `NetSetAddr` | Set network interface IP/netmask/gateway |
| 57 | `NetSetRoute` | Set default gateway |
| 58 | `NetQuery` | Query network interface configuration |
| 59 | `Ftruncate` | Truncate file to specified length |
| 60 | `Mmap2` | Map memory (6-argument variant with offset) |
| 61 | `ShmOpen` | Open POSIX shared memory object |
| 62 | `ShmUnlink` | Remove POSIX shared memory object |
| 63 | `MqOpen` | Open POSIX message queue |
| 64 | `MqClose` | Close POSIX message queue |
| 65 | `MqUnlink` | Remove POSIX message queue |
| 66 | `MqSend` | Send message to POSIX message queue |
| 67 | `MqReceive` | Receive message from POSIX message queue |
| 68 | `Futex` | Fast userspace mutex (wait/wake) |
| 69 | `EpollCreate` | Create epoll instance |
| 70 | `EpollCtl` | Control epoll interest set |
| 71 | `EpollWait` | Wait for epoll events |
| 72 | `SchedSetScheduler` | Set scheduling policy |
| 73 | `SchedGetScheduler` | Get scheduling policy |
| 74 | `CgroupCreate` | Create cgroup hierarchy |
| 75 | `CgroupAddProcess` | Add process to cgroup |
| 76 | `CgroupSetCpuMax` | Set cgroup CPU quota |
| 77 | `CgroupSetMemoryMax` | Set cgroup memory limit |
| 78 | `CgroupSetPidsMax` | Set cgroup PID limit |
| 79 | `EventfdCreate` | Create eventfd file descriptor |
| 80 | `EventfdRead` | Read from eventfd |
| 81 | `EventfdWrite` | Write to eventfd |
| 82 | `TimerfdCreate` | Create timerfd file descriptor |
| 83 | `TimerfdSettime` | Set timerfd interval |
| 84 | `TimerfdGettime` | Get timerfd remaining time |
| 85 | `IoUringSetup` | Set up io_uring submission/completion queues |
| 86 | `IoUringSubmit` | Submit entries to io_uring |
| 87 | `IoUringComplete` | Reap completions from io_uring |

## Error Handling
Syscalls return a 64-bit value in `rax`. Negative values indicate errors.

## Related Decisions
For the architectural rationale behind the syscall interface, see [ADR 0002: User Mode and Syscall Interface (ABI)](decisions/0002-user-mode-syscalls.md).
