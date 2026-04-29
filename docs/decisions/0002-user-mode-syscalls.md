# ADR 0002: User Mode and Syscall Interface

## Status
Accepted

## Context
Transitioning from kernel-only execution to user-mode processes is a critical milestone for NewOS. We need a secure and efficient way for user-mode code to request kernel services.

## Decision
1. **x86_64 SYSCALL/SYSRET**: We use the architectural `SYSCALL` instruction for Ring 3 to Ring 0 transitions.
   - **Rationale**: It is significantly faster than interrupt-based gates.
2. **IRETQ for Return (Initially)**: While `SYSRET` is standard, we currently use `IRETQ` for returning from syscalls to ensure maximum robustness during initial bring-up, as it handles the full stack and flags restoration more predictably.
3. **Register-Based Calling Convention**:
   - `rax`: Syscall number
   - `rdi`, `rsi`, `rdx`, `r10`, `r8`, `r9`: Arguments
   - `rax`: Return value
   - **Rationale**: Follows the System V AMD64 ABI, making it easier to port standard libraries.
4. **ELF for User Executables**: All user processes are loaded from standard 64-bit ELF files.
   - **Rationale**: Leverages existing toolchains (Rust, LLVM).

## Consequences
- The kernel must maintain a per-CPU `kernel_stack_ptr` in the `GS` base for `SYSCALL` entries.
- User processes are isolated in their own address spaces (cloned from kernel higher-half).
