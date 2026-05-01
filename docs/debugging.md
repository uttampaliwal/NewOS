# Debugging Guide

Developing a kernel is challenging because you don't have access to standard debuggers or `printf` during early bring-up. This guide outlines the tools and techniques used in `turnix`.

## 1. Serial Output (The "Printf" of Kernels)

We use the serial port (COM1) as our primary diagnostic tool.
- **Usage**: Use `crate::serial::print!` and `println!` macros.
- **Viewing**: When running via `cargo xtask run-uefi`, the serial output is redirected to your terminal.
- **Stage Tracking**: We use `[STG: ...]` tags to mark successful completion of boot phases.

## 2. QEMU and GDB

QEMU has a built-in GDB stub that allows you to debug the kernel as if it were a normal program.

1.  **Start QEMU with GDB stub**:
    ```powershell
    # Manually run QEMU with -s -S flags
    qemu-system-x86_64 -drive format=raw,file=fat:rw:out/esp -serial stdio -s -S
    ```
2.  **Attach GDB**:
    ```bash
    gdb target/x86_64-unknown-none/debug/newos-kernel-image
    (gdb) target remote :1234
    (gdb) continue
    ```

## 3. Panic Handling

When the kernel panics, it stops execution and prints the panic location and message to the serial port.
- **Freestanding Panic**: See `kernel/src/lib.rs`. It attempts to print to serial before halting the CPU with `hlt`.

## 4. Common Boot Issues

| Symptom | Likely Cause | Fix |
|---------|--------------|-----|
| Immediate QEMU crash | Invalid GDT or IDT setup | Check `gdt.rs` and `interrupts/mod.rs` for entry sizes. |
| Triple Fault | Exception during exception handling | Ensure the Double Fault handler is correctly mapped in the GDT. |
| Hanging at `PAGING_INIT` | Invalid physical memory offset | Verify `PHYS_MEM_OFFSET` matches the UEFI boot info. |
| Page Fault in User Mode | Accessing kernel memory | Check `USER_ACCESSIBLE` bit in page table entries. |

## 5. ISA Debug Exit

We use the `isa-debug-exit` device in QEMU to allow the kernel to shut down the VM. This is used for automated testing.
- **Success Code**: `33` (exits with code 1 in shell)
- **Failure Code**: Any other value.
