# Observability Commands

Turnix provides a set of `no_std` diagnostic utilities in `userland/observability/` for
inspecting system state.

## Commands

- **ps** (`bin/ps.rs`): Lists all running processes with PID, PPID, state, and name.
  Reads the kernel process table via the `ls` syscall.
- **meminfo** (`bin/meminfo.rs`): Displays memory usage statistics including total
  frames, free frames, and swap utilization.
- **mount** (`bin/mount.rs`): Lists all active mount points and their filesystem backends.
- **lsns** (`bin/lsns.rs`): Enumerates active namespaces (PID, mount, network, user) and
  their associated processes.
- **capsh** (`bin/capsh.rs`): Displays the POSIX capability sets (permitted, effective,
  inheritable, bounding, ambient) for the current process.

## Usage

These commands are designed for interactive use from the Turnix shell and for automated
health checks in the init daemon's startup sequence. They use `libturnix` for all
kernel interaction.
