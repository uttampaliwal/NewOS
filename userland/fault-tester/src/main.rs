#![no_std]
#![no_main]

use libturnix::{exit, print};

#[no_mangle]
pub extern "C" fn _start() -> ! {
    print("Fault Tester: Attempting to read from kernel memory (0xFFFF_FFFF_8000_0000)...\n");

    let kernel_ptr = 0xFFFF_FFFF_8000_0000u64 as *const u8;
    let _val = unsafe { *kernel_ptr };

    print("Fault Tester: SUCCESS (This should NOT happen!)\n");
    exit(0);
}

#[cfg(test)]
fn main() {}

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
