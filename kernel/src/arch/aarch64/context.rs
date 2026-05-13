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
        panic!("AArch64 switch_to_user not yet implemented");
    }
}
