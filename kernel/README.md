# Kernel Notes

The kernel crate is intentionally minimal in Phase 0.

Right now it provides:

- a stable place for kernel code to live
- a tiny `KernelInfo` structure
- shared linkage to the ABI crate

In the next milestone this crate will shift from a simple library skeleton to a bootable freestanding kernel target.

For the immediate Phase 1 bring-up, the UEFI loader calls into this crate for shared kernel identity and early handoff structure.
