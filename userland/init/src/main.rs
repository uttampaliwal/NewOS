#![no_std]
#![no_main]

use core::panic::PanicInfo;
use libnewos::{print, exit};

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    print("Hello from User Mode init process (using libnewos)!\n");
    print("This demonstrates a stable SOTA syscall interface.\n");

    exit(0);
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    exit(1);
}
