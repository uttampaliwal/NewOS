#![no_std]
#![no_main]

use libturnix::{exit, getpid, ls, open, read, uptime, write};

#[cfg(not(test))]
use core::panic::PanicInfo;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let tty_fd = open("tty").expect("failed to open /dev/tty");
    
    // Helper to print to tty
    let tty_print = |s: &str| {
        write(tty_fd, s.as_bytes());
    };

        tty_print("Turnix Interactive Shell (Phase 6)\n");
    tty_print("System PID: ");
    print_u64(getpid(), &tty_print);
    tty_print("\nType 'help' for commands.\n\n");

    let mut line_buf = [0u8; 256];

    loop {
        tty_print("> ");
        if let Some(len) = read(tty_fd, &mut line_buf) {
            if len > 0 {
                let line = core::str::from_utf8(&line_buf[..len as usize]).unwrap_or("").trim();
                if !line.is_empty() {
                    handle_command(line, tty_fd, &tty_print);
                }
            }
        }
    }
}

fn handle_command(cmd: &str, _tty_fd: u64, tty_print: &impl Fn(&str)) {
    match cmd {
        "help" => tty_print("Available: help, hello, ls, uptime, exit\n"),
        "hello" => tty_print("Hello from the clean Turnix shell!\n"),
        "ls" => {
            let mut buf = [0u8; 1024];
            if let Some(len) = ls(&mut buf) {
                let s = core::str::from_utf8(&buf[..len as usize]).unwrap_or("");
                tty_print(s);
            }
        }
        "uptime" => {
            tty_print("Uptime ticks: ");
            print_u64(uptime(), tty_print);
            tty_print("\n");
        }
        "exit" => {
            tty_print("Goodbye!\n");
            exit(0);
        }
        _ => {
            tty_print("Unknown command: ");
            tty_print(cmd);
            tty_print("\n");
        }
    }
}

fn print_u64(mut n: u64, tty_print: &impl Fn(&str)) {
    if n == 0 {
        tty_print("0");
        return;
    }
    let mut buf = [0u8; 20];
    let mut idx = 20;
    while n > 0 {
        idx -= 1;
        buf[idx] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    if let Ok(s) = core::str::from_utf8(&buf[idx..]) {
        tty_print(s);
    }
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    exit(1);
}

#[cfg(test)]
fn main() {}
