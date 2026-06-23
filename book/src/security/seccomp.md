> For detailed reference documentation, see [docs/security/](../../../docs/security/)

# Seccomp-BPF

Seccomp (Secure Computing Mode) filters system calls using BPF programs.

## How It Works

Processes install a BPF filter that evaluates each syscall. The filter
returns ALLOW, KILL, or TRAP based on syscall number and arguments.

## Inheritance

Seccomp filters are inherited by child processes during `fork()` and
preserved across `exec()`, ensuring security policies persist.

## Actions

- `SECCOMP_RET_ALLOW`: Permit the syscall
- `SECCOMP_RET_KILL_PROCESS`: Kill the process
- `SECCOMP_RET_TRAP`: Send SIGSYS to the process

For full details, see `docs/security/seccomp.md`.
