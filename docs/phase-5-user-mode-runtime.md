# Phase 5: User Mode and ELF Runtime

## Goal

Transition from kernel-mode tasks to true user-mode processes loaded from ELF binaries in the initramfs.

## Why this matters

True process isolation is a prerequisite for a SOTA operating system. This phase moves us from "kernel threads running in user-address-spaces" to "user binaries running in restricted rings".

## Expected Deliverables

- [ ] **initramfs image creation**: Improved xtask to package multiple files.
- [ ] **Userland "Hello World"**: A separate Rust crate targeting `x86_64-unknown-none` (or a custom target).
- [ ] **ELF Process Loading**: Using the newly implemented `Process::new_from_elf`.
- [ ] **Stable Syscall ABI**: Formalized `SYSCALL` instruction handling.
- [ ] **Init Process**: The first user-mode process that starts the shell.

## Implementation Steps

1. **Userland Crate**: Create `userland/init` as a freestanding Rust binary.
2. **Xtask Packaging**: Update `xtask` to compile `userland/init` and package it into `initramfs.img`.
3. **Kernel Init**: Update `early_boot` to load `/init` from VFS as the first process.
4. **Syscall refinement**: Ensure `Write` and `Exit` syscalls work perfectly for the new init process.

## Success Criteria

- The kernel starts, initializes VFS from ramdisk.
- The kernel finds `/init` in the VFS.
- The kernel loads `/init` as a Ring 3 process.
- `/init` executes and successfully calls the `Write` syscall to print a message.
- `/init` successfully calls `Exit` to terminate.
