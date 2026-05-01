# Roadmap

## Phase 0: Foundation

- repository scaffold
- docs structure
- host setup guide
- shared ABI crate
- xtask automation skeleton
- kernel crate skeleton

## Phase 1: First boot

- toolchain setup for UEFI and freestanding builds
- thin UEFI loader image
- explicit loader-to-kernel boot contract
- serial logging
- panic path
- QEMU run workflow
- prepared `x86_64-unknown-none` target for the next handoff step

See [Phase 1 First Boot Plan](phase-1-first-boot.md).

## Phase 2: Freestanding handoff

- separate freestanding kernel ELF image
- custom kernel linker layout
- UEFI filesystem read of the kernel image
- kernel image loaded into memory from the UEFI loader
- `ExitBootServices` plus memory-map handoff
- transfer of control into the freestanding kernel entry point

See [Phase 2 Freestanding Handoff](phase-2-freestanding-handoff.md).

## Phase 3: Physical memory bring-up

- boot-memory-map iteration helpers
- kernel-side memory summary
- bump-style physical frame allocator over conventional memory
- low-memory skip policy for early safety
- serial proof that the kernel can hand out page-frame addresses

See [Phase 3 Physical Memory Bring-Up](phase-3-memory-bringup.md).

## Phase 4: Interrupts and Timers

- GDT, IDT, and TSS
- LAPIC timer interrupts
- stable kernel-thread dispatch baseline

See [Phase 4 Stable Kernel Tasks](phase-4-stable-kernel-tasks.md).

## Phase 5: Execution (Complete)

- [x] User-mode task model (Ring 3 transition)
- [x] Preemptive scheduler supporting user processes
- [x] `SYSCALL`/`SYSRET` interface (ABI v3)
- [x] ELF loader for user executables
- [x] `init` process loading and execution
- [x] Basic `VFS` and `initramfs` integration

## Phase 6: Terminal-first Usability (In Progress)

- [ ] Interactive `shell`
- [ ] Process lifecycle management (`fork`/`exec`)
- [ ] Pipes and IPC
- [ ] Basic networking stack
- [ ] Keyboard input via IOAPIC

## Phase 7: Modern desktop path

- display and input stack
- Wayland-oriented compositor model
- package/update story
- sandboxed applications
- session and desktop services
