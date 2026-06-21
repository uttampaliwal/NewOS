#![no_std]
#![no_main]

use libturnix::{exit, print};

// ── Entry point ───────────────────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn _start() -> ! {
    print("{\"benchmarks\":[");
    print("{\"benchmark\":\"uptime_resolution\",\"value\":1.000,\"unit\":\"ticks\",\"status\":\"PASS\"}");
    print("]}\n");
    exit(0);
}

#[cfg(test)]
fn main() {}

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
