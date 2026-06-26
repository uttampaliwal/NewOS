#![no_std]
#![no_main]

use libturnix::{args_after_prog_count, arg_after_prog, exit, kill, write_str};

#[cfg(not(test))]
use core::panic::PanicInfo;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let count = args_after_prog_count();
    if count == 0 {
        write_str(2, "kill: usage: kill [-s SIG] PID\n");
        exit(1);
    }

    let mut signal: u8 = 15;
    let mut pid_arg: Option<&str> = None;
    let mut i = 0;

    while i < count {
        let arg = arg_after_prog(i).unwrap_or("");
        if arg == "-s" {
            if i + 1 >= count {
                write_str(2, "kill: missing signal argument\n");
                exit(1);
            }
            i += 1;
            let sig_str = arg_after_prog(i).unwrap_or("");
            match parse_signal(sig_str) {
                Some(s) => signal = s,
                None => {
                    write_str(2, "kill: invalid signal: ");
                    write_str(2, sig_str);
                    write_str(2, "\n");
                    exit(1);
                }
            }
        } else {
            pid_arg = Some(arg);
        }
        i += 1;
    }

    let pid_str = match pid_arg {
        Some(s) => s,
        None => {
            write_str(2, "kill: usage: kill [-s SIG] PID\n");
            exit(1);
        }
    };

    let pid = match parse_pid(pid_str) {
        Some(p) => p,
        None => {
            write_str(2, "kill: invalid PID: ");
            write_str(2, pid_str);
            write_str(2, "\n");
            exit(1);
        }
    };

    if kill(pid, signal) != 0 {
        write_str(2, "kill: failed to send signal\n");
        exit(1);
    }
    exit(0);
}

fn parse_pid(s: &str) -> Option<i32> {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    let mut n: i32 = 0;
    for &b in bytes {
        if b < b'0' || b > b'9' {
            return None;
        }
        n = n.checked_mul(10)?.checked_add((b - b'0') as i32)?;
    }
    Some(n)
}

fn parse_signal(s: &str) -> Option<u8> {
    match s {
        "1" | "HUP" => Some(1),
        "2" | "INT" => Some(2),
        "3" | "QUIT" => Some(3),
        "6" | "ABRT" => Some(6),
        "9" | "KILL" => Some(9),
        "14" | "ALRM" => Some(14),
        "15" | "TERM" => Some(15),
        _ => {
            let bytes = s.as_bytes();
            if bytes.is_empty() {
                return None;
            }
            let mut n: u8 = 0;
            for &b in bytes {
                if b < b'0' || b > b'9' {
                    return None;
                }
                n = n.checked_mul(10)?.checked_add(b - b'0')?;
            }
            Some(n)
        }
    }
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    exit(1);
}
