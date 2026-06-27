#![no_std]
#![no_main]

extern crate alloc;

use alloc::format;
use alloc::string::String;

use libturnix::allocator::BumpAllocator;

#[global_allocator]
static ALLOCATOR: BumpAllocator = BumpAllocator;

use display_manager::{PASSWD_PATH, PasswdEntry};
use libturnix::{close, exit, fork, open, print, println, read, setgid, setuid, waitpid};
use turnix_abi::syscall::{CapData, CapHeader, LINUX_CAPABILITY_VERSION};

#[allow(dead_code)]
fn greet() {
    println("");
    println("╔══════════════════════════════════╗");
    println("║      Turnix Operating System     ║");
    println("║         Please log in            ║");
    println("╚══════════════════════════════════╝");
}

#[allow(dead_code)]
fn read_line() -> String {
    let mut buf = [0u8; 1];
    let mut s = String::new();
    loop {
        match read(0, &mut buf) {
            Some(1) => {
                let c = buf[0] as char;
                match c {
                    '\n' | '\r' => {
                        return s;
                    }
                    '\x7f' | '\x08' => {
                        s.pop();
                    }
                    _ if c.is_ascii_graphic() || c == ' ' => {
                        s.push(c);
                    }
                    _ => {}
                }
            }
            _ => return s,
        }
    }
}

#[allow(dead_code)]
fn read_username() -> String {
    print("login: ");
    read_line()
}

#[allow(dead_code)]
fn read_password() -> String {
    print("password: ");
    let pwd = read_line();
    println("");
    pwd
}

#[allow(dead_code)]
fn load_passwd() -> Option<String> {
    let fd = open(PASSWD_PATH)?;
    let mut buf = [0u8; 4096];
    let n = read(fd, &mut buf)?;
    close(fd);
    core::str::from_utf8(&buf[..n as usize])
        .ok()
        .map(String::from)
}

fn launch_session(entry: &PasswdEntry) {
    print("Starting session for user: ");
    println(entry.username);

    let pid = fork();

    if pid == 0 {
        let r = setgid(entry.gid);
        if r < 0 {
            println("setgid failed");
            exit(1);
        }
        let r = setuid(entry.uid);
        if r < 0 {
            println("setuid failed");
            exit(1);
        }
        let header = CapHeader {
            version: LINUX_CAPABILITY_VERSION,
            pid: 0,
        };
        let data = CapData {
            effective: 0,
            permitted: 0,
            inheritable: 0,
        };
        libturnix::capset(&header, &data);

        libturnix::exec("desktop-shell", core::ptr::null(), core::ptr::null());
        #[allow(unreachable_code)]
        {
            println("exec desktop-shell failed");
            exit(1);
        }
    } else {
        println(&format!("Session spawned as PID {}", pid));
        loop {
            let exited_pid = waitpid(-1, core::ptr::null_mut(), 0) as u64;
            if exited_pid == pid {
                break;
            }
        }
        println("Session exited, returning to login prompt");
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    println("Turnix Display Manager v1");

    // Auto-login as root — skip the interactive login prompt.
    // The framebuffer console doesn't render TTY text (compositor covers it),
    // so interactive login is not usable yet. Launch the desktop directly.
    println("Auto-login as root");

    let fake_entry = display_manager::PasswdEntry {
        username: "root",
        uid: 0,
        gid: 0,
        home: "/root",
        shell: "/bin/sh",
        password_hash: "",
    };
    launch_session(&fake_entry);

    // Should never reach here, but if session exits, loop forever.
    loop {
        libturnix::yielder();
    }
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    print("display-manager panic: ");
    exit(1);
}
