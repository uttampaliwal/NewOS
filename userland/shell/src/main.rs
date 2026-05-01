#![no_std]
#![no_main]

use libnewos::{exit, open, print, read};

#[cfg(not(test))]
use core::panic::PanicInfo;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    print("NewOS Interactive Shell\n");
    print("Type something and press enter...\n");

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

        // Brief spin to prevent 100% CPU usage in a real OS,
        // though our current read is non-blocking and scheduler will switch us anyway.
        for _ in 0..1000 {
            core::hint::spin_loop();
        }
    }
}

fn handle_command(cmd: &str) {
    match cmd {
        "help" => print("Available commands: help, hello, fork, exit\n"),
        "hello" => print("Hello from the NewOS interactive shell!\n"),
        "fork" => {
            let pid = libnewos::fork();
            if pid == 0 {
                print("Child: I am born!\n");
                exit(0);
            } else {
                print("Parent: Spawned child with PID ");
                // Simple number to string for debug
                let mut buf = [0u8; 20];
                let mut n = pid;
                let mut i = 19;
                if n == 0 {
                    print("0");
                } else {
                    while n > 0 {
                        buf[i] = (n % 10) as u8 + b'0';
                        n /= 10;
                        i -= 1;
                    }
                    print(core::str::from_utf8(&buf[i + 1..]).unwrap_or("?"));
                }
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

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    exit(1);
}
