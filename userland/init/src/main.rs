#![no_std]
#![no_main]

use core::panic::PanicInfo;
use newos_abi::syscall::Syscall;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let message = "Hello from User Mode init process!\n";
    
    // Syscall: Write
    syscall2(Syscall::Write as u64, message.as_ptr() as u64, message.len() as u64);

    // Syscall: Exit
    syscall1(Syscall::Exit as u64, 0);

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

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}
