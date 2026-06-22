# ADR 0004: GDT, IDT, and Hardware Interrupts

**Status:** Accepted  
**Date:** 2026-04-24  
**Author:** Antigravity

## Context

After establishing physical memory management (Phase 3) and a kernel heap, the next
essential step is hardware interrupt handling. Without interrupts, the kernel has no
way to respond to hardware events (timers, keyboards, disk I/O) — everything must
be polled. Additionally, the CPU needs a proper GDT and TSS to handle privilege
levels and exception stack switching.

## Decisions

### 1. GDT with TSS and Interrupt Stack Table (IST)

We load a kernel GDT containing:

- **Kernel code segment** — required for `CS` in long mode.
- **Kernel data segment** — reserved for future ring-3 transitions.
- **TSS segment** — carries the Interrupt Stack Table.

A dedicated 20 KiB double-fault stack is registered in IST slot 0. This prevents
triple faults: if a page fault occurs during exception handling, the CPU switches
to this clean stack instead of faulting again on a corrupted one.

**Justification:** Without an IST, a stack overflow silently becomes a triple fault
and an instant reboot with no diagnostic output. The IST catches it as a clean
double-fault panic with a full register dump.

### 2. Data segment registers set to null selector

In x86_64 long mode, DS/ES/SS base and limit are architecturally ignored (always
base=0, limit=max). We load these with the null selector `SegmentSelector(0)`,
which is the standard approach used by Linux. This avoids compatibility issues
with WHPX and other hypervisors that perform strict segment validation.

### 3. pic8259 crate for PIC management

We use the `pic8259` crate rather than hand-rolling the PIC initialization
sequence.

**Justification:** The 8259 PIC is legacy hardware with well-known initialization
quirks (ICW1-ICW4 sequence, edge cases around spurious IRQ7/IRQ15). The `pic8259`
crate is a thin, no-dependency wrapper that handles these correctly. Building from
scratch provides minimal educational value relative to the bug surface. When we
eventually move to the APIC, this module will be cleanly replaced.

### 4. Boot stack increased from 16 KiB to 64 KiB

The `InterruptDescriptorTable` struct is ~4 KiB. Combined with `lazy_static`
closure frames and `format_args!` temporaries in debug builds, the original 16 KiB
boot stack overflowed silently (causing a triple fault). 64 KiB provides
comfortable headroom for the boot path while remaining small enough to not waste
memory.

### 5. lazy_static for IDT and GDT storage

Both the IDT and GDT use `lazy_static!` to provide `'static` lifetime references
without requiring mutable statics or `unsafe` initialization. The `spin_no_std`
feature provides the underlying spinlock for `no_std` environments.

## Consequences

- The kernel can now receive and handle hardware timer interrupts.
- CPU exceptions (breakpoint, page fault, GPF, double fault) produce diagnostic
  panic output instead of silent triple faults.
- Future work (keyboard driver, APIC, scheduler preemption) can build directly
  on this interrupt infrastructure.
- The data segment selector is already in the GDT for the future ring-3 transition.
