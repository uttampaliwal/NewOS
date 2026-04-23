# Roadmap

## Phase 0: Foundation

- repository scaffold
- docs structure
- host setup guide
- shared ABI crate
- xtask automation skeleton
- kernel crate skeleton

## Phase 1: First boot

- toolchain setup for freestanding builds
- Limine integration
- bootable kernel image
- serial logging
- panic path
- QEMU run workflow

See [Phase 1 First Boot Plan](C:\Users\uttam\development\NewOS\docs\phase-1-first-boot.md).

## Phase 2: Bring-up

- GDT and IDT
- interrupts and timer
- physical memory map parsing
- page frame allocator
- kernel heap allocator

## Phase 3: Execution

- kernel tasks
- syscall entry
- user-mode transition
- ELF loading
- init process

## Phase 4: Terminal-first usability

- VFS
- initramfs
- shell
- files, pipes, and process lifecycle
- basic networking

## Phase 5: Modern desktop path

- display and input stack
- Wayland-oriented compositor model
- package/update story
- sandboxed applications
- session and desktop services
