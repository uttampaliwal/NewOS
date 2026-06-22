# Device Drivers

Turnix implements a device driver framework under `kernel/src/drivers/` with support for
several standard virtual and physical devices.

## PCI/PCIe Enumeration

The PCI subsystem (`kernel/src/drivers/pci.rs`, `pcie.rs`) performs MMIO walks via PCIe
ECAM mapping to discover and configure attached devices.

## Block & Network

- **NVMe** (`kernel/src/drivers/nvme.rs`): Full block device driver with submission and
  completion queues, PRP-based data transfers, and interrupt handling.
- **VirtIO-Net** (`kernel/src/drivers/virtio_net.rs`): VirtIO network device with feature
  negotiation and MAC address extraction for virtual networking.

## USB

- **XHCI** (`kernel/src/drivers/xhci.rs`): USB host controller initialization, USB 2.0
  and 3.x device enumeration, and a USB keyboard input handler for early boot interaction.

## Graphics

- **DRM/KMS** (`kernel/src/drivers/gpu/`): Kernel Mode Setting support with
  bochs-display framebuffer mapping. Provides `drm_open`, `drm_set_mode`,
  `drm_page_flip`, and other ioctls for the Wayland compositor.

## ACPI & TPM

- **ACPI** (`kernel/src/acpi.rs`): Superblock parsing (RSDP, XSDT, MCFG, DSDT/SSDT) and
  AML interpreter evaluation. Supports S0/S5 power state transitions.
- **TPM** (`kernel/src/drivers/tpm.rs`): Trusted Platform Module driver for platform
  integrity measurements.
