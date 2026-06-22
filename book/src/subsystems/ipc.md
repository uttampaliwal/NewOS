# Inter-Process Communication

Turnix provides two primary IPC mechanisms implemented in `kernel/src/ipc/`.

## Pipes

Pipes (`kernel/src/ipc/pipe.rs`) are unidirectional byte streams with a 64 KiB ring
buffer. They support full blocking semantics: readers block when the buffer is empty and
writers block when it is full. `SIGPIPE` is delivered to the writer when the read end is
closed. Pipes are created via the `pipe` syscall and share file descriptors across `fork`.

## Unix Domain Sockets

Unix domain sockets (`kernel/src/ipc/unix_socket.rs`) provide bidirectional communication
between processes. Sockets are bound to filesystem paths (typically under `/tmp/`) and
maintained in a global `BOUND_SOCKETS` registry. They support `bind`, `listen`, `accept`,
`connect`, and `shutdown` operations.

## IPC Broker

The IPC Broker daemon (`userland/ipc-broker/`) manages socket lifecycle and message
routing for higher-level services. It uses the shared IPC protocol definitions from
`shared/ipc-proto/`.

## Ring Buffers

Both pipes and sockets share a common ring-buffer implementation for efficient
zero-copy-adjacent data transfer between producer and consumer contexts.
