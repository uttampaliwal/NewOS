# turnix Demo Gallery

This directory contains demo screenshots and boot logs for turnix.

## Adding Demos

To capture a boot log:

```powershell
# Run QEMU and capture serial output
cargo xtask run-uefi 2>&1 | Out-File boot-log.txt

# Or use QEMU monitor for additional debugging
qemu-system-x86_64 ... -serial file:serial.log
```

## Current Demos

| File | Description |
|------|-------------|
| `boot-demo.txt` | Text boot log from QEMU |
| `memory-demo.txt` | Memory allocator output |

## Boot Demo

```
+-----------------------------------------------------------+
|                        turnix v0.0.7                        |
+-----------------------------------------------------------+
|  UEFI turnix Loader                                        |
|  ------------------------                                 |
|  Memory Map:                                              |
|    - Conventional: 513792 KB available                  |
|    - Reserved: 184 KB                                    |
|  Loading kernel from EFI file system... OK                 |
|  Jumping to kernel at 0x100000...                         |
+-----------------------------------------------------------+
|  turnix Kernel                                             |
|  --------------------                                      |
|  Memory: 513792 KB conventional                          |
|  Physical frames: 127948 (0 - 128 MB)                     |
|  Heap: bump allocator initialized at 0x200000                   |
|  GDT: code=0x08, data=0x10, user=0x18                       |
|  IDT: 256 entries                                         |
|  PIC: IRQ0-IRQ15 remapped                                  |
|  Timer: 100 Hz configured                                 |
|  Kernel ready!                                            |
+-----------------------------------------------------------+
```

## Interrupts Demo

```
+-----------------------------------------------------------+
|  Timer tick #1 @ 10ms                                     |
|  Timer tick #2 @ 20ms                                     |
|  Timer tick #3 @ 30ms                                     |
|  ...                                                     |
+-----------------------------------------------------------+
```

## Creating GIFs

To create animated GIFs of the boot process:

1. Use QEMU with screen recording
2. Or capture serial output and render as animation

Example with FFmpeg:

```bash
ffmpeg -f lavfi -i color=c=black:s=640x480:d=5 -vf "drawtext=fontfile=mono.ttf:text='turnix Boot':fontsize=24:fontcolor=white:x=(w-text_w)/2:y=(h-text_h)/2" boot-frame.gif
```
