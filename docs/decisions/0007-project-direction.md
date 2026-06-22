# ADR 0001: Project Direction

## Status

Accepted

## Decision

turnix will be developed as a Rust-first, modular monolithic operating system targeting x86_64 on QEMU first. The product goal is a secure, modern, Linux-like general-purpose operating system with a terminal-first early release and a Wayland-oriented desktop later.

## Why

- This provides the best learning path for a beginner while still aiming at a serious end state.
- Rust helps reduce avoidable memory-safety bugs in new kernel code.
- QEMU-first development is faster and safer than immediate bare-metal work.
- Linux-like behavior matters for usability and source portability.
- Terminal-first scope keeps early milestones realistic.

## Consequences

- Early work favors emulator-friendly devices and workflows.
- Compatibility is an important goal, but not at the cost of copying Linux internals.
- Documentation is part of the product, not an afterthought.

