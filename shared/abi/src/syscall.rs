pub const FILE_TYPE_REGULAR: u32 = 0;
pub const FILE_TYPE_DIRECTORY: u32 = 1;
pub const FILE_TYPE_DEVICE: u32 = 2;
pub const FILE_TYPE_PIPE: u32 = 3;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stat {
    pub size: u64,
    pub file_type: u32,
}

#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Syscall {
    Write = 1,
    Exit = 2,
    Read = 3,
    Open = 4,
    Close = 5,
    Exec = 6,
    Fork = 7,
    Wait = 8,
    Yielder = 9,
    Uptime = 10,
    Ls = 11,
    Stat = 12,
    GetPid = 13,
    Seek = 14,
    WriteFile = 15,
    GetUid = 16,
    GetGid = 17,
    Brk = 18,
    Mkdir = 19,
    Unlink = 20,
    MmapFramebuffer = 21,
    Mmap = 22,
    Munmap = 23,
    Mount = 24,
    Umount = 25,
    Waitpid = 26,
    Pipe = 27,
}

impl Syscall {
    pub const fn from_u16(value: u16) -> Option<Self> {
        match value {
            1 => Some(Self::Write),
            2 => Some(Self::Exit),
            3 => Some(Self::Read),
            4 => Some(Self::Open),
            5 => Some(Self::Close),
            6 => Some(Self::Exec),
            7 => Some(Self::Fork),
            8 => Some(Self::Wait),
            9 => Some(Self::Yielder),
            10 => Some(Self::Uptime),
            11 => Some(Self::Ls),
            12 => Some(Self::Stat),
            13 => Some(Self::GetPid),
            14 => Some(Self::Seek),
            15 => Some(Self::WriteFile),
            16 => Some(Self::GetUid),
            17 => Some(Self::GetGid),
            18 => Some(Self::Brk),
            19 => Some(Self::Mkdir),
            20 => Some(Self::Unlink),
            21 => Some(Self::MmapFramebuffer),
            22 => Some(Self::Mmap),
            23 => Some(Self::Munmap),
            24 => Some(Self::Mount),
            25 => Some(Self::Umount),
            26 => Some(Self::Waitpid),
            27 => Some(Self::Pipe),
            _ => None,
        }
    }

    pub const fn id(&self) -> u16 {
        *self as u16
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyscallHeader {
    pub number: u16,
    pub flags: u16,
    pub reserved: u32,
}

impl SyscallHeader {
    pub const fn new(number: Syscall) -> Self {
        Self {
            number: number as u16,
            flags: 0,
            reserved: 0,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyscallArgs {
    pub arg0: u64,
    pub arg1: u64,
    pub arg2: u64,
    pub arg3: u64,
}

impl SyscallArgs {
    pub const fn new(arg0: u64, arg1: u64, arg2: u64, arg3: u64) -> Self {
        Self {
            arg0,
            arg1,
            arg2,
            arg3,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_syscall_id_is_one() {
        assert_eq!(Syscall::Write as u16, 1);
    }

    #[test]
    fn exit_syscall_id_is_two() {
        assert_eq!(Syscall::Exit as u16, 2);
    }

    #[test]
    fn syscall_round_trip() {
        assert_eq!(Syscall::from_u16(1), Some(Syscall::Write));
        assert_eq!(Syscall::from_u16(2), Some(Syscall::Exit));
        assert_eq!(Syscall::from_u16(3), Some(Syscall::Read));
    }

    #[test]
    fn syscall_header_size() {
        use core::mem::size_of;
        assert_eq!(size_of::<SyscallHeader>(), 8);
    }

    #[test]
    fn syscall_args_size() {
        use core::mem::size_of;
        assert_eq!(size_of::<SyscallArgs>(), 32);
    }

    #[test]
    fn syscall_id_method() {
        assert_eq!(Syscall::Write.id(), 1);
        assert_eq!(Syscall::Exit.id(), 2);
    }

    #[test]
    fn uptime_syscall_id_is_ten() {
        assert_eq!(Syscall::Uptime as u16, 10);
    }

    #[test]
    fn getpid_syscall_id_is_thirteen() {
        assert_eq!(Syscall::GetPid as u16, 13);
    }

    #[test]
    fn seek_syscall_id_is_fourteen() {
        assert_eq!(Syscall::Seek as u16, 14);
    }

    #[test]
    fn writefile_syscall_id_is_fifteen() {
        assert_eq!(Syscall::WriteFile as u16, 15);
    }

    #[test]
    fn getuid_syscall_id_is_sixteen() {
        assert_eq!(Syscall::GetUid as u16, 16);
    }

    #[test]
    fn getgid_syscall_id_is_seventeen() {
        assert_eq!(Syscall::GetGid as u16, 17);
    }

    #[test]
    fn brk_syscall_id_is_eightteen() {
        assert_eq!(Syscall::Brk as u16, 18);
    }

    #[test]
    fn mkdir_syscall_id_is_nineteen() {
        assert_eq!(Syscall::Mkdir as u16, 19);
    }

    #[test]
    fn unlink_syscall_id_is_twenty() {
        assert_eq!(Syscall::Unlink as u16, 20);
    }

    #[test]
    fn mmap_framebuffer_syscall_id_is_twenty_one() {
        assert_eq!(Syscall::MmapFramebuffer as u16, 21);
    }

    #[test]
    fn mmap_framebuffer_syscall_round_trip() {
        assert_eq!(Syscall::from_u16(21), Some(Syscall::MmapFramebuffer));
    }

    #[test]
    fn mmap_syscall_id_is_twenty_two() {
        assert_eq!(Syscall::Mmap as u16, 22);
    }

    #[test]
    fn munmap_syscall_id_is_twenty_three() {
        assert_eq!(Syscall::Munmap as u16, 23);
    }

    #[test]
    fn mmap_syscall_round_trip() {
        assert_eq!(Syscall::from_u16(22), Some(Syscall::Mmap));
    }

    #[test]
    fn munmap_syscall_round_trip() {
        assert_eq!(Syscall::from_u16(23), Some(Syscall::Munmap));
    }

    #[test]
    fn mount_syscall_id_is_twenty_four() {
        assert_eq!(Syscall::Mount as u16, 24);
    }

    #[test]
    fn umount_syscall_id_is_twenty_five() {
        assert_eq!(Syscall::Umount as u16, 25);
    }

    #[test]
    fn mount_syscall_round_trip() {
        assert_eq!(Syscall::from_u16(24), Some(Syscall::Mount));
    }

    #[test]
    fn umount_syscall_round_trip() {
        assert_eq!(Syscall::from_u16(25), Some(Syscall::Umount));
    }

    #[test]
    fn waitpid_syscall_id_is_twenty_six() {
        assert_eq!(Syscall::Waitpid as u16, 26);
    }

    #[test]
    fn waitpid_syscall_round_trip() {
        assert_eq!(Syscall::from_u16(26), Some(Syscall::Waitpid));
    }

    #[test]
    fn pipe_syscall_id_is_twenty_seven() {
        assert_eq!(Syscall::Pipe as u16, 27);
    }

    #[test]
    fn pipe_syscall_round_trip() {
        assert_eq!(Syscall::from_u16(27), Some(Syscall::Pipe));
    }
}
