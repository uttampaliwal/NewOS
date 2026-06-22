# Seccomp-BPF

Turnix implements Linux-compatible seccomp (secure computing) using classic
BPF filters to restrict system calls per-process.

---

## Overview

Seccomp-BPF allows processes to install a filter program that is evaluated
on every system call. The filter can allow, deny, or kill the process based
on the syscall number and its arguments.

---

## Architecture

```
User Process
    │
    ├── prctl(PR_SET_SECCOMP, filter_program)
    │
    ▼
Syscall Entry (int 0x81)
    │
    ├── SeccompFilter::evaluate(data)
    │     ├── Load syscall number
    │     ├── Load arguments (rdi, rsi, rdx, r10, r8, r9)
    │     ├── Execute BPF instructions
    │     └── Return action (Allow / Kill / Errno / Trap / Trace)
    │
    ▼
Syscall Handler (if allowed)
```

---

## BPF Instruction Format

```rust
pub struct BpfInstruction {
    pub code: u16,  // Operation code + mode flags
    pub jt: u8,     // Jump-true offset
    pub jf: u8,     // Jump-false offset
    pub k: u32,     // Constant / offset
}
```

### Supported Operations

| Class | Instructions | Purpose |
|-------|-------------|---------|
| **Load (LD)** | LD IMM, LD ABS, LD IND, LD LEN, LD MEM | Load values from seccomp_data |
| **Store (ST)** | ST, STX | Store to scratch memory |
| **Arithmetic (ALU)** | ADD, SUB, MUL, DIV, OR, AND, LSH, RSH, NEG, MOD, XOR | Arithmetic operations |
| **Jump (JMP)** | JA, JEQ, JGT, JGE, JSET | Conditional branches |
| **Return (RET)** | RET | Return allow/kill/errno |
| **Misc** | TAX, TXA | A <-> X register transfer |

### Data Access Modes

| Mode | Source |
|------|--------|
| `LD IMM` | Immediate constant |
| `LD ABS` | Direct field offset in `SeccompData` |
| `LD IND` | Indirect: `base + X register` |
| `LD LEN` | Size of buffer argument |
| `LD MEM` | Scratch memory slot |

---

## Actions

| Action | Behavior |
|--------|----------|
| `Allow` | Permit the syscall |
| `KillProcess` | Kill the entire process (SIGSYS) |
| `KillThread` | Kill the current thread |
| `Errno(errno)` | Return error code to userspace |
| `Trap` | Send SIGTRAP to the process |
| `Trace` | Allow with ptrace notification |

---

## Filter Installation

```rust
// Create filter with up to 4096 instructions
let filter = SeccompFilter::new(instructions)?;

// Install on current process
prctl(PR_SET_SECCOMP, &filter);
```

### Constraints

- Maximum 4096 BPF instructions per filter
- Empty filters are rejected
- Once installed, filters can only be made more restrictive
- The `no_new_privs` flag is set automatically on installation

---

## Inheritance

Filters are inherited across `fork()` and `exec()`:

```rust
pub fn inherit_on_fork(&self) -> SeccompFilter {
    SeccompFilter {
        instructions: self.instructions.clone(),
        no_new_privs: self.no_new_privs,
    }
}
```

Child processes inherit an identical copy of the parent's filter. The
`no_new_privs` flag prevents children from installing less restrictive
filters.

---

## Safety Properties

1. **Division by zero**: Returns 0 (safe, not a crash)
2. **Out-of-bounds memory access**: Returns KillThread (prevents exploit)
3. **Unknown opcodes**: Returns KillThread
4. **Filter chain**: Multiple filters can be stacked; first deny wins

---

## Example Filter

Allow only `read`, `write`, `exit`, and `exit_group`:

```rust
use turnix_abi::Syscall;

let filter = SeccompFilter::new(vec![
    // Load syscall number
    BpfInstruction { code: 0x0015, jt: 0, jf: 4, k: Syscall::Read as u32 },
    // If read (nr == 3), allow
    BpfInstruction { code: 0x0006, jt: 0, jf: 0, k: 0x7fff0000 }, // ALLOW
    // If write (nr == 1), allow
    BpfInstruction { code: 0x0015, jt: 0, jf: 1, k: Syscall::Write as u32 },
    BpfInstruction { code: 0x0006, jt: 0, jf: 0, k: 0x7fff0000 },
    // Default: kill
    BpfInstruction { code: 0x0006, jt: 0, jf: 0, k: 0x00030000 },
])?;
```
