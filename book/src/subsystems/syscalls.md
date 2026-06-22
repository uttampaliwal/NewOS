# System Call Interface

Turnix exposes 58 system calls through a ring 3 to ring 0 transition mechanism defined
in `kernel/src/syscall/handler.rs` and the shared ABI crate (`shared/abi/`).

## Transition Mechanism

User-space code invokes syscalls via the `libturnix` shims, which load the syscall number
and arguments into registers and execute a dedicated `syscall` instruction. The kernel
entry point saves user registers, validates the syscall number, and dispatches to the
appropriate handler.

## Syscall Categories

- **Process**: `fork`, `exec`, `clone`, `exit`, `wait`, `waitpid`, `getpid`, `getuid`,
  `getgid`, `setuid`, `setgid`, `yielder`
- **Memory**: `brk`, `mmap`, `munmap`, `mmap_framebuffer`
- **File I/O**: `open`, `close`, `read`, `write`, `seek`, `stat`, `ls`, `write_file`,
  `mkdir`, `unlink`
- **IPC**: `pipe`, `socket`, `bind`, `listen`, `accept`, `connect`, `sendto`, `recvfrom`,
  `shutdown`
- **Signals**: `kill`, `signal`
- **System**: `uptime`, `reboot`, `shutdown`, `mount`, `umount`, `lsns`, `capset`,
  `capget`, `seccomp`, `sethostname`
- **GPU**: `drm_open`, `drm_get_properties`, `drm_set_mode`, `drm_add_framebuffer`,
  `drm_page_flip`, `drm_handle_event`

## Argument Validation

Every handler validates pointer arguments for correct alignment, user-space origin, and
bounds before dereferencing. Seccomp-BPF filters (`kernel/src/security/`) can restrict
which syscalls a process is allowed to invoke.
