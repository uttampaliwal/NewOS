# ADR 0005: Process and Thread Model

## Status
Accepted

## Context
turnix needs a clear distinction between execution units and resource containers to support multi-threaded applications, complex process hierarchies, and robust resource management.

## Decision
We adopt a classic Unix-like separation between **Processes** and **Threads** (represented by `Task` objects).

### 1. Process
- **Address Space**: A process owns a unique 4-level page table (PML4).
- **Resource Ownership**: Owns file descriptors (VFS handles), memory regions (VMAs), and metadata (UID/GID).
- **ID**: Has a unique `ProcessId`.
- **Relationship**: Contains one or more threads.

### 2. Thread (Task)
- **Unit of Execution**: The entity scheduled by the kernel.
- **State**: Owns its register state and execution context.
- **Stacks**: Has a private kernel stack and a private user-mode stack.
- **Relationship**: Belongs to exactly one process. Shares the process's address space and resources with sibling threads.

### 3. Implementation (Rust)
- **`Process`**: A `Clone`-able wrapper around `Arc<Mutex<ProcessInner>>`. This allows multiple threads to safely share and modify process-level state.
- **`Task`**: Contains its own stack pointer, unique `TaskId`, and a reference back to its parent `Process`.

### 4. Lifecycle
- **Creation**:
    - `init`: The first process, loaded from an ELF by the kernel.
    - `fork`: Creates a new process by cloning the current address space and metadata (deep copy).
    - `spawn`: (Future) Creates a new thread within the *same* process address space.
- **Termination**: A thread can exit individually. If the last thread of a process exits, the process exits and its address space is reclaimed.

## Consequences
- **Concurrency**: Requires strict synchronization (e.g., `Mutex`) when accessing process-shared resources like the open file table.
- **Flexibility**: Provides a solid foundation for standard Unix primitives (`fork`/`exec`) and modern multi-threading.
- **Overhead**: Address space cloning (`fork`) is expensive; we utilize CoW (Copy-on-Write) strategies in the future to mitigate this.
