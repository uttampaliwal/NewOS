# turnix Scheduler

## Model: Round Robin (RR)
turnix currently implements a simple Round Robin (RR) scheduler.

### Key Characteristics
- **Preemptive:** The scheduler can interrupt a running task to give other tasks a chance to run.
- **Fairness:** Each task in the "Ready" queue is given an equal time slice (quantum).
- **Simplicity:** High performance with low overhead for first-bringup.

## Implementation Details

### 1. Ready Queue
The scheduler maintains a `VecDeque<Task>` containing all tasks that are in the `Ready` state.

### 2. Timer Interrupt
The Local APIC timer is configured to trigger a periodic interrupt (currently at a rate determined by the `0x10000` count). 
When the timer interrupt occurs:
1. The CPU saves the current task's state (RIP, CS, RFLAGS, RSP, SS) on the kernel stack.
2. The assembly entry point saves the remaining general-purpose registers.
3. `timer_tick` is called with the current stack pointer.
4. The scheduler puts the current task back into the `Ready` queue (if it was still `Running`).
5. The scheduler picks the next task from the front of the `Ready` queue.
6. Hardware state (CR3, TSS) is updated for the new task.
7. The new task's stack pointer is returned and restored by the assembly wrapper.

### 3. Context Switching
Context switching is performed during the timer interrupt or when a task explicitly yields.
- **Address Space Switch:** If the next task belongs to a different process, the `CR3` register is updated with the new process's PML4 frame.
- **Stack Switch:** The `RSP` is changed to the next task's saved stack pointer.
- **Kernel Stack for Interrupts:** The TSS is updated with the next task's `kernel_stack_top` so that future interrupts/syscalls use the correct stack.

### 4. Yielding
A task can voluntarily give up its time slice by calling the `Yielder` syscall, which triggers a software interrupt (`int 0x81`).

### 5. Task States
- `Ready`: Waiting to be scheduled.
- `Running`: Currently executing on the CPU.

## Related Decisions
For the scheduling algorithm rationale and future plans, see [ADR 0003: Preemptive Scheduler Design](decisions/0003-scheduler-design.md).
