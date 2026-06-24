#![cfg_attr(not(test), no_std)]

//! Unified kernel error types for Turnix OS.
//!
//! This crate provides a single `KernelError` enum that all subsystems
//! map their internal errors to at boundaries. This ensures consistent
//! error handling across VFS, syscalls, drivers, IPC, and security.

use core::fmt;

/// Unified kernel error type.
///
/// Every subsystem converts its internal error variants into one of
/// these codes. Syscall handlers translate `KernelError` into POSIX
/// errno values for userspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i64)]
pub enum KernelError {
    /// Operation not permitted (EPERM)
    PermissionDenied = 1,
    /// No such file or directory (ENOENT)
    NotFound = 2,
    /// No such process (ESRCH)
    ProcessNotFound = 3,
    /// Interrupted system call (EINTR)
    Interrupted = 4,
    /// I/O error (EIO)
    IoError = 5,
    /// No such device or address (ENXIO)
    NoDevice = 6,
    /// Bad file descriptor (EBADF)
    BadFileDescriptor = 9,
    /// Cannot allocate memory (ENOMEM)
    OutOfMemory = 12,
    /// Permission denied (EACCES)
    AccessDenied = 13,
    /// Bad address (EFAULT)
    InvalidAddress = 14,
    /// Block device required (ENOTBLK)
    NotBlockDevice = 15,
    /// Device or resource busy (EBUSY)
    Busy = 16,
    /// File exists (EEXIST)
    AlreadyExists = 17,
    /// Cross-device link (EXDEV)
    CrossDeviceLink = 18,
    /// No such device (ENODEV)
    DeviceNotFound = 19,
    /// Not a directory (ENOTDIR)
    NotADirectory = 20,
    /// Is a directory (EISDIR)
    IsADirectory = 21,
    /// Invalid argument (EINVAL)
    InvalidArgument = 22,
    /// Too many open files in system (ENFILE)
    TooManyFiles = 23,
    /// Too many open files (EMFILE)
    ProcessLimitReached = 24,
    /// Not a typewriter (ENOTTY)
    NotATerminal = 25,
    /// Text file busy (ETXTBSY)
    TextBusy = 26,
    /// File too large (EFBIG)
    FileTooLarge = 27,
    /// No space left on device (ENOSPC)
    NoSpace = 28,
    /// Illegal seek (ESPIPE)
    IllegalSeek = 29,
    /// Read-only file system (EROFS)
    ReadOnly = 30,
    /// Too many links (EMLINK)
    TooManyLinks = 31,
    /// Broken pipe (EPIPE)
    BrokenPipe = 32,
    /// Math argument out of domain of func (EDOM)
    MathDomain = 33,
    /// Math result not representable (ERANGE)
    MathRange = 34,
    /// Deadlock avoided (EDEADLK)
    Deadlock = 35,
    /// File name too long (ENAMETOOLONG)
    NameTooLong = 36,
    /// No record locks available (ENOLCK)
    NoLocks = 37,
    /// Function not implemented (ENOSYS)
    NotImplemented = 38,
    /// Directory not empty (ENOTEMPTY)
    NotEmpty = 39,
    /// Too many symbolic links encountered (ELOOP)
    TooManySymlinks = 40,
    /// Operation would block (EWOULDBLOCK / EAGAIN)
    WouldBlock = 41,
    /// No message of desired type (ENOMSG)
    NoMessage = 42,
    /// Identifier removed (EIDRM)
    IdentifierRemoved = 43,
    /// Channel number out of range (ECHRNG)
    ChannelRange = 44,
    /// Level 2 not synchronized (L2NSYNC)
    Level2Sync = 45,
    /// Level 3 halted (L3HLT)
    Level3Halt = 46,
    /// Level 3 reset (L3RST)
    Level3Reset = 47,
    /// Link number out of range (HLNRNG)
    LinkRange = 48,
    /// Protocol driver not attached (EPDLBK)
    ProtocolDetached = 49,
    /// No CSI structure available (ENOSTR)
    NoCsiStructure = 50,
    /// Level 2 halted (EPROTO)
    ProtocolError = 51,
    /// No STREAMS resources (ENOSR)
    NoStreamsResources = 52,
    /// STREAMS pipe error (ENOSTR)
    StreamsPipe = 53,
    /// Timer expired (ETIME)
    TimerExpired = 54,
    /// Out of streams resources (ENOSR)
    OutOfStreams = 55,
    /// Machine is not on the network (ENONET)
    NotOnNetwork = 56,
    /// Package not installed (ENOPKG)
    PackageNotInstalled = 57,
    /// Advertise error (EADV)
    AdvertiseError = 58,
    /// Srmount error (ESRMNT)
    SrmountError = 59,
    /// Communication error on send (ECOMM)
    CommSendError = 60,
    /// Protocol error (EPROTO)
    ProtocolError2 = 61,
    /// Multihop attempted (EMULTIHOP)
    Multihop = 62,
    /// RFS specific error (ENOTUNIQ)
    RfsError = 63,
    /// Bad message (EBADMSG)
    BadMessage = 64,
    /// Value too large for defined data type (EOVERFLOW)
    Overflow = 65,
    /// Name not unique on network (ENOTUNIQ)
    NameNotUnique = 66,
    /// File descriptor in bad state (EBADFD)
    BadFd = 67,
    /// Remote address changed (EREMCHG)
    RemoteChanged = 68,
    /// Access shared access revoked (ELIBACC)
    AccessRevoked = 69,
    /// Disk error (EIO)
    DiskError = 70,
}

impl KernelError {
    /// Convert to POSIX errno value.
    pub fn to_errno(self) -> i64 {
        self as i64
    }

    /// Create from POSIX errno value.
    pub fn from_errno(val: i64) -> Option<Self> {
        match val {
            1 => Some(Self::PermissionDenied),
            2 => Some(Self::NotFound),
            3 => Some(Self::ProcessNotFound),
            4 => Some(Self::Interrupted),
            5 => Some(Self::IoError),
            6 => Some(Self::NoDevice),
            9 => Some(Self::BadFileDescriptor),
            12 => Some(Self::OutOfMemory),
            13 => Some(Self::AccessDenied),
            14 => Some(Self::InvalidAddress),
            16 => Some(Self::Busy),
            17 => Some(Self::AlreadyExists),
            20 => Some(Self::NotADirectory),
            21 => Some(Self::IsADirectory),
            22 => Some(Self::InvalidArgument),
            28 => Some(Self::NoSpace),
            30 => Some(Self::ReadOnly),
            32 => Some(Self::BrokenPipe),
            36 => Some(Self::NameTooLong),
            38 => Some(Self::NotImplemented),
            39 => Some(Self::NotEmpty),
            _ => None,
        }
    }
}

impl fmt::Display for KernelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PermissionDenied => write!(f, "Permission denied"),
            Self::NotFound => write!(f, "No such file or directory"),
            Self::ProcessNotFound => write!(f, "No such process"),
            Self::Interrupted => write!(f, "Interrupted system call"),
            Self::IoError => write!(f, "I/O error"),
            Self::NoDevice => write!(f, "No such device or address"),
            Self::BadFileDescriptor => write!(f, "Bad file descriptor"),
            Self::OutOfMemory => write!(f, "Cannot allocate memory"),
            Self::AccessDenied => write!(f, "Permission denied"),
            Self::InvalidAddress => write!(f, "Bad address"),
            Self::NotBlockDevice => write!(f, "Block device required"),
            Self::Busy => write!(f, "Device or resource busy"),
            Self::AlreadyExists => write!(f, "File exists"),
            Self::CrossDeviceLink => write!(f, "Cross-device link"),
            Self::DeviceNotFound => write!(f, "No such device"),
            Self::NotADirectory => write!(f, "Not a directory"),
            Self::IsADirectory => write!(f, "Is a directory"),
            Self::InvalidArgument => write!(f, "Invalid argument"),
            Self::TooManyFiles => write!(f, "Too many open files in system"),
            Self::ProcessLimitReached => write!(f, "Too many open files"),
            Self::NotATerminal => write!(f, "Not a typewriter"),
            Self::TextBusy => write!(f, "Text file busy"),
            Self::FileTooLarge => write!(f, "File too large"),
            Self::NoSpace => write!(f, "No space left on device"),
            Self::IllegalSeek => write!(f, "Illegal seek"),
            Self::ReadOnly => write!(f, "Read-only file system"),
            Self::TooManyLinks => write!(f, "Too many links"),
            Self::BrokenPipe => write!(f, "Broken pipe"),
            Self::MathDomain => write!(f, "Math argument out of domain"),
            Self::MathRange => write!(f, "Math result not representable"),
            Self::Deadlock => write!(f, "Deadlock avoided"),
            Self::NameTooLong => write!(f, "File name too long"),
            Self::NoLocks => write!(f, "No record locks available"),
            Self::NotImplemented => write!(f, "Function not implemented"),
            Self::NotEmpty => write!(f, "Directory not empty"),
            Self::TooManySymlinks => write!(f, "Too many symbolic links"),
            Self::WouldBlock => write!(f, "Operation would block"),
            Self::NoMessage => write!(f, "No message of desired type"),
            Self::IdentifierRemoved => write!(f, "Identifier removed"),
            Self::ChannelRange => write!(f, "Channel number out of range"),
            Self::Level2Sync => write!(f, "Level 2 not synchronized"),
            Self::Level3Halt => write!(f, "Level 3 halted"),
            Self::Level3Reset => write!(f, "Level 3 reset"),
            Self::LinkRange => write!(f, "Link number out of range"),
            Self::ProtocolDetached => write!(f, "Protocol driver not attached"),
            Self::NoCsiStructure => write!(f, "No CSI structure available"),
            Self::ProtocolError => write!(f, "Protocol error"),
            Self::NoStreamsResources => write!(f, "No STREAMS resources"),
            Self::StreamsPipe => write!(f, "STREAMS pipe error"),
            Self::TimerExpired => write!(f, "Timer expired"),
            Self::OutOfStreams => write!(f, "Out of streams resources"),
            Self::NotOnNetwork => write!(f, "Machine is not on the network"),
            Self::PackageNotInstalled => write!(f, "Package not installed"),
            Self::AdvertiseError => write!(f, "Advertise error"),
            Self::SrmountError => write!(f, "Srmount error"),
            Self::CommSendError => write!(f, "Communication error on send"),
            Self::ProtocolError2 => write!(f, "Protocol error"),
            Self::Multihop => write!(f, "Multihop attempted"),
            Self::RfsError => write!(f, "RFS specific error"),
            Self::BadMessage => write!(f, "Bad message"),
            Self::Overflow => write!(f, "Value too large for defined data type"),
            Self::NameNotUnique => write!(f, "Name not unique on network"),
            Self::BadFd => write!(f, "File descriptor in bad state"),
            Self::RemoteChanged => write!(f, "Remote address changed"),
            Self::AccessRevoked => write!(f, "Shared access revoked"),
            Self::DiskError => write!(f, "Disk error"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errno_roundtrip() {
        for errno in 1..70 {
            if let Some(err) = KernelError::from_errno(errno) {
                assert_eq!(err.to_errno(), errno);
            }
        }
    }

    #[test]
    fn known_errors() {
        assert_eq!(KernelError::NotFound.to_errno(), 2);
        assert_eq!(KernelError::PermissionDenied.to_errno(), 1);
        assert_eq!(KernelError::OutOfMemory.to_errno(), 12);
        assert_eq!(KernelError::InvalidArgument.to_errno(), 22);
        assert_eq!(KernelError::NotImplemented.to_errno(), 38);
    }

    #[test]
    fn display_messages() {
        assert_eq!(
            format!("{}", KernelError::NotFound),
            "No such file or directory"
        );
        assert_eq!(format!("{}", KernelError::BrokenPipe), "Broken pipe");
    }
}
