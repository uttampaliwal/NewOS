# Running in QEMU

## Quick Start

```bash
# Boot the OS
cargo xtask run

# Boot without display window
TURNIX_QEMU_DISPLAY=none cargo xtask run
```

## Test Mode

```bash
# Run boot smoke tests (30-attempt boot verification)
cargo xtask test-qemu

# Full test suite
TURNIX_QEMU_DISPLAY=none cargo xtask test-qemu
```

## QEMU Configuration

The xtask runner configures QEMU with:
- 256 MB RAM
- VirtIO-Net for networking
- NVMe block device (if available)
- Serial console (COM1)
- OVMF UEFI firmware

## Debugging

```bash
# Enable GDB stub
cargo xtask run -- --gdb

# Verbose serial output
TURNIX_QEMU_DISPLAY=none cargo xtask run
```
