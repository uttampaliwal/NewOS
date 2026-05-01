#![no_main]
#![no_std]

use core::arch::{asm, global_asm};
use core::panic::PanicInfo;

use turnix_abi::boot::{BootInfo, BootOutcome};

global_asm!(
    r#"
    .section .text._start,"ax"
    .global _start
_start:
    lea rsp, [rip + boot_stack_top]
    xor rbp, rbp
    call kernel_image_main
1:
    hlt
    jmp 1b

    .section .bss.stack,"aw",@nobits
    .align 16
boot_stack:
    .skip 65536
boot_stack_top:
"#
);

const QEMU_DEBUG_EXIT_PORT: u16 = 0xF4;

#[unsafe(no_mangle)]
extern "sysv64" fn kernel_image_main(boot_info: *const BootInfo) -> ! {
    let boot_info = unsafe { &*boot_info };

    match turnix_kernel::boot::early_boot(boot_info) {
        BootOutcome::ExitSuccess => qemu_exit_success(),
        BootOutcome::ExitFailure => qemu_exit_failure(),
    }
}

#[panic_handler]
fn panic(info: &PanicInfo<'_>) -> ! {
    turnix_kernel::serial::init();
    turnix_kernel::serial::print(format_args!("panic: {}\n", info));
    qemu_exit_failure();
}

fn qemu_exit_success() -> ! {
    qemu_exit(0x10);
}

fn qemu_exit_failure() -> ! {
    qemu_exit(0x11);
}

fn qemu_exit(code: u32) -> ! {
    unsafe {
        asm!("out dx, eax", in("dx") QEMU_DEBUG_EXIT_PORT, in("eax") code, options(nomem, nostack, preserves_flags));
    }

    loop {
        unsafe {
            asm!("hlt", options(nomem, nostack, preserves_flags));
        }
    }
}
