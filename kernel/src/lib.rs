#![no_std]
#![cfg_attr(feature = "arch-x86_64", feature(abi_x86_interrupt))]

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
    use std::sync::Mutex;

    static LOCKED: AtomicBool = AtomicBool::new(false);

    pub struct Guard;

    impl Drop for Guard {
        fn drop(&mut self) {
            LOCKED.store(false, Ordering::SeqCst);
        }
    }

    pub fn acquire() -> Guard {
        while LOCKED.swap(true, Ordering::SeqCst) {
            std::hint::spin_loop();
        }
        Guard
    }
}

pub mod arch;

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
#[cfg(feature = "arch-x86_64")]
pub mod memory;
#[cfg(feature = "arch-x86_64")]
pub mod process;
#[cfg(feature = "arch-x86_64")]
pub mod security;
pub mod serial;
#[cfg(feature = "arch-x86_64")]
pub mod smp;
#[cfg(feature = "arch-x86_64")]
pub mod syscall;
#[cfg(feature = "arch-x86_64")]
pub mod task;
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
