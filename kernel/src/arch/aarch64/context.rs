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
        loop {
            core::hint::spin_loop();
        }
    }
}
