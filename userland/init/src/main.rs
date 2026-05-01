#![no_std]
#![no_main]

#[cfg(not(test))]
use core::panic::PanicInfo;
use libturnix::{exit, print};

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    print("turnix System Init (v3)\n");
    print("Searching for shell...\n");

    // In a real OS we'd use exec() here.
    // Since we don't have fork/exec fully ready,
    // we'll let the scheduler handle the task switch if shell is already loaded.
    // For now, init just exits and the scheduler will run the shell task.

    exit(0);
}

#[cfg(test)]
fn main() {}

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    exit(1);
}
