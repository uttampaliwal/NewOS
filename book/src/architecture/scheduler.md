# Scheduler

The Turnix scheduler is a preemptive round-robin design implemented in
`kernel/src/task/scheduler.rs`.

## Core Design

A global `Scheduler` struct holds a `VecDeque<Task>` ready queue and tracks the currently
running task. A static `Mutex<Scheduler>` protects all scheduler state. The PIC8259 timer
interrupt fires at a fixed quantum, at which point the scheduler preempts the running task
and rotates it to the back of the queue.

## Task Lifecycle

Tasks are created via `fork` or `clone` syscalls. Each task carries a `TaskId`, a saved
kernel stack pointer, and a reference to its parent `Process`. When a task blocks (e.g.,
waiting for I/O or a child to exit), it is moved to a `blocked_tasks` vector and skipped
during scheduling.

## Context Switching

On each timer tick the scheduler performs a context switch: the current task's register
state (including the kernel stack pointer) is saved, and the next task's state is restored.
The actual switch is implemented in architecture-specific assembly under `kernel/src/arch/`.

## Uptime Tracking

A global atomic `UPTIME_TICKS` counter is incremented on every timer interrupt, providing
a monotonic clock for the `uptime` syscall and relative timeouts.
