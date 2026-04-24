# Good First Issues

This file documents beginner-friendly issues that are great for new contributors.

## How to Help

1. Check the issue tracker for issues labeled `good first issue`
2. Read [CONTRIBUTING.md](../CONTRIBUTING.md) for setup instructions
3. Comment on the issue you'd like to work on
4. Fork, branch, and submit a PR

---

## Recommended Starter Issues

### 1. Add more shell commands

**Location**: `kernel/src/shell.rs`
**Labels**: `enhancement`, `good first issue`
**Difficulty**: Beginner

Add support for additional commands like:
- `date` - show current date/time (simulated)
- `uptime` - show system uptime
- `echo` - with options like `-n`

### 2. Expand unit tests for shared/abi

**Location**: `shared/abi/`
**Labels**: `testing`, `good first issue`
**Difficulty**: Beginner

Current tests cover boot info and syscall IDs. Add tests for:
- Memory descriptor validation
- BootInfo serialization
- Syscall header defaults

Run tests:
```powershell
cargo test -p newos-abi
```

### 3. Document interrupt handlers

**Location**: `kernel/src/interrupts.rs`
**Labels**: `documentation`, `good first issue`
**Difficulty**: Beginner

Add doc comments explaining each interrupt handler.

### 4. Scheduler statistics

**Location**: `kernel/src/task/scheduler.rs`
**Labels**: `enhancement`, `good first issue`
**Difficulty**: Intermediate

Add serial output showing task count, running task ID.

---

## Getting Help

- Open a discussion in [GitHub Discussions](https://github.com/uttampaliwal/NewOS/discussions)
- Ask in the issue comments
- Review closed PRs for examples