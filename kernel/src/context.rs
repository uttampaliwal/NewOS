use x86_64::VirtAddr;

pub const USER_CODE_SEGMENT: u64 = 0x23;
pub const USER_DATA_SEGMENT: u64 = 0x2b;
pub const KERNEL_CODE_SEGMENT: u64 = 0x08;
pub const KERNEL_DATA_SEGMENT: u64 = 0x10;

#[derive(Debug, Clone, Copy)]
pub struct UserContext {
    pub entry: u64,
    pub stackPointer: u64,
    pub pageTables: u64,
}

impl UserContext {
    pub fn new(entry: u64, stackPointer: u64) -> Self {
        Self {
            entry,
            stackPointer,
            pageTables: 0,
        }
    }

    pub fn switch_to_user(&self) -> ! {
        unsafe {
            core::arch::asm!(
                "mov rsp, {0}",
                "push {1}",
                "push rsp",
                "pushf",
                "push {2}",
                "push rdi",
                "iretq",
                in(reg) self.stackPointer,
                in(reg) 0,
                in(reg) self.entry,
            );
        }
        core::hint::unreachable_unchecked()
    }
}
