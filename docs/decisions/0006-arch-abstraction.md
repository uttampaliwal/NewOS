# ADR 0006: Architecture Abstraction Boundary

## Status
Accepted

## Context
The Turnix kernel was initially developed with a focus on x86_64. To support future architectures like AArch64, we need to abstract architecture-specific code (GDT, IDT, context switching, syscall entry/exit) behind a unified boundary.

## Decision
1.  All architecture-specific code will reside in `kernel/src/arch/<arch>/`.
2.  The `kernel/src/arch/mod.rs` defines a common interface (as a trait or via re-exports) for these components.
3.  We use compile-time feature flags (`arch-x86_64`, `arch-aarch64`) to select the active architecture.
4.  Re-export shims in `kernel/src/lib.rs` maintain backward compatibility for existing module paths like `crate::gdt`, `crate::interrupts`, and `crate::context`.

## Consequences
-   **Multi-arch support:** The kernel core becomes platform-agnostic, simplifying porting to new architectures.
-   **Cleaner code:** Hardware-specific details are isolated from high-level kernel logic.
-   **Build system complexity:** Adding a new architecture requires updating `Cargo.toml` and `.cargo/config.toml`, and providing stub implementations for all required arch services.
-   **No runtime overhead:** By using compile-time selection, we avoid the overhead of dynamic dispatch for architecture-specific operations.

## Architecture Equivalents

| Mechanism | x86_64 | AArch64 |
| :--- | :--- | :--- |
| Segments/Privilege | GDT (Global Descriptor Table) | ELs (Exception Levels), VBAR_EL1 |
| Interrupts | IDT (Interrupt Descriptor Table), LAPIC/IOAPIC | EL1 Exception Vector Table, GIC (Generic Interrupt Controller) |
| Context Switch | CR3 (Page Table Base), Registers | TTBR0_EL1/TTBR1_EL1, Registers |
| Syscalls | SYSCALL/SYSRET instructions, MSRs | SVC instruction, VBAR_EL1 handler |
| Memory Barriers | `mfence`, `sfence`, `lfence` | `DMB`, `DSB`, `ISB` |
