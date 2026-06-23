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
    Socket = 28,
    Bind = 29,
    Listen = 30,
    Accept = 31,
    Connect = 32,
    Sigaction = 33,
    Sigprocmask = 34,
    Sigreturn = 35,
    Kill = 36,
    Dup = 37,
    Dup2 = 38,
    Shutdown = 39,
    ReadShutdownSignal = 40,
    Capget = 41,
    Capset = 42,
    Clone = 43,
    Prctl = 44,
    InputRead = 45,
    GbmCreate = 46,
    GbmMap = 47,
    GbmDestroy = 48,
    DrmPageFlip = 49,
    SetUid = 50,
    SetGid = 51,
    Chdir = 52,
    Dmesg = 53,
    XattrGet = 54,
    XattrSet = 55,
    NetSetAddr = 56,
    NetSetRoute = 57,
    NetQuery = 58,
    Ftruncate = 59,
    Mmap2 = 60,
    ShmOpen = 61,
    ShmUnlink = 62,
    MqOpen = 63,
    MqClose = 64,
    MqUnlink = 65,
    MqSend = 66,
    MqReceive = 67,
    Futex = 68,
    EpollCreate = 69,
    EpollCtl = 70,
    EpollWait = 71,
    SchedSetScheduler = 72,
    SchedGetScheduler = 73,
    CgroupCreate = 74,
    CgroupAddProcess = 75,
    CgroupSetCpuMax = 76,
    CgroupSetMemoryMax = 77,
    CgroupSetPidsMax = 78,
    EventFdCreate = 79,
    EventFdRead = 80,
    EventFdWrite = 81,
    TimerFdCreate = 82,
    TimerFdSettime = 83,
    TimerFdGettime = 84,
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
            28 => Some(Self::Socket),
            29 => Some(Self::Bind),
            30 => Some(Self::Listen),
            31 => Some(Self::Accept),
            32 => Some(Self::Connect),
            33 => Some(Self::Sigaction),
            34 => Some(Self::Sigprocmask),
            35 => Some(Self::Sigreturn),
            36 => Some(Self::Kill),
            37 => Some(Self::Dup),
            38 => Some(Self::Dup2),
            39 => Some(Self::Shutdown),
            40 => Some(Self::ReadShutdownSignal),
            41 => Some(Self::Capget),
            42 => Some(Self::Capset),
            43 => Some(Self::Clone),
            44 => Some(Self::Prctl),
            45 => Some(Self::InputRead),
            46 => Some(Self::GbmCreate),
            47 => Some(Self::GbmMap),
            48 => Some(Self::GbmDestroy),
            49 => Some(Self::DrmPageFlip),
            50 => Some(Self::SetUid),
            51 => Some(Self::SetGid),
            52 => Some(Self::Chdir),
            53 => Some(Self::Dmesg),
            54 => Some(Self::XattrGet),
            55 => Some(Self::XattrSet),
            56 => Some(Self::NetSetAddr),
            57 => Some(Self::NetSetRoute),
            58 => Some(Self::NetQuery),
            59 => Some(Self::Ftruncate),
            60 => Some(Self::Mmap2),
            61 => Some(Self::ShmOpen),
            62 => Some(Self::ShmUnlink),
            63 => Some(Self::MqOpen),
            64 => Some(Self::MqClose),
            65 => Some(Self::MqUnlink),
            66 => Some(Self::MqSend),
            67 => Some(Self::MqReceive),
            68 => Some(Self::Futex),
            69 => Some(Self::EpollCreate),
            70 => Some(Self::EpollCtl),
            71 => Some(Self::EpollWait),
            72 => Some(Self::SchedSetScheduler),
            73 => Some(Self::SchedGetScheduler),
            74 => Some(Self::CgroupCreate),
            75 => Some(Self::CgroupAddProcess),
            76 => Some(Self::CgroupSetCpuMax),
            77 => Some(Self::CgroupSetMemoryMax),
            78 => Some(Self::CgroupSetPidsMax),
            79 => Some(Self::EventFdCreate),
            80 => Some(Self::EventFdRead),
            81 => Some(Self::EventFdWrite),
            82 => Some(Self::TimerFdCreate),
            83 => Some(Self::TimerFdSettime),
            84 => Some(Self::TimerFdGettime),
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

/// Version number for capget/capset (must be `LINUX_CAPABILITY_VERSION_3 = 0x20080522`).
pub const LINUX_CAPABILITY_VERSION: u32 = 0x20080522;

/// Capability header for capget/capset.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapHeader {
    pub version: u32,
    pub pid: i32,
}

/// Capability data for capget/capset (maps to POSIX cap_user_data_t).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapData {
    pub effective: u64,
    pub permitted: u64,
    pub inheritable: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyscallArgs {
    pub arg0: u64,
    pub arg1: u64,
    pub arg2: u64,
    pub arg3: u64,
    pub arg4: u64,
    pub arg5: u64,
}

impl SyscallArgs {
    pub const fn new(arg0: u64, arg1: u64, arg2: u64, arg3: u64) -> Self {
        Self {
            arg0,
            arg1,
            arg2,
            arg3,
            arg4: 0,
            arg5: 0,
        }
    }

    pub const fn with_ext(arg0: u64, arg1: u64, arg2: u64, arg3: u64, arg4: u64, arg5: u64) -> Self {
        Self {
            arg0,
            arg1,
            arg2,
            arg3,
            arg4,
            arg5,
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
        assert_eq!(size_of::<SyscallArgs>(), 48);
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

    #[test]
    fn socket_syscall_id_is_twenty_eight() {
        assert_eq!(Syscall::Socket as u16, 28);
    }

    #[test]
    fn socket_syscall_round_trip() {
        assert_eq!(Syscall::from_u16(28), Some(Syscall::Socket));
    }

    #[test]
    fn bind_syscall_id_is_twenty_nine() {
        assert_eq!(Syscall::Bind as u16, 29);
    }

    #[test]
    fn bind_syscall_round_trip() {
        assert_eq!(Syscall::from_u16(29), Some(Syscall::Bind));
    }

    #[test]
    fn listen_syscall_id_is_thirty() {
        assert_eq!(Syscall::Listen as u16, 30);
    }

    #[test]
    fn listen_syscall_round_trip() {
        assert_eq!(Syscall::from_u16(30), Some(Syscall::Listen));
    }

    #[test]
    fn accept_syscall_id_is_thirty_one() {
        assert_eq!(Syscall::Accept as u16, 31);
    }

    #[test]
    fn accept_syscall_round_trip() {
        assert_eq!(Syscall::from_u16(31), Some(Syscall::Accept));
    }

    #[test]
    fn connect_syscall_id_is_thirty_two() {
        assert_eq!(Syscall::Connect as u16, 32);
    }

    #[test]
    fn connect_syscall_round_trip() {
        assert_eq!(Syscall::from_u16(32), Some(Syscall::Connect));
    }

    #[test]
    fn sigaction_syscall_id_is_thirty_three() {
        assert_eq!(Syscall::Sigaction as u16, 33);
    }

    #[test]
    fn sigaction_syscall_round_trip() {
        assert_eq!(Syscall::from_u16(33), Some(Syscall::Sigaction));
    }

    #[test]
    fn sigprocmask_syscall_id_is_thirty_four() {
        assert_eq!(Syscall::Sigprocmask as u16, 34);
    }

    #[test]
    fn sigprocmask_syscall_round_trip() {
        assert_eq!(Syscall::from_u16(34), Some(Syscall::Sigprocmask));
    }

    #[test]
    fn sigreturn_syscall_id_is_thirty_five() {
        assert_eq!(Syscall::Sigreturn as u16, 35);
    }

    #[test]
    fn sigreturn_syscall_round_trip() {
        assert_eq!(Syscall::from_u16(35), Some(Syscall::Sigreturn));
    }

    #[test]
    fn kill_syscall_id_is_thirty_six() {
        assert_eq!(Syscall::Kill as u16, 36);
    }

    #[test]
    fn kill_syscall_round_trip() {
        assert_eq!(Syscall::from_u16(36), Some(Syscall::Kill));
    }

    #[test]
    fn dup_syscall_id_is_thirty_seven() {
        assert_eq!(Syscall::Dup as u16, 37);
    }

    #[test]
    fn dup_syscall_round_trip() {
        assert_eq!(Syscall::from_u16(37), Some(Syscall::Dup));
    }

    #[test]
    fn dup2_syscall_id_is_thirty_eight() {
        assert_eq!(Syscall::Dup2 as u16, 38);
    }

    #[test]
    fn dup2_syscall_round_trip() {
        assert_eq!(Syscall::from_u16(38), Some(Syscall::Dup2));
    }

    #[test]
    fn shutdown_syscall_id_is_thirty_nine() {
        assert_eq!(Syscall::Shutdown as u16, 39);
    }

    #[test]
    fn shutdown_syscall_round_trip() {
        assert_eq!(Syscall::from_u16(39), Some(Syscall::Shutdown));
    }

    #[test]
    fn read_shutdown_signal_syscall_id_is_forty() {
        assert_eq!(Syscall::ReadShutdownSignal as u16, 40);
    }

    #[test]
    fn read_shutdown_signal_syscall_round_trip() {
        assert_eq!(Syscall::from_u16(40), Some(Syscall::ReadShutdownSignal));
    }
}
