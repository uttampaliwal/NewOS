<<<<<<< HEAD
# turnix Debugging Guide

This guide covers common techniques for debugging the turnix kernel and user-mode processes.
=======
# NewOS Debugging Guide

This guide covers common techniques for debugging the NewOS kernel and user-mode processes.
>>>>>>> unstable

## 1. QEMU Logging
We use the `isa-debug-exit` device and serial output for basic logging.
- **Serial Output**: The kernel writes to COM1 (`0x3f8`). `xtask` redirects this to your terminal.
- **Port 0xe9**: Standard QEMU "hack" for debug output.
- **Exit Codes**: Code `33` indicates a successful kernel exit path.

## 2. GDB Debugging
To debug with GDB:
1. Start QEMU in "wait for debugger" mode:
   ```bash
<<<<<<< HEAD
   turnix_QEMU_ARGS="-s -S" cargo xtask run-uefi
   ```
2. In another terminal, connect with GDB:
   ```bash
   gdb target/x86_64-unknown-none/debug/turnix-kernel
=======
   NEWOS_QEMU_ARGS="-s -S" cargo xtask run-uefi
   ```
2. In another terminal, connect with GDB:
   ```bash
   gdb target/x86_64-unknown-none/debug/newos-kernel
>>>>>>> unstable
   (gdb) target remote :1234
   ```

## 3. Interpreting Faults
When a Page Fault or Double Fault occurs, the kernel prints an `InterruptStackFrame`.
- **IP (Instruction Pointer)**: Use `nm` or `objdump` on the kernel ELF to find the failing function.
  ```bash
<<<<<<< HEAD
  nm kernel/target/x86_64-unknown-none/debug/turnix-kernel | sort
=======
  nm kernel/target/x86_64-unknown-none/debug/newos-kernel | sort
>>>>>>> unstable
  ```
- **Accessed Address**: For Page Faults, this is the address that caused the fault (from `CR2`).

## 4. Common Issues
- **Double Fault**: Usually caused by a kernel stack overflow or a fault inside a fault handler. Check `TSS` and IST configuration.
- **Page Fault (0x0)**: Read from non-present page.
- **Page Fault (0x2)**: Write to read-only page (check `CR0.WP` bit).
- **Page Fault (0x4)**: User-mode tried to access supervisor-only page.
