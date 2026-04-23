# Kernel Notes

The kernel crate is intentionally minimal in Phase 0.

Right now it provides:

- a stable place for kernel code to live
- a tiny `KernelInfo` structure
- shared linkage to the ABI crate

In the next milestone this crate will shift from a simple library skeleton to a bootable freestanding kernel target.
