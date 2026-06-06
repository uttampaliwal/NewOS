//! VFS compatibility shim.
//!
//! The canonical VFS implementation lives in [`crate::fs::vfs`].
//! This module re-exports everything needed by the rest of the kernel so that
//! existing `use crate::vfs::…` references continue to compile without change.

pub use crate::fs::vfs::{
    DirEntry,
    DirEntry as VfsEntry, // keep old name available as alias
    FdKind,
    // Core types
    FileDescriptor,
    FileStat,
    FileType,
    FsBackend,
    FsError,
    InodeId,
    InodeStat,
    // Limits / misc
    MAX_OPEN_FILES,
    MountEntry,
    MountFlags,
    OpenFlags,
    // Placeholder IPC types
    PipeBuffer,
    Timestamp,
    UnixSocketState,
    // Global singleton
    VFS,
    Vfs,
    // Helpers
    check_permission,
};
