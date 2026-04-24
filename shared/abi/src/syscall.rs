#[cfg(test)]
mod tests {
    #[repr(u16)]
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Syscall {
        Write = 1,
        Exit = 2,
    }

    #[repr(C)]
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct SyscallHeader {
        pub number: u16,
        pub flags: u16,
        pub reserved: u32,
    }

    #[test]
    fn write_syscall_id_is_one() {
        assert_eq!(Syscall::Write as u16, 1);
    }

    #[test]
    fn exit_syscall_id_is_two() {
        assert_eq!(Syscall::Exit as u16, 2);
    }

    #[test]
    fn syscall_header_size_is_valid() {
        use core::mem::size_of;
        assert_eq!(size_of::<SyscallHeader>(), 8);
    }
}
