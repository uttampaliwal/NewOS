# ADR 0008: UEFI-First Bring-Up

## Status

Accepted

## Decision

The first boot milestone will use a thin UEFI loader built for `x86_64-unknown-uefi` instead of jumping directly to a freestanding kernel image with a separate bootloader.

## Why

- It fits the current Windows 11 plus QEMU plus EDK2 development environment very well.
- It reduces early setup complexity and shortens the first feedback loop.
- It still teaches modern boot concepts without forcing us to solve every freestanding kernel concern at once.
- It keeps the door open for a later freestanding kernel handoff using the already-installed `x86_64-unknown-none` target.

## Consequences

- Phase 1 is a firmware-facing bring-up milestone rather than a full kernel handoff milestone.
- We still need a later transition from the loader to a freestanding kernel runtime boundary.
- The early serial logging and panic path remain directly useful in later phases.

