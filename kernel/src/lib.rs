#![no_std]
#![cfg_attr(feature = "arch-x86_64", feature(abi_x86_interrupt))]
// TODO: Switch to #![warn(clippy::undocumented_unsafe_blocks)] once safety
// comments are backfilled. ~197/403 blocks documented (paging, interrupts,
// apic, wx, acpi, virtio_net, nvme, process, swap). Remaining: ~206 blocks.
// Track: KNOWN_ISSUES.md #16
#![allow(clippy::undocumented_unsafe_blocks)]

//! Kernel skeleton for turnix.
//!
//! This crate is intentionally tiny in Phase 0. The next milestone will add
//! bootloader integration, low-level entry, logging, and hardware bring-up.

extern crate alloc;
#[cfg(test)]
extern crate std;

/// Global test serialization lock — all tests mutating shared state
/// (PROCESS_TABLE, scheduler) must acquire this guard.
/// Uses an atomic spin internally so it does not depend on std's
/// thread-parking primitives, which can interact badly with the
/// kernel's no_std-alien test binary.
#[cfg(test)]
pub mod test_serial {
    use core::sync::atomic::{AtomicBool, Ordering};

    static LOCKED: AtomicBool = AtomicBool::new(false);
    static DISABLED_SERIAL: AtomicBool = AtomicBool::new(false);

    pub struct Guard;

    impl Drop for Guard {
        fn drop(&mut self) {
            LOCKED.store(false, Ordering::SeqCst);
        }
    }

    pub fn acquire() -> Guard {
        // Disable hardware serial once on first acquire to prevent
        // SIGSEGV from port I/O in userspace test mode.
        if !DISABLED_SERIAL.swap(true, Ordering::SeqCst) {
            turnix_serial::disable_serial();
        }
        while LOCKED.swap(true, Ordering::SeqCst) {
            std::hint::spin_loop();
        }
        Guard
    }
}

pub mod arch;
pub mod block;
pub mod cgroup;
pub mod crypto;

// Re-export arch-specific modules at their original paths so
// existing `crate::gdt::*` and `crate::interrupts::*` references
// continue to compile without changes.
pub use arch::context;
pub use arch::gdt;
pub use arch::interrupts;

#[cfg(feature = "arch-x86_64")]
pub mod acpi;
#[cfg(feature = "arch-x86_64")]
pub mod boot;
#[cfg(feature = "arch-x86_64")]
pub mod drivers;
pub mod elf;
#[cfg(feature = "arch-x86_64")]
pub mod fs;
#[cfg(feature = "arch-x86_64")]
pub mod input;
#[cfg(feature = "arch-x86_64")]
pub mod ipc;
pub mod log_ring;
#[cfg(feature = "arch-x86_64")]
pub mod memory;
#[cfg(feature = "arch-x86_64")]
pub mod net;
#[cfg(feature = "arch-x86_64")]
pub mod process;
#[cfg(feature = "arch-x86_64")]
pub mod security;
pub mod serial;
#[cfg(feature = "arch-x86_64")]
pub mod smp;
pub mod softirq;
pub mod sync;
#[cfg(feature = "arch-x86_64")]
pub mod syscall;
#[cfg(feature = "arch-x86_64")]
pub mod task;
pub mod tasklet;
#[cfg(feature = "arch-x86_64")]
pub mod time;
#[cfg(feature = "arch-x86_64")]
pub mod tty;
#[cfg(feature = "arch-x86_64")]
pub mod vfs;

use turnix_abi::version::{ABI_VERSION, PROJECT_NAME};

pub struct KernelInfo {
    pub project_name: &'static str,
    pub abi_version: u32,
}

pub const fn kernel_info() -> KernelInfo {
    KernelInfo {
        project_name: PROJECT_NAME,
        abi_version: ABI_VERSION,
    }
}
