pub const USER_CODE_SEGMENT: u64 = 0x23;
pub const USER_DATA_SEGMENT: u64 = 0x2b;
pub const KERNEL_CODE_SEGMENT: u64 = 0x08;
pub const KERNEL_DATA_SEGMENT: u64 = 0x10;

#[derive(Debug, Clone, Copy)]
pub struct UserContext {
    pub entry: u64,
    pub stack_pointer: u64,
    pub page_tables: u64,
}

impl UserContext {
    pub fn new(entry: u64, stack_pointer: u64) -> Self {
        Self {
            entry,
            stack_pointer,
            page_tables: 0,
        }
    }

    pub fn switch_to_user(&self) -> ! {
        unsafe {
            core::arch::asm!(
                "mov rsp, {0:r}",
                "push {1:r}",
                "push rsp",
                "pushf",
                "push {2:r}",
                "push rdi",
                "iretq",
                in(reg) self.stack_pointer,
                in(reg) 0,
                in(reg) self.entry,
            );
            core::hint::unreachable_unchecked()
        }
    }
}
