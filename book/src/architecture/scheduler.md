# Scheduler

The Turnix scheduler is a preemptive CFS-style design implemented in
`kernel/src/task/scheduler.rs` with a pluggable class framework in
`kernel/src/task/scheduler_class.rs`.

## Core Design

A global `Scheduler` struct holds a `VecDeque<Task>` ready queue and tracks the currently
running task. A static `Mutex<Scheduler>` protects all scheduler state. The PIC8259 timer
interrupt fires at a fixed quantum, at which point the scheduler preempts the running task.

## Scheduler Classes

Tasks are assigned a `SchedulingPolicy` that determines their scheduling behavior:

- **SCHED_NORMAL** (default): CFS-style fair scheduling with virtual runtime tracking
- **SCHED_BATCH**: Batch processing with lower priority
- **SCHED_FIFO**: Real-time FIFO (highest static priority wins)
- **SCHED_RR**: Real-time round-robin with time quantum
- **SCHED_IDLE**: Lowest priority idle tasks

## Virtual Runtime (CFS)

For SCHED_NORMAL and SCHED_BATCH tasks, the scheduler tracks a `vruntime` (virtual
runtime) per task. On each timer tick, `vruntime` is incremented by 1 for the running
task. The scheduler always selects the task with the lowest `vruntime` from the ready
queue, ensuring fair CPU time distribution.

## Task Lifecycle

Tasks are created via `fork` or `clone` syscalls. Each task carries a `TaskId`, a saved
kernel stack pointer, a `vruntime` counter, and a reference to its parent `Process`. When
a task blocks (e.g., waiting for I/O or a child to exit), it is moved to a `blocked_tasks`
vector and skipped during scheduling.

## Context Switching

On each timer tick the scheduler performs a context switch: the current task's register
state (including the kernel stack pointer) is saved, and the next task's state is restored.
The actual switch is implemented in architecture-specific assembly under `kernel/src/arch/`.

## Per-CPU Scheduling

On SMP systems, each CPU has its own scheduling queue indexed by CPU ID. The BSP
(Bootstrap Processor) manages the boot CPU, while Application Processors (APs) are
brought up via SIPI sequence and each run their own scheduler instance.

## Resource Control (cgroups v2)

The scheduler integrates with cgroups v2 for CPU quota enforcement. Each cgroup tracks
`cpu_used` ticks. When `cpu_used >= cpu_max`, the scheduler preempts the process and
resets the counter. This provides bandwidth limiting without modifying the core scheduling
algorithm.

## Uptime Tracking

A global atomic `UPTIME_TICKS` counter is incremented on every timer interrupt, providing
a monotonic clock for the `uptime` syscall and relative timeouts.
