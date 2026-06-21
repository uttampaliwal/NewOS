#![no_std]
#![no_main]

#[cfg(not(test))]
use core::panic::PanicInfo;

use libturnix::allocator::BumpAllocator;
use libturnix::{exit, print, println};

#[global_allocator]
static ALLOCATOR: BumpAllocator = BumpAllocator;

use compositor::state::TurnixCompositor;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    println("Turnix Compositor v1");

    let mut compositor = match TurnixCompositor::new() {
        Some(c) => c,
        None => {
            println("ERROR: Failed to initialise compositor (no display?)");
            exit(1);
        }
    };

    println("Compositor running — entering main loop");
    compositor.run();
    exit(0);
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    print("compositor panic: ");
    exit(1);
}
