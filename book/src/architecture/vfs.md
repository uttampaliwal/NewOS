# Virtual File System

Turnix provides a VFS abstraction layer (`kernel/src/fs/vfs.rs` and `kernel/src/vfs.rs`)
that unifies access to multiple filesystem backends.

## Mount Table

The VFS maintains a mount table mapping paths to filesystem backends. At boot, the root
(`/`) is mounted as `tmpfs` and `/mnt` is mounted as `ext4` in read-write mode. Additional
mounts can be added via the `mount` syscall.

## Filesystem Backends

- **tmpfs** (`kernel/src/fs/tmpfs.rs`): An in-memory filesystem used for the root
  filesystem and temporary storage. All data lives in kernel heap pages.
- **ext4** (`kernel/src/fs/ext4/`): A block-device-backed filesystem supporting standard
  POSIX operations. Reads and writes go through the NVMe block driver.

## File Descriptors

Each process maintains a file descriptor table (up to 1024 entries). Standard `stdin`,
`stdout`, and `stderr` (fds 0-2) are inherited across `fork` and `exec`. The `dup` and
`dup2` syscalls support I/O redirection.

## Operations

The VFS dispatches `open`, `read`, `write`, `seek`, `stat`, `mkdir`, and `unlink` calls
to the appropriate backend through a trait-object interface (`Box<dyn FsBackend>`).
