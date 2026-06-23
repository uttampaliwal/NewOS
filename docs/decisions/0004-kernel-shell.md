# ADR 0004: In-Kernel REPL Shell (Superseded)

## Status

**Superseded** by userland shell implementation.

## Context

An in-kernel REPL shell was originally proposed to provide a minimal debugging and
development interface directly within the kernel. This would have allowed early
system bringup and interactive debugging without requiring a full userland.

## Decision

This ADR is preserved for historical reference only. The in-kernel REPL shell
approach was later abandoned in favor of a userland shell implementation, which
provides better isolation, security, and compatibility with standard Unix shell semantics.
