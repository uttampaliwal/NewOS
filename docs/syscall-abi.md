# turnix Syscall ABI

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
| `r8` | Argument 4 (Reserved) |
| `r9` | Argument 5 (Reserved) |
| `rcx` | Destroyed by `syscall` (Stores return RIP) |
| `r11` | Destroyed by `syscall` (Stores return RFLAGS) |

## Syscall Table

| ID | Name | Description | Arguments |
| --- | --- | --- | --- |
| 1 | `Write` | Write to serial/file | `arg0: buffer_ptr`, `arg1: len` |
| 2 | `Exit` | Terminate current task | `arg0: exit_code` |
| 3 | `Read` | Read from file/device | `arg0: fd`, `arg1: buffer_ptr`, `arg2: len` |
| 4 | `Open` | Open a file | `arg0: path_ptr`, `arg1: path_len` |
| 5 | `Close` | Close a file | `arg0: fd` |
| 6 | `Exec` | Replace current process with ELF | `arg0: elf_ptr`, `arg1: elf_len` |
| 7 | `Fork` | Create a copy of current process | (None) |
| 8 | `Wait` | Wait for child process | `arg0: pid` |
| 9 | `Yielder` | Yield execution | `arg0: non-zero to yield` |
| 10 | `Stat` | Get file information | `arg0: path_ptr`, `arg1: path_len`, `arg2: stat_ptr` |
| 11 | `Ls` | List directory contents | `arg0: buffer_ptr`, `arg1: buffer_len` |

## Error Handling
Syscalls return a 64-bit value in `rax`.
- Values `>= 0` typically indicate success (e.g., number of bytes read/written).
- Negative values indicate an error (e.g., `-1` for general error).
