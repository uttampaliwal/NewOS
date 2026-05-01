# ADR 0003: Preemptive Scheduler Design

## Status
Accepted

## Context
turnix needs to support multiple concurrent execution flows (threads) and processes to provide a modern, responsive user experience. We need a scheduling algorithm and implementation that is fair, efficient, and robust.

## Decision
1. **Round-Robin (RR) Algorithm**: We implement a preemptive Round-Robin scheduler for the initial stable release.
   - **Rationale**: RR is simple to implement correctly and ensures that no task can monopolize the CPU. It provides a baseline of fairness that is sufficient for early system development.
2. **Fixed Time Quantum**: Each task is given a fixed time slice (quantum) determined by the Local APIC timer frequency.
3. **Task-Based Execution (Thread Model)**: The scheduler manages `Task` objects, which represent individual threads of execution.
   - **Rationale**: Decoupling execution units (`Tasks`) from resource containers (`Processes`) allows for multi-threading within a single address space (ADR 0005).
4. **Preemption via LAPIC Timer**: We use the Local APIC timer to trigger a periodic interrupt that invokes the scheduler's `timer_tick` logic.
   - **Mechanism**: The timer interrupt handler saves the current task context, switches to the next task in the `Ready` queue, and returns to the new task's context.
5. **Ready Queue Management**: A global `VecDeque` is used to store tasks that are ready to run.
   - **Performance**: While $O(1)$ complexity for insertion/deletion, we use a `Mutex` for synchronization, which is acceptable for single-core bring-up.

## Implementation Details
- **Context Frame**: The task stack stores the full CPU state (15 general-purpose registers + IRETQ frame).
- **Yielding**: A software interrupt (`int 0x81`) is provided to allow tasks to voluntarily give up their time slice before the quantum expires.
- **Task States**:
  - `Ready`: In the queue, waiting for CPU time.
  - `Running`: Currently executing.
  - `Blocked`: Waiting for I/O or a lock (future expansion).
  - `Zombie`: Execution finished, waiting for resource cleanup.

## Consequences
- **Fairness**: Every task gets an equal chance to run.
- **Overhead**: Frequent timer interrupts introduce some overhead; the quantum must be tuned (initially ~10ms).
- **Scalability**: The simple `VecDeque` and global `Mutex` will need to be replaced with per-CPU runqueues and more advanced priority logic (e.g., Multi-Level Feedback Queue) as we move toward SMP (Symmetric Multi-Processing).
