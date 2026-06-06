//! VFS compatibility shim.
//!
//! The canonical VFS implementation lives in [`crate::fs::vfs`].
//! This module re-exports everything needed by the rest of the kernel so that
//! existing `use crate::vfs::…` references continue to compile without change.

pub use crate::fs::vfs::{
    // Global singleton
    VFS,
    // Core types
    FileDescriptor,
    FdKind,
    InodeId,
    InodeStat,
    OpenFlags,
    MountFlags,
    FsError,
    FileType,
    FileStat,
    DirEntry,
    DirEntry as VfsEntry,   // keep old name available as alias
    FsBackend,
    MountEntry,
    Vfs,
    // Placeholder IPC types
    PipeBuffer,
    UnixSocketState,
    // Helpers
    check_permission,
    // Limits / misc
    MAX_OPEN_FILES,
    Timestamp,
};
