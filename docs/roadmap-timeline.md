# NewOS Roadmap

```
Timeline: Building from Scratch to Desktop OS
=========================================

2026
----

Q1          Q2          Q3          Q4
|----|----|----|----|----|----|----|----|

Phase 0     Phase 1     Phase 2     Phase 3
███████      ███████     ███████     ███████
            Phase 4     Phase 5     Phase 6
                        ███████     ███████
                                    Phase 7
                                    ███████
```

## Phase Timeline

```
Month    1   2   3   4   5   6   7   8   9   10  11  12
        |   |   |   |   |   |   |   |   |   |   |   |
Phase 0 █████████
Phase 1         █████████
Phase 2                 ████████
Phase 3                         ██████████
Phase 4                                 ████████
Phase 5                                         ████████████
Phase 6                                                     ████████████
Phase 7                                                             ████████
```

## Milestone Progress

```
v0.0.1 ─────────────────────────────────────────────────────►
    │ Phase 0: Workspace scaffold
    │
v0.0.2 ─────────────────────────────────────────────────────────────►
    │ Phase 1: UEFI boot
    │
v0.0.3 ───────────────────────────────────────────────────────────────────►
    │ Phase 2-3: Kernel handoff + memory
    │
v0.0.4 ────────────────────────────────────────────────────────────────────►
    │ Phase 4: Interrupts + timers
    │ Phase 5: Cooperative multitasking
    │
v0.1.0 ────────────────────────────────────────────────────────────────────────►
    🚧 Phase 5: Syscalls + basic exec
    │
v0.2.0 ──────────────────────────────────────────────────────────────────────────►
    │ Phase 6: Terminal-first usability
    │
v1.0.0 ───────────────────────────────────────────────────────────────────────────────────►
    │ Phase 7: Wayland desktop
    │
```

## Phase Details

| Phase | Name | Target | Status | Key Features |
|-------|------|--------|--------|-------------|
| 0 | Foundation | Q1 2026 | ✅ Complete | Workspace, docs, shared/abi |
| 1 | First Boot | Q1 2026 | ✅ Complete | UEFI loader, serial output |
| 2 | Freestanding | Q2 2026 | ✅ Complete | Kernel ELF, boot handoff |
| 3 | Memory | Q2 2026 | ✅ Complete | Frame allocator, bump heap |
| 4 | Interrupts | Q2-Q3 2026 | 🔄 In Progress | GDT, IDT, TSS, PIC, timer |
| 5 | Execution | Q3-Q4 2026 | 🚧 Pending | Syscalls, exec, user mode |
| 6 | Terminal | Q4 2026-Q1 2027 | 🚧 Pending | VFS, initramfs, shell |
| 7 | Desktop | 2027 | 🚧 Pending | Wayland, compositor |

## Focus Areas by Quarter

### Q2 2026 (Current)
- [x] Phase 4: Interrupts & timers
- [x] Phase 5: Cooperative multitasking
- [ ] Phase 5: Syscall layer
- [ ] CI improvement with tests

### Q3 2026
- [ ] Phase 5: User/kernel ABI
- [ ] Phase 5: ELF loader
- [ ] Basic shell experiment

### Q4 2026
- [ ] Phase 6: Terminal usability
- [ ] Basic file I/O

### 2027
- [ ] Phase 7: Display stack
- [ ] Wayland compositor

## Contributing Timeline

Want to help? Here's when different skill levels can contribute:

| Time | Beginner | Intermediate | Advanced |
|------|----------|-------------|-----------|
| Now | Docs, tests | Memory alloc |Interrupt handlers |
| Q3 | Shell dev | Syscall API | User/kernel ABI |
| Q4+ | App sandbox | VFS | Wayland |

## Release Cadence

- **Alpha releases**: Every phase completion
- **Beta releases**: After Phase 5 (syscalls)
- **Stable**: After Phase 7 (desktop)

See [docs/roadmap.md](roadmap.md) for detailed phase specs.