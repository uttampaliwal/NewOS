#![no_std]
#![feature(abi_x86_interrupt)]

//! Kernel skeleton for NewOS.
//!
//! This crate is intentionally tiny in Phase 0. The next milestone will add
//! bootloader integration, low-level entry, logging, and hardware bring-up.

extern crate alloc;

pub mod boot;
pub mod drivers;
pub mod elf;
pub mod gdt;
pub mod input;
pub mod interrupts;
pub mod memory;
pub mod process;
pub mod serial;
pub mod shell;
pub mod syscall;
pub mod task;
pub mod vfs;

use newos_abi::version::{ABI_VERSION, PROJECT_NAME};

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
