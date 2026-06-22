# Shell

The Turnix interactive shell is implemented in `userland/shell/src/main.rs` as a freestanding
`no_std` binary.

## Features

The shell reads lines from `/dev/tty` and dispatches built-in commands. It uses the
`libturnix` syscall shims for all I/O. Built-in commands include:

- `help` — list available commands
- `uptime` — display system uptime
- `pid` — print the shell's PID
- `ls` — list directory contents
- `clear` — clear the terminal
- `exit` — terminate the shell

## Design

The shell is intentionally minimal. It does not implement scripting, environment variable
expansion, or external command execution. It serves as a reference userland application and
a tool for interactively testing kernel syscalls and VFS operations.
