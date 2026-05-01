# ADR 0005: Process and Thread Model

## Status
Proposed

## Context
<<<<<<< HEAD
turnix needs a clear distinction between processes and threads to support multi-threaded applications and robust resource management. Currently, `Process` and `Task` structs exist but their relationship is loosely defined.
=======
NewOS needs a clear distinction between processes and threads to support multi-threaded applications and robust resource management. Currently, `Process` and `Task` structs exist but their relationship is loosely defined.
>>>>>>> unstable

## Decision
We will adopt a model where a **Process** is a container for resources and an **Address Space**, while a **Thread** (represented by `Task`) is the unit of execution.

### 1. Process
- Represents a single address space (PML4).
- Owns resources like file descriptors (VFS handles).
- Contains one or more threads.
- Has a unique `ProcessId`.
- When a process exits, all its threads are terminated, and its resources are reclaimed.

### 2. Thread (Task)
- Represents a single execution flow.
- Has its own stack (kernel and user).
- Has its own register state.
- Belongs to exactly one process.
- Has a unique `TaskId`.
- Scheduled by the kernel scheduler.

### 3. Relationship
- `Process` struct will maintain a list of associated `TaskId`s.
- `Task` struct will maintain a reference (e.g., `Arc<Process>`) to its parent process.

### 4. Lifecycle
- **Process Creation:** Created via `exec` (initial process) or `fork`.
- **Thread Creation:** Currently implicitly created during process creation. Future `spawn` syscall will allow creating additional threads.
- **Termination:** A thread can exit individually. If the last thread of a process exits, the process exits. A process can also be killed, terminating all its threads.

## Consequences
- Clearer resource ownership.
- Foundation for multi-threading.
- Simplified scheduler (it only deals with `Task`s).
- Need to ensure thread-safe access to process-shared resources.
