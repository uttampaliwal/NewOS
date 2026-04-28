#![no_std]

use newos_abi::syscall::Syscall;

pub fn print(message: &str) {
    syscall2(Syscall::Write as u64, message.as_ptr() as u64, message.len() as u64);
}

pub fn exit(code: i32) -> ! {
    syscall1(Syscall::Exit as u64, code as u64);
    loop {}
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
