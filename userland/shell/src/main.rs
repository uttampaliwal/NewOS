#![no_std]
#![no_main]

use libnewos::{print, ls, open, read, close, exit, uptime, getpid};

#[cfg(not(test))]
use core::panic::PanicInfo;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    print("NewOS User Shell\n");

    // Show PID
    let pid = getpid();
    print("PID: ");
    print_u64(pid);
    print("\n");

    // Show uptime
    let ticks = uptime();
    print("Uptime ticks: ");
    print_u64(ticks);
    print("\n");

    // List files with 'ls'
    let mut ls_buf = [0u8; 512];
    if let Some(len) = ls(&mut ls_buf) {
        print("\nFiles:\n");
        let ls_str = core::str::from_utf8(&ls_buf[..len as usize]).unwrap_or("");
        print(ls_str);
    }

    // Read and display initramfs.txt
    if let Some(fd) = open("initramfs.txt") {
        let mut file_buf = [0u8; 256];
        if let Some(len) = read(fd, &mut file_buf) {
            print("\n--- initramfs.txt ---\n");
            let file_str = core::str::from_utf8(&file_buf[..len as usize]).unwrap_or("");
            print(file_str);
            print("---\n");
        }
        close(fd);
    }

    print("\nShell session complete. Exiting.\n");
    exit(0);
}

fn print_u64(mut n: u64) {
    if n == 0 {
        print("0");
        return;
    }
    let mut buf = [0u8; 20];
    let mut idx = 19;
    while n > 0 {
        buf[idx] = b'0' + (n % 10) as u8;
        n /= 10;
        if idx == 0 { break; }
        idx -= 1;
    }
    if let Ok(s) = core::str::from_utf8(&buf[idx..]) {
        print(s);
    }
}

#[cfg(test)]
fn main() {}

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    exit(1);
}
