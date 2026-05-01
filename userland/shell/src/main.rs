#![no_std]
#![no_main]

use libnewos::{print, ls, open, read, close, exit};

#[cfg(not(test))]
use core::panic::PanicInfo;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    print("NewOS User Shell\n");
    print("NewOS> ");

    // 1. Demonstrate 'ls'
    let mut ls_buf = [0u8; 512];
    if let Some(len) = ls(&mut ls_buf) {
        print("Files:\n");
        let ls_str = core::str::from_utf8(&ls_buf[..len as usize]).unwrap_or("");
        print(ls_str);
    }

    // 2. Demonstrate 'cat initramfs.txt'
    if let Some(fd) = open("initramfs.txt") {
        let mut file_buf = [0u8; 128];
        if let Some(len) = read(fd, &mut file_buf) {
            print("\nContent of initramfs.txt:\n");
            let file_str = core::str::from_utf8(&file_buf[..len as usize]).unwrap_or("");
            print(file_str);
        }
        close(fd);
    }

    print("\nShell session complete. Exiting.\n");
    exit(0);
}

#[cfg(test)]
fn main() {}

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    exit(1);
}
