# ADR 0002: User Mode and Syscall Interface (ABI)

## Status
Accepted

## Context
Transitioning from kernel-only execution to user-mode processes is a critical milestone for turnix. We need a secure, high-performance, and standardized way for user-mode code to request kernel services while maintaining strict isolation.

## Decision
1. **x86_64 SYSCALL/SYSRET**: We use the native `SYSCALL` and `SYSRET` instructions for Ring 3 to Ring 0 transitions.
   - **Rationale**: These instructions provide the fastest possible path for system calls by avoiding the overhead of the interrupt descriptor table (IDT) for every request.
2. **IRETQ for Return (Stability Phase)**: Initially, we use `IRETQ` to return to user mode.
   - **Rationale**: `IRETQ` is more robust during early development as it explicitly restores the stack pointer (`RSP`) and CPU flags (`RFLAGS`) from the stack, making it easier to debug context switches. We will transition to `SYSRET` for production performance once stability is verified.
3. **Register-Based Calling Convention**:
   | Register | Role |
   | --- | --- |
   | `rax` | Syscall Number (In) / Return Value (Out) |
   | `rdi` | Argument 0 |
   | `rsi` | Argument 1 |
   | `rdx` | Argument 2 |
   | `r10` | Argument 3 |
   | `r8` | Argument 4 |
   | `r9` | Argument 5 |
   | `rcx` | *Destroyed* (used by hardware for return address) |
   | `r11` | *Destroyed* (used by hardware for flags) |
   - **Rationale**: Matches the System V AMD64 ABI used by Linux, allowing for easier porting of standard libraries and compilers.
4. **Error Handling**: Syscalls return results in `rax`.
   - **Protocol**: Positive values indicate success (e.g., number of bytes read). Negative values (in the range `-1` to `-4095`) represent error codes (similar to `errno` in Unix).
5. **ELF for User Executables**: Standard 64-bit ELF binaries are used for all user-space processes.
   - **Rationale**: Allows the use of standard Rust and LLVM toolchains without custom object formats.

## Consequences
- **Security**: The kernel must strictly validate all pointers passed in registers to prevent "Confused Deputy" attacks where user-mode code tries to trick the kernel into reading/writing kernel memory.
- **Kernel GS Base**: The kernel uses `swapgs` on syscall entry to access the per-CPU `GS` segment, which holds the kernel stack pointer. This must be managed with extreme care to prevent kernel stack corruption.
