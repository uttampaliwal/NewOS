# turnix Architecture

## High-Level System Design

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                     User Space                             │
│  ┌─────────┐  ┌─────────┐  ┌─────────┐  ┌─────────┐  │
│  │  Shell  │  │ Apps   │  │ Utils  │  │ Games │  │ ...
│  └────┬────┘  └────┬────┘  └────┬────┘  └────┬────┘  │
│       │            │            │            │            │
│       └──────────┴──────────┴──────────┴──────────┘
│                          │
│                    ┌─────┴─────┐
│                    │ Syscall   │
│                    │ Interface │
│                    └─────┬─────┘
└──────────────────────────┼────────────────────────────────
                           │
┌──────────────────────────┼────────────────────────────────
│                    Kernel│Space                           │
│                    ┌─────┴─────┐                         │
│                    │  Syscall  │                         │
│                    │  Handler │                         │
│                    └─────┬─────┘                         │
│         ┌────────────────┼────────────────┐                │
│    ┌────┴────┐   ┌───┴───┐  ┌────┴────┐  ┌───┴───┐   │
│    │Memory  │   │  GDT  │  │  IDT  │  │ Timer │   │
│    │Manager │   │  TSS  │  │  PIC  │  │ IRQ  │   │
│    └────┬────┘   └───┬───┘  └───┬───┘  └───┬───┘   │
│         │             │          │          │         │
│    ┌────┴────┐      └──────────┴──────────┘         │
│    │ Frame  │                                      │
│    │ Alloc │                                      │
│    └──┬───┘                                      │
│       │                                           │
└──────┼────────────────────────────────────────────┘
       │
┌──────┼─────────────────────────────────────────────┐
│      │       Boot Services                        │
│  ┌───┴───┐    ┌──────────────┐    ┌──────────┐│
│  │ BIOS │    │ UEFI Boot  │    │  Kernel  ││
│  │/UEFI│    │  Loader    │    │  Entry   ││
│  └─────┘    └────────────┘    └──────────┘│
└─────────────────────��───────────────────────────┘
```

## Memory Layout

```
Higher Half Kernel (PML4)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
0xFFFF8000_00000000  +------------------+
                    |   Kernel Code  |
                    |   (read-only)|
0xFFFF8000_10000000  +------------------+
                    |   Kernel    |
                    |   Data     |
0xFFFF8000_20000000  +------------------+
                    |   Heap     |
                    |   (bump)   |
0xFFFF8000_30000000  +------------------+
                    |   Kernel   |
                    |   Stack   |
0xFFFF8000_40000000  +------------------+
                    |   Reserved |
                    |   (guard)  |
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

Lower Half (Identity Mapped)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
0x00000000_00000000  +------------------+
                    |   IVT / Real |
                    |   Mode      |
0x00000000_00100000  +------------------+
                    |   BIOS/UEFI |
                    |   Data      |
0x00000000_01000000  +------------------+
                    |   Kernel    |
                    |   Image    |
                    | (0x100000)|
0x00000000_10000000  +------------------+
                    |   Free     |
                    |   Memory   |
                    |            |
                    |            |
0x00000007_FC000000  +------------------+
                    |   APIC     |
0x00000007_FEC00000  +------------------+
                    |   I/O     |
                    |   Ports    |
0x00000007_FFF00000  +------------------+
                    |   Legacy   |
                    |   Areas    │
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```

## Boot Flow

```
┌─────────────────┐
│   Power On     │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  UEFI BIOS     │
│  (runs first) │
└──────┬────────┘
       │
       ▼
┌─────────────────────────────────────┐
│  UEFI Loader (boot/uefi-loader)     │
│  - Initialize console              │
│  - Parse memory map              │
│  - Load kernel.elf              │
│  - ExitBootServices              │
└──────────────┬────────────────────┘
               │
               ▼
┌─────────────────────────────────────────┐
│  Kernel Entry (kernel/src/main.rs)        │
│  - Set up GDT, IDT, TSS              │
│  - Initialize memory allocator       │
│  - Set up timer interrupts         │
│  - Jump to kernel_main()           │
└──────────────┬────────────────────────┘
               │
               ▼
┌─────────────────────────────────────────┐
│  Kernel Main                         │
│  - Initialize subsystems          │
│  - Start init process           │
│  - Enter idle loop             │
└─────────────────────────────────┘
```

## Interrupt Flow

```
Hardware Event (e.g., timer tick)
          │
          ▼
    ┌───────────┐
    │  CPU     │
    │triggers  │
    │IDT entry │
    └────┬────┘
         │
         ▼
    ┌───────────┐
    │ Interrupt │
    │ Handler   │
    │(kernel)   │
    └────┬────┘
         │
    ┌────┴────┐
    │ Save    │
    │ regs    │
    └────┬────┘
         │
         ▼
    ┌───────────┐
    │ Dispatch │
    │ to       │
    │ handler  │
    └────┬────┘
         │
    ┌────┴────┐
    │ Timer   │
    │ tick   │
    │ ->     │
    │ sched  │
    └────┬────┘
         │
    ┌────┴────┐
    │ Context │
    │ switch  │
    │ (co-op) │
    └────┬────┘
         │
         ▼
    ┌───────────┐
    │ IRET    │
    │(return)  │
    └─────────┘
```

## Syscall Interface

```
User Space                    Kernel Space
─────────                    ───────────
┌──────────┐                ┌──────────┐
│ write()  │──syscall──▶│sys_write │
│ read()   │──syscall──▶│sys_read │
│ fork()   │──syscall──▶│sys_fork  │
│ execve() │──syscall──▶│sys_exec │
│ exit()   │──syscall──▶│sys_exit │
│ mmap()   │──syscall──▶│sys_mmap │
└──────────┘                └──────────┘
      │                          │
      │                    ┌─────┴─────┐
      │                    │  Syscall  │
      │                    │  Table   │
      │                    └──────────┘
      │                          │
      ▼                          ▼
   Returns                  Returns
   to user                 to kernel
```

## Key Files

| Path | Purpose |
|------|---------|
| `boot/uefi-loader/src/main.rs` | UEFI entry point |
| `kernel/src/main.rs` | Kernel entry, early setup |
| `kernel/src/memory/mod.rs` | Frame allocator, heap |
| `kernel/src/interrupts/mod.rs` | GDT, IDT, TSS, PIC |
| `kernel/src/sched/mod.rs` | Task scheduling |
| `kernel/src/syscall/mod.rs` | Syscall handlers |
| `shared/abi/src/lib.rs` | Shared types |

See [docs/architecture.md](architecture.md) for details.
