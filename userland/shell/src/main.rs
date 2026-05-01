#![no_std]
#![no_main]

use libnewos::{close, exit, getpid, ls, open, print, read, uptime};

#[cfg(not(test))]
use core::panic::PanicInfo;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    print("NewOS User Shell\n");

    // Show system status
    let pid = getpid();
    print("PID: ");
    print_u64(pid);
    print("\n");

    let ticks = uptime();
    print("Uptime ticks: ");
    print_u64(ticks);
    print("\n");

    // List files
    let mut ls_buf = [0u8; 512];
    if let Some(len) = ls(&mut ls_buf) {
        print("\nFiles:\n");
        let ls_str = core::str::from_utf8(&ls_buf[..len as usize]).unwrap_or("");
        print(ls_str);
    }

    print("\nEntering interactive mode...\n");
    print("Type 'help' for available commands.\n");

    let kbd_fd = open("keyboard").expect("failed to open keyboard device");
    let mut line_buf = [0u8; 128];
    let mut cursor = 0;

    print("> ");

    loop {
        let mut char_buf = [0u8; 1];
        if let Some(read_len) = read(kbd_fd, &mut char_buf) {
            if read_len > 0 {
                let c = char_buf[0];

                if c == b'\n' || c == b'\r' {
                    print("\n");
                    if cursor > 0 {
                        let cmd = core::str::from_utf8(&line_buf[..cursor]).unwrap_or("");
                        handle_command(cmd);
                    }
                    cursor = 0;
                    print("> ");
                } else if c == 8 || c == 127 {
                    // Backspace
                    if cursor > 0 {
                        cursor -= 1;
                        print("\x08 \x08"); // Backspace, space, backspace to clear character
                    }
                } else if cursor < line_buf.len() {
                    line_buf[cursor] = c;
                    cursor += 1;

                    // Echo character
                    let echo = core::str::from_utf8(&char_buf).unwrap_or("");
                    print(echo);
                }
            }
        }

        for _ in 0..1000 {
            core::hint::spin_loop();
        }
    }
}

fn handle_command(cmd: &str) {
    match cmd {
        "help" => print("Available commands: help, hello, fork, uptime, ls, exit\n"),
        "hello" => print("Hello from the NewOS interactive shell!\n"),
        "uptime" => {
            print("Uptime ticks: ");
            print_u64(uptime());
            print("\n");
        }
        "ls" => {
            let mut ls_buf = [0u8; 512];
            if let Some(len) = ls(&mut ls_buf) {
                let ls_str = core::str::from_utf8(&ls_buf[..len as usize]).unwrap_or("");
                print(ls_str);
            }
        }
        "fork" => {
            let pid = libnewos::fork();
            if pid == 0 {
                print("Child: I am born!\n");
                exit(0);
            } else {
                print("Parent: Spawned child with PID ");
                print_u64(pid);
                print("\n");
            }
        }
        "exit" => {
            print("Exiting shell...\n");
            exit(0);
        }
        _ => {
            print("Unknown command: ");
            print(cmd);
            print("\n");
        }
    }
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
        if idx == 0 {
            break;
        }
        idx -= 1;
    }
    if let Ok(s) = core::str::from_utf8(&buf[idx + 1..]) {
        if idx == 19 && buf[19] != 0 {
             // Special case for single digit handled by idx+1 logic
        }
    }
    // Corrected print_u64 logic
    let s = core::str::from_utf8(&buf[idx + 1..]).unwrap_or("?");
    print(s);
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    exit(1);
}

#[cfg(test)]
fn main() {}
