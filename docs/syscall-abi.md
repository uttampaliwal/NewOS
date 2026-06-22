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
| 10 | `Stat` | Get file status |
| 11 | `Ls` | List directory contents |
| 12 | `Mmap` | Map memory (anonymous or file-backed) |
| 13 | `Munmap` | Unmap memory region |
| 14 | `Pipe` | Create a pipe pair |
| 15 | `Socket` | Create a socket |
| 16 | `Bind` | Bind socket to address |
| 17 | `Listen` | Mark socket as passive (server) |
| 18 | `Accept` | Accept incoming connection |
| 19 | `Connect` | Connect to remote socket |
| 20 | `Send` | Send data on socket |
| 21 | `Recv` | Receive data from socket |
| 22 | `CloseSocket` | Close a socket (separate from file close) |
| 23 | `Kill` | Send signal to process |
| 24 | `Sigaction` | Set signal action handler |
| 25 | `Sigprocmask` | Get/set signal mask |
| 26 | `Sigreturn` | Return from signal handler |
| 27 | `Dup` | Duplicate file descriptor |
| 28 | `Dup2` | Duplicate file descriptor to specific number |
| 29 | `Getcwd` | Get current working directory |
| 30 | `Chdir` | Change current working directory |
| 31 | `Dmesg` | Read kernel log ring buffer |
| 32 | `XattrGet` | Get extended attribute |
| 33 | `XattrSet` | Set extended attribute |
| 56 | `NetSetAddr` | Set network interface IP/netmask/gateway |
| 57 | `NetSetRoute` | Set default gateway |
| 58 | `NetQuery` | Query network interface configuration |

## Error Handling
Syscalls return a 64-bit value in `rax`. Negative values indicate errors.

## Related Decisions
For the architectural rationale behind the syscall interface, see [ADR 0002: User Mode and Syscall Interface (ABI)](decisions/0002-user-mode-syscalls.md).
