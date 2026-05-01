# ADR 0001: Kernel Architecture and Memory Model

## Status
Accepted

## Context
<<<<<<< HEAD
turnix needs a robust foundation for a modern desktop operating system. We need to decide on the fundamental kernel model and memory layout to ensure scalability and security.
=======
NewOS needs a robust foundation for a modern desktop operating system. We need to decide on the fundamental kernel model and memory layout to ensure scalability and security.
>>>>>>> unstable

## Decision
1. **Higher-Half Kernel**: The kernel is mapped to the higher half of the virtual address space (starting at `0xffffffff80000000`). 
   - **Rationale**: This leaves the lower half (canonical addresses) entirely for user-mode processes, simplifying memory management for applications and allowing the kernel to stay mapped during context switches.
2. **Monolithic Design (Phase 1)**: For initial development, we use a monolithic design where core services (Memory, VFS, Scheduler) run in kernel space.
   - **Rationale**: Reduces complexity and inter-process communication (IPC) overhead during the bring-up phase. We may transition to a hybrid model later.
3. **Rust-First Implementation**: The entire kernel and bootloader are written in Rust.
   - **Rationale**: Memory safety without a garbage collector is essential for OS development. Rust's type system helps enforce invariants like "kernel stacks must be mapped before use".

## Consequences
- Kernel addresses are isolated from user-mode via page table flags (`USER_ACCESSIBLE` bit).
- Physical memory is mapped linearly at a high offset (`0xffff800000000000`) to allow easy access to any physical frame.
