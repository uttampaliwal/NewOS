#![no_std]
#![no_main]

extern crate alloc;

use alloc::format;
use alloc::string::String;

use libturnix::allocator::BumpAllocator;

#[global_allocator]
static ALLOCATOR: BumpAllocator = BumpAllocator;

use display_manager::{PASSWD_PATH, PasswdEntry, authenticate};
use libturnix::{close, exit, fork, open, print, println, read, setgid, setuid, waitpid};
use turnix_abi::syscall::{CapData, CapHeader, LINUX_CAPABILITY_VERSION};

fn greet() {
    println("");
    println("╔══════════════════════════════════╗");
    println("║      Turnix Operating System     ║");
    println("║         Please log in            ║");
    println("╚══════════════════════════════════╝");
}

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

fn read_username() -> String {
    print("login: ");
    read_line()
}

fn read_password() -> String {
    print("password: ");
    let pwd = read_line();
    println("");
    pwd
}

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

    let passwd_content = match load_passwd() {
        Some(c) => c,
        None => {
            print("WARNING: no password file found at ");
            println(PASSWD_PATH);
            println("Default credentials: root/root, turnix/turnix");
            println("Create /etc/turnix/passwd with lines: username:uid:gid:home:shell:sha256hex");
            println("Proceeding with emergency fallback authentication");
            println("");
            let root_hash = "4813494d137e1631bba301d5acab6e7bb7aa74ce1185d456565ef51d737677b2";
            let turnix_hash = "35dc5cc5d07a524eb7a7b32cb2f004ba802677843fd39c9f93d26a207e7cf381";
            let fallback = alloc::format!(
                "root:0:0:/root:/bin/sh:{}\nturnix:1000:1000:/home/turnix:/bin/sh:{}\n",
                root_hash,
                turnix_hash
            );
            fallback
        }
    };

    loop {
        greet();
        let username = read_username();
        if username.is_empty() {
            continue;
        }
        let password = read_password();

        match authenticate(&username, &password, &passwd_content) {
            Some(entry) => {
                launch_session(&entry);
            }
            None => {
                println("Login incorrect");
            }
        }
    }
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    print("display-manager panic: ");
    exit(1);
}
