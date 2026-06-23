# Inter-Process Communication

Turnix provides multiple IPC mechanisms implemented in `kernel/src/ipc/`.

## Pipes

Pipes (`kernel/src/ipc/pipe.rs`) are unidirectional byte streams with a 64 KiB ring
buffer allocated via the slab allocator. They support full blocking semantics: readers
block when the buffer is empty and writers block when it is full. `SIGPIPE` is delivered
to the writer when the read end is closed. Pipes are created via the `pipe` syscall and
share file descriptors across `fork`. State changes trigger global epoll notifications.

## Unix Domain Sockets

Unix domain sockets (`kernel/src/ipc/unix_socket.rs`) provide bidirectional communication
between processes. Sockets are bound to filesystem paths (typically under `/tmp/`) and
maintained in a global `BOUND_SOCKETS` registry. They support `bind`, `listen`, `accept`,
`connect`, and `shutdown` operations. State changes trigger global epoll notifications.

## Epoll

Epoll (`kernel/src/ipc/epoll.rs`) provides event-driven I/O multiplexing. Processes create
an epoll instance via `epoll_create`, register file descriptors via `epoll_ctl` with
interest masks (`EPOLLIN`, `EPOLLOUT`, `EPOLLRDHUP`), and wait for events via `epoll_wait`.
The wait is blocking — processes are added to a `blocked_waiters` list and woken when any
monitored file descriptor changes state. Global `notify_all_epoll_waiters()` is called by
pipes, sockets, and message queues on state changes.

## Futex

Futex (`kernel/src/ipc/futex.rs`) provides fast userspace synchronization primitives.
`FUTEX_WAIT` blocks a process if the futex value matches the expected value. `FUTEX_WAKE`
wakes up to N blocked processes. Uses a hash-table-based wait queue for efficient lookup.

## POSIX Message Queues

POSIX message queues (`kernel/src/ipc/mqueue.rs`) provide structured message passing.
Queues are created via `mq_open`, messages are sent via `mq_send` and received via
`mq_receive`. Both operations support blocking semantics. Queue state changes trigger
global epoll notifications. Queues are identified by name and tracked in a global registry.

## POSIX Shared Memory

POSIX shared memory (`kernel/src/ipc/shm.rs`) provides shared memory regions between
processes. `shm_open` creates or opens a shared memory object backed by VFS (under
`/dev/shm/`), and `shm_unlink` removes it. Memory is mapped via the `mmap` syscall.

## IPC Broker

The IPC Broker daemon (`userland/ipc-broker/`) manages socket lifecycle and message
routing for higher-level services. It uses the shared IPC protocol definitions from
`shared/ipc-proto/`.

## Ring Buffers

Pipes share a common ring-buffer implementation for efficient zero-copy-adjacent data
transfer between producer and consumer contexts.
