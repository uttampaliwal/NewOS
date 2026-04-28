# Phase 4: Stable Kernel Tasks

## Why this phase exists

The earlier bring-up mixed three different milestones at once:

- higher-half kernel boot
- preemptive scheduling
- early user-mode/process experiments

That made the first `iretq` path fragile and hid the real learning boundary. This phase resets the execution model to a cleaner baseline: the kernel owns one address space, each task gets a real mapped kernel stack, and the scheduler only switches between kernel tasks.

## What changed

- task stacks are now mapped into the active higher-half kernel address space directly
- the initial dispatch frame matches the timer interrupt save/restore path
- the scheduler starts from kernel tasks, not copied pseudo-user code
- the timer interrupt gate stays ring-0 only
- the heap remains supervisor-only
- `cargo xtask uefi-loader` is kept as a compatibility alias for `cargo xtask run-uefi`

## Why this is the better architecture

- It removes an unsound assumption: copying bytes from a kernel Rust function is not a valid user-program loader.
- It gives us a trustworthy preemption baseline before we add ABI, ELF relocation, or syscall-backed user mode.
- It keeps the boot path short enough to debug on Windows + QEMU while still teaching real kernel mechanics.

## What the kernel proves now

- higher-half freestanding handoff works
- paging and heap initialization work
- GDT, IDT, TSS, LAPIC timer, and interrupt dispatch work
- multiple kernel tasks can be scheduled without corrupting the first return frame

## What comes next

User mode is still a goal, but it needs a proper boundary:

- dedicated user binaries, not copied kernel text
- a stable syscall ABI
- explicit user virtual-memory layout
- loader support for user ELF images
- fault handling and teardown rules for bad user tasks

That is the next execution milestone after this stabilized kernel-thread baseline.
