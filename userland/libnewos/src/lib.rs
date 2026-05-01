#![no_std]

use newos_abi::syscall::Syscall;

pub fn print(message: &str) {
    syscall2(Syscall::Write as u64, message.as_ptr() as u64, message.len() as u64);
}

pub fn read(fd: u64, buf: &mut [u8]) -> Option<u64> {
    let res = syscall3(Syscall::Read as u64, fd, buf.as_mut_ptr() as u64, buf.len() as u64);
    if (res as i64) < 0 { None } else { Some(res) }
}

pub fn open(path: &str) -> Option<u64> {
    let res = syscall2(Syscall::Open as u64, path.as_ptr() as u64, path.len() as u64);
    if (res as i64) < 0 { None } else { Some(res) }
}

pub fn close(fd: u64) {
    syscall1(Syscall::Close as u64, fd);
}

pub fn ls(buf: &mut [u8]) -> Option<u64> {
    let res = syscall2(Syscall::Ls as u64, buf.as_mut_ptr() as u64, buf.len() as u64);
    if (res as i64) < 0 { None } else { Some(res) }
}

pub use newos_abi::syscall::Stat;

pub fn stat(path: &str) -> Option<Stat> {
    let mut st = Stat { size: 0, file_type: 0 };
    let res = syscall3(Syscall::Stat as u64, path.as_ptr() as u64, path.len() as u64, &mut st as *mut Stat as u64);
    if (res as i64) < 0 { None } else { Some(st) }
}

pub fn exec(elf_data: &[u8]) -> ! {
    syscall2(Syscall::Exec as u64, elf_data.as_ptr() as u64, elf_data.len() as u64);
    loop {} // Should never reach here
}

pub fn exit(code: i32) -> ! {
    syscall1(Syscall::Exit as u64, code as u64);
    loop {}
}

pub fn fork() -> u64 {
    syscall0(Syscall::Fork as u64)
}

fn syscall0(num: u64) -> u64 {
    let res: u64;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") num,
            out("rcx") _,
            out("r11") _,
            lateout("rax") res,
        );
    }
    res
}

fn syscall1(num: u64, arg0: u64) -> u64 {
    let res: u64;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") num,
            in("rdi") arg0,
            out("rcx") _,
            out("r11") _,
            lateout("rax") res,
        );
    }
    res
}

fn syscall2(num: u64, arg0: u64, arg1: u64) -> u64 {
    let res: u64;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") num,
            in("rdi") arg0,
            in("rsi") arg1,
            out("rcx") _,
            out("r11") _,
            lateout("rax") res,
        );
    }
    res
}

fn syscall3(num: u64, arg0: u64, arg1: u64, arg2: u64) -> u64 {
    let res: u64;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") num,
            in("rdi") arg0,
            in("rsi") arg1,
            in("rdx") arg2,
            out("rcx") _,
            out("r11") _,
            lateout("rax") res,
        );
    }
    res
}
