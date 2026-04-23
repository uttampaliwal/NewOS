#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Syscall {
    Write = 1,
    Exit = 2,
}

impl Syscall {
    pub const fn from_u16(value: u16) -> Option<Self> {
        match value {
            1 => Some(Self::Write),
            2 => Some(Self::Exit),
            _ => None,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyscallHeader {
    pub number: u16,
    pub flags: u16,
    pub reserved: u32,
}

