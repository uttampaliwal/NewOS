use core::arch::global_asm;
use newos_abi::syscall::{Syscall, SyscallArgs};
use x86_64::VirtAddr;
use x86_64::registers::model_specific::{LStar, Msr, SFMask};

pub mod handler;

#[repr(C, align(16))]
pub struct SyscallFrame {
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rbp: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rbx: u64,
    pub rax: u64,
    // IRETQ frame
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

pub fn init() {
    unsafe {
        let mut star = Msr::new(0xC0000081);
        let kernel_base = 0x08u64;
        let user_base = 0x1Bu64; // GDT Index 3 (User Code 32)
        star.write((user_base << 48) | (kernel_base << 32));

        LStar::write(VirtAddr::new(syscall_entry as *const () as u64));
        SFMask::write(x86_64::registers::rflags::RFlags::INTERRUPT_FLAG);

        let mut efer = Msr::new(0xC0000080);
        efer.write(efer.read() | 1);
    }
}

global_asm!(
    r#"
    .global syscall_entry
    syscall_entry:
        // 1. Swap to kernel GS base to access PerCpu
        swapgs
        
        // 2. Save user RSP and switch to kernel stack
        // We use gs:[0] which is PER_CPU.kernel_stack_ptr
        mov qword ptr gs:[8], rsp // Save User RSP in a temporary slot in PerCpu (need to add it)
        mov rsp, qword ptr gs:[0]

        // 3. Build IRETQ Frame (SS, RSP, RFLAGS, CS, RIP)
        push 0x23 // User SS
        push qword ptr gs:[8]  // User RSP
        push r11  // User RFLAGS
        push 0x2b // User CS
        push rcx  // User RIP

        // 4. Save all registers (matches SyscallFrame)
        push rax 
        push rbx
        push rcx // CPU saved RIP
        push rdx
        push qword ptr gs:[8] // User RSP
        push rsi
        push rdi
        push r8
        push r9
        push r10
        push r11 // CPU saved RFLAGS
        push r12
        push r13
        push r14
        push r15

        // 5. Dispatch
        mov rdi, rsp
        call syscall_dispatch
        
        // 6. Save result to stack rax position
        mov [rsp + 14*8], rax

        // 7. Restore all registers
        pop r15
        pop r14
        pop r13
        pop r12
        pop r11
        pop r10
        pop r9
        pop r8
        pop rdi
        pop rsi
        pop rbp
        pop rdx
        pop rcx
        pop rbx
        pop rax

        // 8. Restore User GS and return
        swapgs
        iretq
    "#
);

unsafe extern "C" {
    fn syscall_entry();
}

#[unsafe(no_mangle)]
pub extern "C" fn get_current_kernel_stack_top() -> usize {
    crate::task::scheduler::get_current_kernel_stack_top()
}

#[unsafe(no_mangle)]
pub extern "C" fn syscall_dispatch(frame: &mut SyscallFrame) -> u64 {
    let syscall_num = frame.rax as u16;
    let args = SyscallArgs {
        arg0: frame.rdi,
        arg1: frame.rsi,
        arg2: frame.rdx,
        arg3: frame.r10,
    };

    if let Some(syscall) = Syscall::from_u16(syscall_num) {
        let result = handler::handle_syscall(syscall, args);
        match result {
            handler::SyscallResult::Success(val) => val,
            handler::SyscallResult::Error(err) => err as u64,
        }
    } else {
        0xFFFFFFFFFFFFFFFFu64
    }
}
