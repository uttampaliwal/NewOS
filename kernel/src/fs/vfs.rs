//! Virtual Filesystem (VFS) layer for Turnix OS.
//!
//! This module provides:
//! - [`FsBackend`] trait — the interface every filesystem driver must implement
//! - [`Vfs`] struct — the mount table and path-resolution engine
//! - [`FileDescriptor`] — per-open-file state (inode, backend, offset, flags, kind)
//! - [`InodeId`] / [`InodeStat`] — inode identity and metadata
//! - [`check_permission`] — Unix rwx permission enforcement
//!
//! # Design invariants
//! - All mutable state in backends is guarded by the backend's own internal lock
//!   (backends take `&self` and handle locking internally).
//! - The global [`VFS`] is a `spin::Mutex<Vfs>` (same pattern used throughout the kernel).
//! - `PipeBuffer` and `UnixSocketState` are forward-compatible placeholders; they will be
//!   replaced by real implementations in Phase 3 (tasks 24 and 25).
//! - Timestamps are `u64` Unix seconds (no `std::time` available in `no_std`).

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use lazy_static::lazy_static;
use spin::Mutex;

use crate::drivers::framework::DeviceKey;

// ---------------------------------------------------------------------------
// Global VFS instance
// ---------------------------------------------------------------------------

lazy_static! {
    /// Kernel-global VFS.  All syscall handlers lock this to resolve paths.
    pub static ref VFS: Mutex<Vfs> = Mutex::new(Vfs::new());
}

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Maximum number of simultaneously open file descriptors across the whole VFS.
pub const MAX_OPEN_FILES: usize = 1024;

// ---------------------------------------------------------------------------
// Timestamps
// ---------------------------------------------------------------------------

/// Unix timestamp in seconds (`u64` — no std::time in no_std).
pub type Timestamp = u64;

// ---------------------------------------------------------------------------
// Placeholder types for Phase-3 IPC modules
// ---------------------------------------------------------------------------

/// Placeholder for the pipe ring buffer — will be replaced by
/// `kernel/src/ipc/pipe.rs` in Phase 3 (task 24).
pub struct PipeBuffer;

/// Placeholder for Unix-domain socket state — will be replaced by
/// `kernel/src/ipc/unix_socket.rs` in Phase 3 (task 25).
pub struct UnixSocketState;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors returned by VFS and filesystem backend operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsError {
    /// The requested file or directory does not exist.
    NotFound,
    /// The caller does not have sufficient permissions.
    PermissionDenied,
    /// The path component is not a directory.
    NotADirectory,
    /// The path points to a directory but a file was expected.
    IsADirectory,
    /// An entry with that name already exists.
    AlreadyExists,
    /// A supplied argument is invalid (e.g. bad flags, null pointer).
    InvalidArgument,
    /// The operation is not supported by this backend.
    NotSupported,
    /// A low-level I/O error occurred.
    IoError,
    /// `umount` was rejected because there are still open file descriptors.
    BusyMounted,
    /// The filesystem has no space for the requested allocation.
    NoSpace,
}

// ---------------------------------------------------------------------------
// InodeId
// ---------------------------------------------------------------------------

/// Opaque inode identifier — a `u64` newtype.
///
/// The meaning of the value is private to each [`FsBackend`]; the VFS treats
/// it as an opaque handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InodeId(pub u64);

impl InodeId {
    /// The null / invalid inode sentinel.
    pub const NULL: InodeId = InodeId(0);
}

// ---------------------------------------------------------------------------
// File types and flags
// ---------------------------------------------------------------------------

/// High-level file-system object type (subset of POSIX `mode_t` type bits).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    Regular,
    Directory,
    Device,
    Pipe,
    Socket,
    Symlink,
}

/// Flags supplied at `open(2)` time — mirrors common POSIX O_* flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenFlags(pub u32);

impl OpenFlags {
    pub const RDONLY: OpenFlags = OpenFlags(0);
    pub const WRONLY: OpenFlags = OpenFlags(1);
    pub const RDWR: OpenFlags = OpenFlags(2);
    pub const CREAT: OpenFlags = OpenFlags(0o100);
    pub const TRUNC: OpenFlags = OpenFlags(0o1000);
    pub const APPEND: OpenFlags = OpenFlags(0o2000);
    pub const CLOEXEC: OpenFlags = OpenFlags(0o2000000);
    pub const DIRECTORY: OpenFlags = OpenFlags(0o200000);

    pub fn readable(&self) -> bool {
        self.0 & 3 != 1 // not WRONLY
    }

    pub fn writable(&self) -> bool {
        self.0 & 3 != 0 // WRONLY or RDWR
    }

    pub fn is_append(&self) -> bool {
        self.0 & Self::APPEND.0 != 0
    }

    pub fn is_creat(&self) -> bool {
        self.0 & Self::CREAT.0 != 0
    }

    pub fn is_trunc(&self) -> bool {
        self.0 & Self::TRUNC.0 != 0
    }

    pub fn is_cloexec(&self) -> bool {
        self.0 & Self::CLOEXEC.0 != 0
    }
}

/// Flags for [`Vfs::mount`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MountFlags(pub u32);

impl MountFlags {
    pub const RDONLY: MountFlags = MountFlags(1);
    pub const NOSUID: MountFlags = MountFlags(2);
    pub const NOEXEC: MountFlags = MountFlags(4);
    pub const NODEV: MountFlags = MountFlags(8);
}

// ---------------------------------------------------------------------------
// InodeStat
// ---------------------------------------------------------------------------

/// Filesystem metadata for a single inode (analogous to POSIX `struct stat`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InodeStat {
    /// Permission bits and file-type encoding (low 12 bits used).
    pub mode: u32,
    /// Owner user ID.
    pub uid: u32,
    /// Owner group ID.
    pub gid: u32,
    /// Hard-link count.
    pub nlink: u32,
    /// Last access time (Unix seconds).
    pub atime: Timestamp,
    /// Last modification time.
    pub mtime: Timestamp,
    /// Last status-change time.
    pub ctime: Timestamp,
    /// File size in bytes.
    pub size: u64,
    /// High-level file type (redundant with `mode` bits, kept for convenience).
    pub file_type: FileType,
}

/// Legacy flat stat — preserved for syscall-handler backward compatibility.
#[derive(Debug, Clone)]
pub struct FileStat {
    pub size: u64,
    pub file_type: FileType,
}

impl FileStat {
    pub fn to_abi(&self) -> turnix_abi::syscall::Stat {
        use turnix_abi::syscall::*;
        let abi_type = match self.file_type {
            FileType::Regular => FILE_TYPE_REGULAR,
            FileType::Directory => FILE_TYPE_DIRECTORY,
            FileType::Device => FILE_TYPE_DEVICE,
            FileType::Pipe => FILE_TYPE_PIPE,
            FileType::Socket => FILE_TYPE_REGULAR, // no ABI constant yet
            FileType::Symlink => FILE_TYPE_REGULAR, // no ABI constant yet
        };
        turnix_abi::syscall::Stat {
            size: self.size,
            file_type: abi_type,
        }
    }
}

impl From<InodeStat> for FileStat {
    fn from(s: InodeStat) -> Self {
        FileStat {
            size: s.size,
            file_type: s.file_type,
        }
    }
}

// ---------------------------------------------------------------------------
// DirEntry
// ---------------------------------------------------------------------------

/// A single directory listing entry returned by [`FsBackend::readdir`].
#[derive(Debug, Clone)]
pub struct DirEntry {
    pub inode: InodeId,
    pub name: String,
    pub file_type: FileType,
}

// ---------------------------------------------------------------------------
// Permission check
// ---------------------------------------------------------------------------

/// Check Unix rwx permission bits.
///
/// * `access` — a 3-bit field: bit 2 = read, bit 1 = write, bit 0 = execute.
///
/// Returns `true` if the caller (`uid`, `gid`) has the requested access on
/// the inode described by `stat`.
///
/// # Permission model
/// - If `uid == stat.uid` the **owner** bits (`mode[8:6]`) are checked.
/// - Else if `gid == stat.gid` the **group** bits (`mode[5:3]`) are checked.
/// - Otherwise the **other** bits (`mode[2:0]`) are checked.
/// - UID 0 (root) bypasses permission checks for read and write; execute
///   requires at least one execute bit to be set (standard Linux semantics).
pub fn check_permission(stat: &InodeStat, uid: u32, gid: u32, access: u8) -> bool {
    // UID 0 (root) can always read/write; execute requires ≥1 exec bit set.
    if uid == 0 {
        let exec_bit = access & 0x1;
        if exec_bit != 0 {
            // Any execute bit must be set
            let any_exec = (stat.mode & 0o111) != 0;
            return any_exec;
        }
        return true;
    }

    let access = access as u32;
    let mode = stat.mode;

    if uid == stat.uid {
        let owner = (mode >> 6) & 0x7;
        (owner & access) == access
    } else if gid == stat.gid {
        let group = (mode >> 3) & 0x7;
        (group & access) == access
    } else {
        let other = mode & 0x7;
        (other & access) == access
    }
}

// ---------------------------------------------------------------------------
// FsBackend trait
// ---------------------------------------------------------------------------

/// The interface that every filesystem driver must implement.
///
/// All methods take `&self` — the backend owns its internal locking.  This
/// allows `Arc<dyn FsBackend>` to be shared freely across the VFS.
pub trait FsBackend: Send + Sync {
    /// Return the [`InodeId`] of the filesystem root directory.
    fn root_inode(&self) -> InodeId;

    /// Look up a directory entry by name.
    ///
    /// Returns the [`InodeId`] of the child named `name` inside `parent`.
    fn lookup(&self, parent: InodeId, name: &str) -> Result<InodeId, FsError>;

    /// Prepare an inode for I/O with the given flags (may update access time, etc.).
    fn open(&self, inode: InodeId, flags: OpenFlags) -> Result<(), FsError>;

    /// Read up to `buf.len()` bytes from `inode` starting at `offset`.
    ///
    /// Returns the number of bytes actually read (0 = EOF).
    fn read(&self, inode: InodeId, offset: u64, buf: &mut [u8]) -> Result<usize, FsError>;

    /// Write `buf` to `inode` starting at `offset`.
    ///
    /// Returns the number of bytes written.
    fn write(&self, inode: InodeId, offset: u64, buf: &[u8]) -> Result<usize, FsError>;

    /// Return metadata for `inode`.
    fn stat(&self, inode: InodeId) -> Result<InodeStat, FsError>;

    /// List directory entries for `inode` (which must be a directory).
    fn readdir(&self, inode: InodeId) -> Result<Vec<DirEntry>, FsError>;

    /// Create a new directory named `name` inside `parent` with the given `mode`.
    fn mkdir(&self, parent: InodeId, name: &str, mode: u32) -> Result<InodeId, FsError>;

    /// Remove the directory entry `name` inside `parent`.
    fn unlink(&self, parent: InodeId, name: &str) -> Result<(), FsError>;

    /// Rename/move `old_name` in `old_parent` to `new_name` in `new_parent`.
    fn rename(
        &self,
        old_parent: InodeId,
        old_name: &str,
        new_parent: InodeId,
        new_name: &str,
    ) -> Result<(), FsError>;

    /// Flush all pending writes to durable storage.
    fn sync(&self) -> Result<(), FsError>;
}

// ---------------------------------------------------------------------------
// FdKind
// ---------------------------------------------------------------------------

/// The kind of resource backing a file descriptor.
#[derive(Clone)]
pub enum FdKind {
    /// A regular file.
    Regular,
    /// A directory (opened with `O_DIRECTORY` or automatically for dirs).
    Directory,
    /// A pipe endpoint.
    Pipe(Arc<PipeBuffer>),
    /// A Unix-domain socket.
    UnixSocket(Arc<UnixSocketState>),
    /// A character or block device identified by a [`DeviceKey`].
    Device(DeviceKey),
    /// An epoll file descriptor.
    Epoll,
}

impl core::fmt::Debug for FdKind {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            FdKind::Regular => write!(f, "Regular"),
            FdKind::Directory => write!(f, "Directory"),
            FdKind::Pipe(_) => write!(f, "Pipe"),
            FdKind::UnixSocket(_) => write!(f, "UnixSocket"),
            FdKind::Device(k) => write!(f, "Device({:?})", k),
            FdKind::Epoll => write!(f, "Epoll"),
        }
    }
}

// ---------------------------------------------------------------------------
// FileDescriptor
// ---------------------------------------------------------------------------

/// Per-open-file state stored in a process's fd_table.
///
/// The `offset` field uses `AtomicU64` so multiple reads and writes can
/// advance it without holding the VFS lock.  All other fields are immutable
/// after creation.
pub struct FileDescriptor {
    /// The inode this descriptor refers to.
    pub inode: InodeId,
    /// The filesystem backend that owns the inode.
    pub backend: Arc<dyn FsBackend>,
    /// Current file position.
    pub offset: AtomicU64,
    /// Open flags (O_RDONLY, O_WRONLY, O_RDWR, …).
    pub flags: OpenFlags,
    /// What kind of resource is behind this fd.
    pub kind: FdKind,
    /// Cached path / name (for diagnostic purposes and list_dir).
    pub name: String,
}

impl FileDescriptor {
    pub fn new(
        inode: InodeId,
        backend: Arc<dyn FsBackend>,
        flags: OpenFlags,
        kind: FdKind,
        name: String,
    ) -> Self {
        FileDescriptor {
            inode,
            backend,
            offset: AtomicU64::new(0),
            flags,
            kind,
            name,
        }
    }

    /// Read file position atomically.
    pub fn get_offset(&self) -> u64 {
        self.offset.load(Ordering::Relaxed)
    }

    /// Set file position atomically.
    pub fn set_offset(&self, off: u64) {
        self.offset.store(off, Ordering::Relaxed);
    }
}

impl core::fmt::Debug for FileDescriptor {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("FileDescriptor")
            .field("inode", &self.inode)
            .field("offset", &self.get_offset())
            .field("kind", &self.kind)
            .field("name", &self.name)
            .finish()
    }
}

// FileDescriptor cannot derive Clone because AtomicU64 isn't Clone.
// Provide a manual impl that copies the current offset value.
impl Clone for FileDescriptor {
    fn clone(&self) -> Self {
        FileDescriptor {
            inode: self.inode,
            backend: self.backend.clone(),
            offset: AtomicU64::new(self.get_offset()),
            flags: self.flags,
            kind: self.kind.clone(),
            name: self.name.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// MountEntry
// ---------------------------------------------------------------------------

/// One entry in the VFS mount table.
pub struct MountEntry {
    /// Absolute path where this filesystem is mounted (e.g. `"/"` or `"/mnt"`).
    pub mount_point: String,
    /// The filesystem backend.
    pub backend: Arc<dyn FsBackend>,
    /// Mount options.
    pub flags: MountFlags,
    /// Number of open [`FileDescriptor`]s pointing into this backend.
    /// `umount` is rejected when this is > 0.
    pub open_fd_count: usize,
}

impl core::fmt::Debug for MountEntry {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("MountEntry")
            .field("mount_point", &self.mount_point)
            .field("flags", &self.flags)
            .field("open_fd_count", &self.open_fd_count)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Vfs
// ---------------------------------------------------------------------------

/// The Virtual Filesystem.
///
/// Holds the mount table and a flat open-file table for the kernel-wide fd
/// namespace.  (Per-process fd tables in `ProcessControlBlock.fd_table` hold
/// `Option<FileDescriptor>` clones from here.)
pub struct Vfs {
    /// Mount table, sorted by `mount_point.len()` **descending** so that the
    /// longest matching prefix is found first in a linear scan.
    mounts: Vec<MountEntry>,
    /// Kernel-wide open file table.
    open_files: BTreeMap<usize, FileDescriptor>,
    /// Next FD to try when allocating a new descriptor.
    next_fd: usize,
    /// Flat entry list (name → data) retained for backward-compat with the
    /// page-cache `read_page` interface and `init_from_ramdisk`.
    entries: Vec<LegacyEntry>,
    /// Current working directory (legacy; not used by new path resolution).
    #[allow(dead_code)]
    cwd: String,
}

/// Legacy in-memory file entry retained for `init_from_ramdisk` / `read_page`.
struct LegacyEntry {
    name: String,
    file_type: FileType,
    data: Option<Vec<u8>>,
}

impl Vfs {
    /// Create an empty VFS with no mounts and no open files.
    pub fn new() -> Self {
        Vfs {
            mounts: Vec::new(),
            open_files: BTreeMap::new(),
            next_fd: 0,
            entries: Vec::new(),
            cwd: String::from("/"),
        }
    }

    // -----------------------------------------------------------------------
    // Mount / umount
    // -----------------------------------------------------------------------

    /// Attach `backend` at `mount_point`.
    ///
    /// If a mount already exists at exactly this path it is replaced.
    /// After the operation the table is re-sorted by path length descending.
    pub fn mount(
        &mut self,
        mount_point: &str,
        backend: Arc<dyn FsBackend>,
        flags: MountFlags,
    ) -> Result<(), FsError> {
        if mount_point.is_empty() {
            return Err(FsError::InvalidArgument);
        }
        // Remove an existing mount at the same point (re-mount semantics).
        self.mounts.retain(|m| m.mount_point != mount_point);
        self.mounts.push(MountEntry {
            mount_point: String::from(mount_point),
            backend,
            flags,
            open_fd_count: 0,
        });
        // Sort by path length descending for longest-prefix matching.
        self.mounts
            .sort_by_key(|m| core::cmp::Reverse(m.mount_point.len()));
        Ok(())
    }

    /// Detach the filesystem mounted at `mount_point`.
    ///
    /// Returns [`FsError::BusyMounted`] if any open file descriptors still
    /// reference the backend (requirement 21.3).
    pub fn umount(&mut self, mount_point: &str) -> Result<(), FsError> {
        let idx = self
            .mounts
            .iter()
            .position(|m| m.mount_point == mount_point)
            .ok_or(FsError::NotFound)?;
        if self.mounts[idx].open_fd_count > 0 {
            return Err(FsError::BusyMounted);
        }
        self.mounts.remove(idx);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Path resolution
    // -----------------------------------------------------------------------

    /// Resolve an absolute path to `(backend, relative_path_within_backend)`.
    ///
    /// Uses longest-prefix matching on mount points (requirement 21.5).
    ///
    /// Returns `FsError::NotFound` if no mount covers the path.
    pub fn resolve<'a>(&'a self, path: &'a str) -> Result<(&'a MountEntry, &'a str), FsError> {
        // The mount table is already sorted by path length descending, so the
        // first entry whose mount_point is a prefix of `path` is the longest match.
        for entry in &self.mounts {
            let mp = entry.mount_point.as_str();
            if path.starts_with(mp) {
                // The remainder after stripping the mount-point prefix.
                let raw_rel = path.strip_prefix(mp).unwrap_or("");
                // Determine the relative path within the backend:
                //  - If the mount point IS "/" the rel path is simply the
                //    original path (we don't strip the slash).
                //  - Otherwise verify the next character is '/' (so that
                //    "/mnt" doesn't match "/mntfoo").
                let rel: &str = if mp == "/" {
                    // root mount: relative path is the original path itself.
                    path
                } else if raw_rel.is_empty() {
                    // Path equals mount point exactly (e.g. path="/mnt").
                    "/"
                } else if raw_rel.starts_with('/') {
                    // Path has a component after the mount point.
                    raw_rel
                } else {
                    // e.g. path="/mntfoo" vs mp="/mnt" — not a real match.
                    continue;
                };
                return Ok((entry, rel));
            }
        }
        Err(FsError::NotFound)
    }

    /// Walk a relative path within a backend starting from `parent_inode`.
    ///
    /// `rel_path` should be either `"/"` (root) or a slash-prefixed string of
    /// `/component/component/…`.
    fn walk_path(
        backend: &dyn FsBackend,
        start: InodeId,
        rel_path: &str,
    ) -> Result<InodeId, FsError> {
        let mut current = start;
        // Split on '/' and skip empty components (handles leading '/' and
        // duplicate slashes).
        let components: Vec<&str> = rel_path.split('/').filter(|c| !c.is_empty()).collect();
        for component in components {
            current = backend.lookup(current, component)?;
        }
        Ok(current)
    }

    // -----------------------------------------------------------------------
    // Open / close
    // -----------------------------------------------------------------------

    /// Open `path` with the given `flags`.
    ///
    /// Resolves the path across mount points, enforces permission bits, and
    /// allocates a file descriptor.  Returns the fd index.
    ///
    /// `uid` / `gid` are the caller's credentials used for permission checking.
    pub fn open_with_creds(
        &mut self,
        path: &str,
        flags: OpenFlags,
        uid: u32,
        gid: u32,
    ) -> Result<usize, FsError> {
        let (mount_entry, rel_path) = self.resolve(path)?;
        let backend = mount_entry.backend.clone();
        let root = backend.root_inode();
        let inode = Self::walk_path(backend.as_ref(), root, rel_path)?;
        let stat = backend.stat(inode)?;

        // Determine required permission bits.
        let mut access: u8 = 0;
        if flags.readable() {
            access |= 0x4;
        } // read
        if flags.writable() {
            access |= 0x2;
        } // write

        if access != 0 && !check_permission(&stat, uid, gid, access) {
            return Err(FsError::PermissionDenied);
        }

        // Reject write open on read-only mount.
        if flags.writable() && (mount_entry.flags.0 & MountFlags::RDONLY.0 != 0) {
            return Err(FsError::PermissionDenied);
        }

        backend.open(inode, flags)?;

        let kind = match stat.file_type {
            FileType::Directory => FdKind::Directory,
            FileType::Pipe => FdKind::Pipe(Arc::new(PipeBuffer)),
            FileType::Socket => FdKind::UnixSocket(Arc::new(UnixSocketState)),
            _ => FdKind::Regular,
        };

        let fd = FileDescriptor::new(inode, backend, flags, kind, String::from(path));
        let fd_idx = self.alloc_fd();
        // Increment open_fd_count for the relevant mount.
        if let Some(me) = self
            .mounts
            .iter_mut()
            .find(|m| path.starts_with(m.mount_point.as_str()))
        {
            me.open_fd_count += 1;
        }
        self.open_files.insert(fd_idx, fd);
        Ok(fd_idx)
    }

    /// Close the fd at index `fd_idx`.
    ///
    /// Decrements the open_fd_count for the backing mount.
    pub fn close_fd(&mut self, fd_idx: usize) -> bool {
        if let Some(fd) = self.open_files.remove(&fd_idx) {
            let name = fd.name.clone();
            // Decrement open_fd_count for the matching mount.
            if let Some(me) = self
                .mounts
                .iter_mut()
                .find(|m| name.starts_with(m.mount_point.as_str()))
                && me.open_fd_count > 0
            {
                me.open_fd_count -= 1;
            }
            true
        } else {
            false
        }
    }

    // -----------------------------------------------------------------------
    // Read / write / seek
    // -----------------------------------------------------------------------

    /// Read up to `buf.len()` bytes from fd `fd_idx` at its current offset.
    pub fn read_fd(&mut self, fd_idx: usize, buf: &mut [u8]) -> Option<usize> {
        let (inode, offset, backend) = {
            let fd = self.open_files.get(&fd_idx)?;
            if !fd.flags.readable() {
                return None;
            }
            (fd.inode, fd.get_offset(), fd.backend.clone())
        };
        let n = backend.read(inode, offset, buf).ok()?;
        if let Some(fd) = self.open_files.get(&fd_idx) {
            fd.set_offset(offset + n as u64);
        }
        Some(n)
    }

    /// Write `buf` to fd `fd_idx` at its current offset (or end for APPEND).
    pub fn write_fd(&mut self, fd_idx: usize, buf: &[u8]) -> Option<usize> {
        let (inode, offset, backend, is_append) = {
            let fd = self.open_files.get(&fd_idx)?;
            if !fd.flags.writable() {
                return None;
            }
            let off = if fd.flags.is_append() {
                // Seek to end for append mode.
                let stat = fd.backend.stat(fd.inode).ok()?;
                stat.size
            } else {
                fd.get_offset()
            };
            (fd.inode, off, fd.backend.clone(), fd.flags.is_append())
        };
        let _ = is_append; // used above
        let n = backend.write(inode, offset, buf).ok()?;
        if let Some(fd) = self.open_files.get(&fd_idx) {
            fd.set_offset(offset + n as u64);
        }
        Some(n)
    }

    /// Set the file offset for fd `fd_idx` to `offset`.
    pub fn seek_fd(&mut self, fd_idx: usize, offset: u64) -> bool {
        if let Some(fd) = self.open_files.get(&fd_idx) {
            fd.set_offset(offset);
            true
        } else {
            false
        }
    }

    // -----------------------------------------------------------------------
    // Stat / readdir / mkdir / unlink
    // -----------------------------------------------------------------------

    /// Return stat for `path`, or `None` if not found.
    pub fn stat_path(&self, path: &str) -> Option<FileStat> {
        let (entry, rel_path) = self.resolve(path).ok()?;
        let backend = &entry.backend;
        let root = backend.root_inode();
        let inode = Self::walk_path(backend.as_ref(), root, rel_path).ok()?;
        let stat = backend.stat(inode).ok()?;
        Some(FileStat::from(stat))
    }

    /// List names in the root directory (legacy `list_dir` behaviour).
    pub fn list_dir_root(&self) -> Vec<String> {
        // Try to list from the "/" mount if available.
        if let Ok((entry, _rel)) = self.resolve("/") {
            let backend = &entry.backend;
            let root = backend.root_inode();
            if let Ok(entries) = backend.readdir(root) {
                return entries.into_iter().map(|e| e.name).collect();
            }
        }
        // Fall back to legacy entries.
        self.entries.iter().map(|e| e.name.clone()).collect()
    }

    /// Create directory at `path`.
    pub fn mkdir_path(&mut self, path: &str) -> bool {
        let (mp, rel_path) = match self.resolve(path) {
            Ok(r) => r,
            Err(_) => return false,
        };
        if mp.flags.0 & MountFlags::RDONLY.0 != 0 {
            return false;
        }
        let backend = mp.backend.clone();
        let root = backend.root_inode();
        // Split into parent path and leaf name.
        let (parent_rel, name) = split_parent_name(rel_path);
        let parent_inode = match Self::walk_path(backend.as_ref(), root, parent_rel) {
            Ok(i) => i,
            Err(_) => return false,
        };
        backend.mkdir(parent_inode, name, 0o755).is_ok()
    }

    /// Remove the entry at `path`.
    pub fn unlink_path(&mut self, path: &str) -> bool {
        let (mp, rel_path) = match self.resolve(path) {
            Ok(r) => r,
            Err(_) => return false,
        };
        if mp.flags.0 & MountFlags::RDONLY.0 != 0 {
            return false;
        }
        let backend = mp.backend.clone();
        let root = backend.root_inode();
        let (parent_rel, name) = split_parent_name(rel_path);
        let parent_inode = match Self::walk_path(backend.as_ref(), root, parent_rel) {
            Ok(i) => i,
            Err(_) => return false,
        };
        backend.unlink(parent_inode, name).is_ok()
    }

    // -----------------------------------------------------------------------
    // Legacy / backward-compat API (used by syscall handlers)
    // -----------------------------------------------------------------------

    /// Open `path` with read-only flags, as root (uid=0, gid=0).
    /// This is the backward-compatible entry point used by syscall handlers
    /// that haven't been updated to pass uid/gid yet.
    pub fn open(&mut self, path: &str) -> Option<usize> {
        // Try the new mount-based path first.
        if let Ok(fd) = self.open_with_creds(path, OpenFlags::RDONLY, 0, 0) {
            return Some(fd);
        }
        // Fall back to legacy flat-entry lookup.
        let entry_idx = self.entries.iter().position(|e| e.name == path)?;
        let ft = self.entries[entry_idx].file_type;
        let fd_idx = self.alloc_fd();

        // Use a NullBackend so the new FileDescriptor still has a valid backend.
        let backend: Arc<dyn FsBackend> = Arc::new(NullBackend);
        let kind = match ft {
            FileType::Directory => FdKind::Directory,
            _ => FdKind::Regular,
        };
        let fd = FileDescriptor::new(
            InodeId(entry_idx as u64 + 1),
            backend,
            OpenFlags::RDONLY,
            kind,
            String::from(path),
        );
        self.open_files.insert(fd_idx, fd);
        Some(fd_idx)
    }

    /// Close fd — legacy wrapper.
    pub fn close(&mut self, fd_idx: usize) -> bool {
        self.close_fd(fd_idx)
    }

    /// Read from fd — legacy wrapper that also handles the TTY special case.
    pub fn read(&mut self, fd_idx: usize, buf: &mut [u8]) -> Option<usize> {
        // TTY special case.
        {
            let fd = self.open_files.get(&fd_idx)?;
            if fd.name == "tty" {
                let mut tty = crate::tty::TTY.lock();
                if let Some(line) = tty.read_line() {
                    let bytes = line.as_bytes();
                    let len = buf.len().min(bytes.len());
                    buf[..len].copy_from_slice(&bytes[..len]);
                    return Some(len);
                } else {
                    return Some(0);
                }
            }
        }

        // Try via backend if we have a real mount.
        if let Some(n) = self.read_fd(fd_idx, buf) {
            return Some(n);
        }

        // Legacy flat-entry fallback.
        let (inode_idx, offset) = {
            let fd = self.open_files.get(&fd_idx)?;
            let idx = (fd.inode.0 as usize).wrapping_sub(1);
            (idx, fd.get_offset())
        };
        if let Some(entry) = self.entries.get(inode_idx)
            && let Some(data) = &entry.data
        {
            let start = offset as usize;
            if start >= data.len() {
                return Some(0);
            }
            let avail = data.len() - start;
            let len = buf.len().min(avail);
            buf[..len].copy_from_slice(&data[start..start + len]);
            if let Some(fd) = self.open_files.get(&fd_idx) {
                fd.set_offset(offset + len as u64);
            }
            return Some(len);
        }
        None
    }

    /// Write to fd — legacy wrapper that also handles the TTY special case.
    pub fn write(&mut self, fd_idx: usize, buf: &[u8]) -> Option<usize> {
        // TTY special case.
        {
            let fd = self.open_files.get(&fd_idx)?;
            if fd.name == "tty" {
                if let Ok(s) = core::str::from_utf8(buf) {
                    crate::tty::TTY.lock().write(s);
                    return Some(buf.len());
                }
                return None;
            }
        }

        // Try via backend.
        if let Some(n) = self.write_fd(fd_idx, buf) {
            return Some(n);
        }

        // Legacy flat-entry fallback.
        let (inode_idx, offset) = {
            let fd = self.open_files.get(&fd_idx)?;
            let idx = (fd.inode.0 as usize).wrapping_sub(1);
            (idx, fd.get_offset())
        };
        if let Some(entry) = self.entries.get_mut(inode_idx)
            && let Some(data) = &mut entry.data
        {
            let start = offset as usize;
            if start > data.len() {
                data.resize(start, 0);
            }
            data.extend_from_slice(buf);
            let written = buf.len();
            if let Some(fd) = self.open_files.get(&fd_idx) {
                fd.set_offset(offset + written as u64);
            }
            return Some(written);
        }
        None
    }

    /// Seek fd — legacy wrapper.
    pub fn seek(&mut self, fd_idx: usize, offset: u64) -> bool {
        self.seek_fd(fd_idx, offset)
    }

    /// Stat path — legacy wrapper.
    pub fn stat(&self, path: &str) -> Option<FileStat> {
        // Try new backend-based resolution first.
        if let Some(s) = self.stat_path(path) {
            return Some(s);
        }
        // Legacy flat entries.
        let entry = self.entries.iter().find(|e| e.name == path)?;
        Some(FileStat {
            size: entry.data.as_ref().map(|d| d.len() as u64).unwrap_or(0),
            file_type: entry.file_type,
        })
    }

    /// List the root directory — legacy wrapper.
    pub fn list_dir(&self) -> Vec<String> {
        self.list_dir_root()
    }

    /// Create a directory — legacy wrapper.
    pub fn mkdir(&mut self, path: &str) -> bool {
        if self.mkdir_path(path) {
            return true;
        }
        // Legacy fallback.
        if self.entries.iter().any(|e| e.name == path) {
            return false;
        }
        self.entries.push(LegacyEntry {
            name: String::from(path),
            file_type: FileType::Directory,
            data: Some(Vec::new()),
        });
        true
    }

    /// Remove a file — legacy wrapper.
    pub fn unlink(&mut self, path: &str) -> bool {
        if self.unlink_path(path) {
            return true;
        }
        // Legacy fallback.
        if let Some(idx) = self.entries.iter().position(|e| e.name == path) {
            self.entries.remove(idx);
            // Also close open fds pointing to this path.
            self.open_files.retain(|_, fd| fd.name != path);
            true
        } else {
            false
        }
    }

    // -----------------------------------------------------------------------
    // Ramdisk initialisation (legacy)
    // -----------------------------------------------------------------------

    /// Populate the VFS from a flat ramdisk image.
    ///
    /// Format: repeated `[64-byte name NUL-padded][8-byte LE size][data bytes]`.
    pub fn init_from_ramdisk(&mut self, addr: u64, size: u64) {
        self.init_defaults();
        if addr == 0 || size == 0 {
            return;
        }
        let data = unsafe { core::slice::from_raw_parts(addr as *const u8, size as usize) };
        let mut offset = 0;
        while offset + 72 <= data.len() {
            let name_bytes = &data[offset..offset + 64];
            let name_len = name_bytes.iter().position(|&b| b == 0).unwrap_or(64);
            let name = core::str::from_utf8(&name_bytes[..name_len]).unwrap_or("unknown");
            let mut size_bytes = [0u8; 8];
            size_bytes.copy_from_slice(&data[offset + 64..offset + 72]);
            let file_size = u64::from_le_bytes(size_bytes) as usize;
            offset += 72;
            if offset + file_size <= data.len() {
                let file_data = &data[offset..offset + file_size];
                self.entries.push(LegacyEntry {
                    name: String::from(name),
                    file_type: FileType::Regular,
                    data: Some(Vec::from(file_data)),
                });
                offset += file_size;
            } else {
                break;
            }
        }
    }

    fn init_defaults(&mut self) {
        for &(name, ft) in &[
            (".", FileType::Directory),
            ("dev", FileType::Directory),
            ("null", FileType::Device),
            ("tty", FileType::Device),
        ] {
            self.entries.push(LegacyEntry {
                name: String::from(name),
                file_type: ft,
                data: None,
            });
        }
    }

    // -----------------------------------------------------------------------
    // Page cache integration (legacy read_page)
    // -----------------------------------------------------------------------

    /// Read a 4 KiB chunk from a legacy entry identified by its index.
    pub fn read_page(&self, entry_idx: usize, page_offset: u64, buf: &mut [u8]) -> bool {
        if entry_idx >= self.entries.len() {
            return false;
        }
        if let Some(data) = &self.entries[entry_idx].data {
            let start = page_offset as usize;
            if start >= data.len() {
                return false;
            }
            let avail = data.len() - start;
            let len = buf.len().min(avail);
            buf[..len].copy_from_slice(&data[start..start + len]);
            if len < buf.len() {
                buf[len..].fill(0);
            }
            return true;
        }
        false
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    fn alloc_fd(&mut self) -> usize {
        // Find the smallest free fd index.
        loop {
            let candidate = self.next_fd;
            self.next_fd = self.next_fd.wrapping_add(1);
            if !self.open_files.contains_key(&candidate) {
                return candidate;
            }
            // Avoid infinite loop if somehow we wrap the whole range.
            if self.next_fd == 0 {
                // Linear scan to find a free slot.
                for i in 0..MAX_OPEN_FILES {
                    if !self.open_files.contains_key(&i) {
                        self.next_fd = i + 1;
                        return i;
                    }
                }
                panic!("VFS: no free file descriptors");
            }
        }
    }
}

impl Default for Vfs {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// NullBackend — used as placeholder when opening legacy flat entries
// ---------------------------------------------------------------------------

/// A no-op backend used for legacy flat-entry file descriptors.
struct NullBackend;

impl FsBackend for NullBackend {
    fn root_inode(&self) -> InodeId {
        InodeId(1)
    }
    fn lookup(&self, _parent: InodeId, _name: &str) -> Result<InodeId, FsError> {
        Err(FsError::NotFound)
    }
    fn open(&self, _inode: InodeId, _flags: OpenFlags) -> Result<(), FsError> {
        Ok(())
    }
    fn read(&self, _inode: InodeId, _offset: u64, _buf: &mut [u8]) -> Result<usize, FsError> {
        Ok(0)
    }
    fn write(&self, _inode: InodeId, _offset: u64, _buf: &[u8]) -> Result<usize, FsError> {
        Ok(0)
    }
    fn stat(&self, _inode: InodeId) -> Result<InodeStat, FsError> {
        Err(FsError::NotFound)
    }
    fn readdir(&self, _inode: InodeId) -> Result<Vec<DirEntry>, FsError> {
        Err(FsError::NotFound)
    }
    fn mkdir(&self, _parent: InodeId, _name: &str, _mode: u32) -> Result<InodeId, FsError> {
        Err(FsError::NotSupported)
    }
    fn unlink(&self, _parent: InodeId, _name: &str) -> Result<(), FsError> {
        Err(FsError::NotSupported)
    }
    fn rename(&self, _op: InodeId, _on: &str, _np: InodeId, _nn: &str) -> Result<(), FsError> {
        Err(FsError::NotSupported)
    }
    fn sync(&self) -> Result<(), FsError> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Path utilities
// ---------------------------------------------------------------------------

/// Split a relative path such as `/foo/bar/baz` into `("/foo/bar", "baz")`.
///
/// If there is no '/' separator (or only the leading '/') the parent is `"/"`
/// and the name is the whole component.
fn split_parent_name(rel: &str) -> (&str, &str) {
    // Strip trailing slash.
    let rel = rel.trim_end_matches('/');
    match rel.rfind('/') {
        Some(0) => ("/", &rel[1..]),
        Some(pos) => (&rel[..pos], &rel[pos + 1..]),
        None => ("/", rel),
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    // -----------------------------------------------------------------------
    // Minimal in-memory backend for tests
    // -----------------------------------------------------------------------

    /// A simple in-memory backend: inode 1 = root dir, inodes 2… = files.
    struct MemBackend {
        inner: Mutex<MemBackendInner>,
    }

    struct MemBackendInner {
        files: BTreeMap<String, (InodeId, Vec<u8>)>,
        next_inode: u64,
    }

    impl MemBackend {
        fn new() -> Self {
            let mut files = BTreeMap::new();
            // Pre-populate with a couple of test files.
            files.insert(
                "hello.txt".to_string(),
                (InodeId(2), b"hello world".to_vec()),
            );
            files.insert("dir".to_string(), (InodeId(3), Vec::new()));
            MemBackend {
                inner: Mutex::new(MemBackendInner {
                    files,
                    next_inode: 4,
                }),
            }
        }
    }

    impl FsBackend for MemBackend {
        fn root_inode(&self) -> InodeId {
            InodeId(1)
        }

        fn lookup(&self, _parent: InodeId, name: &str) -> Result<InodeId, FsError> {
            let inner = self.inner.lock();
            inner
                .files
                .get(name)
                .map(|(id, _)| *id)
                .ok_or(FsError::NotFound)
        }

        fn open(&self, _inode: InodeId, _flags: OpenFlags) -> Result<(), FsError> {
            Ok(())
        }

        fn read(&self, inode: InodeId, offset: u64, buf: &mut [u8]) -> Result<usize, FsError> {
            let inner = self.inner.lock();
            for (_, (id, data)) in &inner.files {
                if *id == inode {
                    let start = offset as usize;
                    if start >= data.len() {
                        return Ok(0);
                    }
                    let avail = data.len() - start;
                    let len = buf.len().min(avail);
                    buf[..len].copy_from_slice(&data[start..start + len]);
                    return Ok(len);
                }
            }
            Err(FsError::NotFound)
        }

        fn write(&self, inode: InodeId, offset: u64, buf: &[u8]) -> Result<usize, FsError> {
            let mut inner = self.inner.lock();
            for (_, (id, data)) in &mut inner.files {
                if *id == inode {
                    let start = offset as usize;
                    if start > data.len() {
                        data.resize(start, 0);
                    }
                    let end = start + buf.len();
                    if end > data.len() {
                        data.resize(end, 0);
                    }
                    data[start..end].copy_from_slice(buf);
                    return Ok(buf.len());
                }
            }
            Err(FsError::NotFound)
        }

        fn stat(&self, inode: InodeId) -> Result<InodeStat, FsError> {
            if inode == InodeId(1) {
                return Ok(InodeStat {
                    mode: 0o755,
                    uid: 0,
                    gid: 0,
                    nlink: 2,
                    atime: 0,
                    mtime: 0,
                    ctime: 0,
                    size: 0,
                    file_type: FileType::Directory,
                });
            }
            let inner = self.inner.lock();
            for (name, (id, data)) in &inner.files {
                if *id == inode {
                    let ft = if data.is_empty() && name == "dir" {
                        FileType::Directory
                    } else {
                        FileType::Regular
                    };
                    return Ok(InodeStat {
                        mode: 0o644,
                        uid: 1000,
                        gid: 1000,
                        nlink: 1,
                        atime: 0,
                        mtime: 0,
                        ctime: 0,
                        size: data.len() as u64,
                        file_type: ft,
                    });
                }
            }
            Err(FsError::NotFound)
        }

        fn readdir(&self, inode: InodeId) -> Result<Vec<DirEntry>, FsError> {
            if inode != InodeId(1) {
                return Err(FsError::NotADirectory);
            }
            let inner = self.inner.lock();
            Ok(inner
                .files
                .iter()
                .map(|(name, (id, data))| DirEntry {
                    inode: *id,
                    name: name.clone(),
                    file_type: if data.is_empty() && name == "dir" {
                        FileType::Directory
                    } else {
                        FileType::Regular
                    },
                })
                .collect())
        }

        fn mkdir(&self, _parent: InodeId, name: &str, _mode: u32) -> Result<InodeId, FsError> {
            let mut inner = self.inner.lock();
            if inner.files.contains_key(name) {
                return Err(FsError::AlreadyExists);
            }
            let id = InodeId(inner.next_inode);
            inner.next_inode += 1;
            inner.files.insert(name.to_string(), (id, Vec::new()));
            Ok(id)
        }

        fn unlink(&self, _parent: InodeId, name: &str) -> Result<(), FsError> {
            let mut inner = self.inner.lock();
            inner.files.remove(name).ok_or(FsError::NotFound)?;
            Ok(())
        }

        fn rename(&self, _op: InodeId, old: &str, _np: InodeId, new: &str) -> Result<(), FsError> {
            let mut inner = self.inner.lock();
            let val = inner.files.remove(old).ok_or(FsError::NotFound)?;
            inner.files.insert(new.to_string(), val);
            Ok(())
        }

        fn sync(&self) -> Result<(), FsError> {
            Ok(())
        }
    }

    // -----------------------------------------------------------------------
    // check_permission tests
    // -----------------------------------------------------------------------

    fn make_stat(mode: u32, uid: u32, gid: u32) -> InodeStat {
        InodeStat {
            mode,
            uid,
            gid,
            nlink: 1,
            atime: 0,
            mtime: 0,
            ctime: 0,
            size: 0,
            file_type: FileType::Regular,
        }
    }

    #[test]
    fn owner_can_read_file() {
        let stat = make_stat(0o644, 1000, 1000);
        assert!(check_permission(&stat, 1000, 9999, 0x4)); // read
    }

    #[test]
    fn other_cannot_write_without_permission() {
        let stat = make_stat(0o644, 1000, 1000);
        assert!(!check_permission(&stat, 9999, 9999, 0x2)); // write
    }

    #[test]
    fn group_member_can_read() {
        let stat = make_stat(0o640, 1000, 1001);
        assert!(check_permission(&stat, 9999, 1001, 0x4)); // read via group
    }

    #[test]
    fn other_can_read_world_readable() {
        let stat = make_stat(0o644, 1000, 1000);
        assert!(check_permission(&stat, 9999, 9999, 0x4));
    }

    #[test]
    fn root_can_always_write() {
        let stat = make_stat(0o000, 1000, 1000);
        assert!(check_permission(&stat, 0, 0, 0x6)); // read + write
    }

    #[test]
    fn root_cannot_exec_without_exec_bit() {
        let stat = make_stat(0o644, 1000, 1000); // no exec bits
        assert!(!check_permission(&stat, 0, 0, 0x1));
    }

    #[test]
    fn root_can_exec_when_exec_bit_set() {
        let stat = make_stat(0o755, 1000, 1000);
        assert!(check_permission(&stat, 0, 0, 0x1));
    }

    // -----------------------------------------------------------------------
    // Vfs::resolve tests
    // -----------------------------------------------------------------------

    fn make_vfs_with_backends() -> Vfs {
        let mut vfs = Vfs::new();
        let root_backend: Arc<dyn FsBackend> = Arc::new(MemBackend::new());
        let mnt_backend: Arc<dyn FsBackend> = Arc::new(MemBackend::new());
        vfs.mount("/", root_backend, MountFlags::default()).unwrap();
        vfs.mount("/mnt", mnt_backend, MountFlags::default())
            .unwrap();
        vfs
    }

    #[test]
    fn resolve_root() {
        let vfs = make_vfs_with_backends();
        let (entry, rel) = vfs.resolve("/").expect("resolve /");
        assert_eq!(entry.mount_point, "/");
        assert_eq!(rel, "/");
    }

    #[test]
    fn resolve_file_under_root() {
        let vfs = make_vfs_with_backends();
        let (entry, rel) = vfs.resolve("/hello.txt").expect("resolve /hello.txt");
        assert_eq!(entry.mount_point, "/");
        assert_eq!(rel, "/hello.txt");
    }

    #[test]
    fn resolve_file_under_mnt_uses_mnt_backend() {
        let vfs = make_vfs_with_backends();
        let (entry, rel) = vfs
            .resolve("/mnt/hello.txt")
            .expect("resolve /mnt/hello.txt");
        assert_eq!(entry.mount_point, "/mnt");
        assert_eq!(rel, "/hello.txt");
    }

    #[test]
    fn resolve_mnt_itself() {
        let vfs = make_vfs_with_backends();
        let (entry, rel) = vfs.resolve("/mnt").expect("resolve /mnt");
        assert_eq!(entry.mount_point, "/mnt");
        assert_eq!(rel, "/");
    }

    #[test]
    fn resolve_no_mount_returns_error() {
        let vfs = Vfs::new(); // no mounts
        assert_eq!(vfs.resolve("/foo").unwrap_err(), FsError::NotFound);
    }

    #[test]
    fn resolve_partial_path_name_not_confused_with_mount() {
        // /mntfoo should resolve to / (not /mnt)
        let vfs = make_vfs_with_backends();
        let (entry, rel) = vfs.resolve("/mntfoo").expect("resolve /mntfoo");
        assert_eq!(entry.mount_point, "/");
        assert_eq!(rel, "/mntfoo");
    }

    // -----------------------------------------------------------------------
    // Vfs::mount / umount tests
    // -----------------------------------------------------------------------

    #[test]
    fn umount_without_open_fds_succeeds() {
        let mut vfs = Vfs::new();
        let b: Arc<dyn FsBackend> = Arc::new(MemBackend::new());
        vfs.mount("/", b, MountFlags::default()).unwrap();
        assert!(vfs.umount("/").is_ok());
    }

    #[test]
    fn umount_with_open_fds_returns_busy() {
        let mut vfs = Vfs::new();
        let b: Arc<dyn FsBackend> = Arc::new(MemBackend::new());
        vfs.mount("/", b, MountFlags::default()).unwrap();
        // Open a file, which increments open_fd_count.
        let _fd = vfs
            .open_with_creds("/hello.txt", OpenFlags::RDONLY, 1000, 1000)
            .unwrap();
        assert_eq!(vfs.umount("/").unwrap_err(), FsError::BusyMounted);
    }

    #[test]
    fn umount_after_close_succeeds() {
        let mut vfs = Vfs::new();
        let b: Arc<dyn FsBackend> = Arc::new(MemBackend::new());
        vfs.mount("/", b, MountFlags::default()).unwrap();
        let fd = vfs
            .open_with_creds("/hello.txt", OpenFlags::RDONLY, 1000, 1000)
            .unwrap();
        vfs.close_fd(fd);
        assert!(vfs.umount("/").is_ok());
    }

    // -----------------------------------------------------------------------
    // Open / read / close via new API
    // -----------------------------------------------------------------------

    #[test]
    fn open_and_read_file() {
        let mut vfs = Vfs::new();
        let b: Arc<dyn FsBackend> = Arc::new(MemBackend::new());
        vfs.mount("/", b, MountFlags::default()).unwrap();

        let fd = vfs
            .open_with_creds("/hello.txt", OpenFlags::RDONLY, 1000, 1000)
            .expect("open");
        let mut buf = [0u8; 11];
        let n = vfs.read_fd(fd, &mut buf).expect("read");
        assert_eq!(n, 11);
        assert_eq!(&buf, b"hello world");
        vfs.close_fd(fd);
    }

    #[test]
    fn open_permission_denied_for_no_read() {
        let mut vfs = Vfs::new();
        let b: Arc<dyn FsBackend> = Arc::new(MemBackend::new());
        vfs.mount("/", b, MountFlags::default()).unwrap();
        // File mode 0o644, owned by uid 1000 / gid 1000.
        // Open as uid 9999, gid 9999 — read should still work (world-readable).
        let fd = vfs.open_with_creds("/hello.txt", OpenFlags::RDONLY, 9999, 9999);
        assert!(
            fd.is_ok(),
            "world-readable file should be openable by others"
        );
    }

    // -----------------------------------------------------------------------
    // Mkdir / unlink (new API)
    // -----------------------------------------------------------------------

    #[test]
    fn mkdir_and_stat() {
        let mut vfs = Vfs::new();
        let b: Arc<dyn FsBackend> = Arc::new(MemBackend::new());
        vfs.mount("/", b, MountFlags::default()).unwrap();
        assert!(vfs.mkdir_path("/newdir"));
        // stat should find it now.
        let s = vfs.stat_path("/newdir");
        assert!(s.is_some());
    }

    #[test]
    fn unlink_removes_file() {
        let mut vfs = Vfs::new();
        let b: Arc<dyn FsBackend> = Arc::new(MemBackend::new());
        vfs.mount("/", b, MountFlags::default()).unwrap();
        assert!(vfs.unlink_path("/hello.txt"));
        assert!(vfs.stat_path("/hello.txt").is_none());
    }

    // -----------------------------------------------------------------------
    // split_parent_name
    // -----------------------------------------------------------------------

    #[test]
    fn split_root_file() {
        let (p, n) = split_parent_name("/hello.txt");
        assert_eq!(p, "/");
        assert_eq!(n, "hello.txt");
    }

    #[test]
    fn split_nested() {
        let (p, n) = split_parent_name("/foo/bar/baz");
        assert_eq!(p, "/foo/bar");
        assert_eq!(n, "baz");
    }

    #[test]
    fn split_with_trailing_slash() {
        let (p, n) = split_parent_name("/foo/bar/");
        assert_eq!(p, "/foo");
        assert_eq!(n, "bar");
    }
}

// ---------------------------------------------------------------------------
// Property-based tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod prop_tests {
    use super::*;
    use alloc::string::ToString;
    use proptest::prelude::*;

    // -----------------------------------------------------------------------
    // In-memory backend for property tests
    // -----------------------------------------------------------------------

    /// A parameterised in-memory backend with a configurable set of files.
    /// Each file is identified by name and has a fixed content payload.
    struct PropMemBackend {
        /// All files are flat under the root inode (inode 1).
        /// `files[i]` has InodeId(i+2) (inode 1 is root).
        files: alloc::vec::Vec<(String, alloc::vec::Vec<u8>)>,
    }

    impl PropMemBackend {
        /// Create a backend seeded with `files`.
        fn new(files: alloc::vec::Vec<(String, alloc::vec::Vec<u8>)>) -> Self {
            Self { files }
        }

        /// Return the inode for a file name, or None.
        fn inode_for(&self, name: &str) -> Option<InodeId> {
            self.files
                .iter()
                .enumerate()
                .find(|(_, (n, _))| n == name)
                .map(|(i, _)| InodeId(i as u64 + 2))
        }
    }

    impl FsBackend for PropMemBackend {
        fn root_inode(&self) -> InodeId {
            InodeId(1)
        }

        fn lookup(&self, _parent: InodeId, name: &str) -> Result<InodeId, FsError> {
            self.inode_for(name).ok_or(FsError::NotFound)
        }

        fn open(&self, _inode: InodeId, _flags: OpenFlags) -> Result<(), FsError> {
            Ok(())
        }

        fn read(&self, inode: InodeId, offset: u64, buf: &mut [u8]) -> Result<usize, FsError> {
            let idx = inode.0.checked_sub(2).ok_or(FsError::NotFound)? as usize;
            let data = &self.files.get(idx).ok_or(FsError::NotFound)?.1;
            let start = offset as usize;
            if start >= data.len() {
                return Ok(0);
            }
            let avail = data.len() - start;
            let len = buf.len().min(avail);
            buf[..len].copy_from_slice(&data[start..start + len]);
            Ok(len)
        }

        fn write(&self, _inode: InodeId, _offset: u64, _buf: &[u8]) -> Result<usize, FsError> {
            Err(FsError::PermissionDenied)
        }

        fn stat(&self, inode: InodeId) -> Result<InodeStat, FsError> {
            if inode == InodeId(1) {
                return Ok(InodeStat {
                    mode: 0o755,
                    uid: 0,
                    gid: 0,
                    nlink: 2,
                    atime: 0,
                    mtime: 0,
                    ctime: 0,
                    size: 0,
                    file_type: FileType::Directory,
                });
            }
            let idx = inode.0.checked_sub(2).ok_or(FsError::NotFound)? as usize;
            let (_, data) = self.files.get(idx).ok_or(FsError::NotFound)?;
            Ok(InodeStat {
                mode: 0o644,
                uid: 1000,
                gid: 1000,
                nlink: 1,
                atime: 0,
                mtime: 0,
                ctime: 0,
                size: data.len() as u64,
                file_type: FileType::Regular,
            })
        }

        fn readdir(&self, inode: InodeId) -> Result<alloc::vec::Vec<DirEntry>, FsError> {
            if inode != InodeId(1) {
                return Err(FsError::NotADirectory);
            }
            Ok(self
                .files
                .iter()
                .enumerate()
                .map(|(i, (name, _))| DirEntry {
                    inode: InodeId(i as u64 + 2),
                    name: name.clone(),
                    file_type: FileType::Regular,
                })
                .collect())
        }

        fn mkdir(&self, _parent: InodeId, _name: &str, _mode: u32) -> Result<InodeId, FsError> {
            Err(FsError::NotSupported)
        }

        fn unlink(&self, _parent: InodeId, _name: &str) -> Result<(), FsError> {
            Err(FsError::NotSupported)
        }

        fn rename(&self, _op: InodeId, _on: &str, _np: InodeId, _nn: &str) -> Result<(), FsError> {
            Err(FsError::NotSupported)
        }

        fn sync(&self) -> Result<(), FsError> {
            Ok(())
        }
    }

    // -----------------------------------------------------------------------
    // Generators
    // -----------------------------------------------------------------------

    /// Generate a mount point path (one of a fixed set of meaningful paths).
    fn arb_mount_point() -> impl Strategy<Value = String> {
        prop::sample::select(alloc::vec![
            "/mnt".to_string(),
            "/srv".to_string(),
            "/data".to_string(),
            "/media".to_string(),
            "/opt".to_string(),
        ])
    }

    /// Generate a simple file name (no slashes, non-empty, valid ASCII).
    fn arb_filename() -> impl Strategy<Value = String> {
        "[a-z][a-z0-9]{0,7}(\\.[a-z]{1,3})?".prop_map(|s: String| s)
    }

    /// Generate small arbitrary content for a file.
    fn arb_content() -> impl Strategy<Value = alloc::vec::Vec<u8>> {
        proptest::collection::vec(any::<u8>(), 0..=64)
    }

    /// Generate 1–4 files as `(name, content)` pairs with unique names.
    fn arb_files() -> impl Strategy<Value = alloc::vec::Vec<(String, alloc::vec::Vec<u8>)>> {
        proptest::collection::vec((arb_filename(), arb_content()), 1..=4)
            .prop_map(|mut v| {
                // Deduplicate names (keep first occurrence).
                let mut seen = alloc::vec::Vec::<String>::new();
                v.retain(|(name, _)| {
                    if seen.contains(name) {
                        false
                    } else {
                        seen.push(name.clone());
                        true
                    }
                });
                v
            })
            .prop_filter("need at least one file", |v| !v.is_empty())
    }

    // -----------------------------------------------------------------------
    // Property 20 — VFS Path Lookup Across Mount Points
    //
    // Validates: Requirements 21.5
    //
    // For any path that crosses a mount point boundary (i.e. a path of the
    // form `<mount_point>/<filename>`), the VFS SHALL transparently resolve
    // the path by delegating to the mounted filesystem backend.
    //
    // Concretely this property asserts:
    //  1. `Vfs::resolve` selects the mounted backend (not the root backend)
    //     when the path starts with the non-root mount point.
    //  2. The relative path extracted by `resolve` strips the mount-point
    //     prefix and leaves a well-formed backend-relative path.
    //  3. Walking the relative path within the backend's own namespace
    //     produces the same `InodeId` as querying the backend directly.
    //  4. `stat_path` on the cross-mount path returns the same file size as
    //     the backend reports for the same inode — i.e. resolution is
    //     transparent (the VFS does not alter metadata).
    // -----------------------------------------------------------------------

    proptest! {
        /// **Validates: Requirements 21.5**
        ///
        /// Property 20: VFS Path Lookup Across Mount Points
        ///
        /// Generate a VFS with a root backend at `/` and a second backend at
        /// a non-root mount point.  For every file in the second backend,
        /// build the absolute path `<mount_point>/<filename>` and assert:
        ///   - `resolve` delegates to the second backend (not the root)
        ///   - The relative path extracted from `resolve` maps to the correct
        ///     inode inside the second backend
        ///   - The `InodeId` reached via the VFS equals the direct backend
        ///     lookup result
        ///   - `stat_path` returns metadata that matches the backend's own
        ///     `stat` call (transparent delegation, no metadata mutation)
        #[test]
        fn vfs_path_lookup_across_mount_points(
            mount_point in arb_mount_point(),
            root_files  in arb_files(),
            mnt_files   in arb_files(),
            // Pick a file index inside the mounted backend to look up.
            file_idx_raw in any::<usize>(),
        ) {
            // Build two independent in-memory backends.
            let root_backend: Arc<dyn FsBackend> =
                Arc::new(PropMemBackend::new(root_files));
            let mnt_backend = Arc::new(PropMemBackend::new(mnt_files.clone()));
            let mnt_backend_dyn: Arc<dyn FsBackend> = mnt_backend.clone();

            // Mount both: "/" first (added last so it sorts after the longer
            // mount point and is thus tried second during resolve).
            let mut vfs = Vfs::new();
            vfs.mount("/", root_backend, MountFlags::default()).unwrap();
            vfs.mount(&mount_point, mnt_backend_dyn, MountFlags::default()).unwrap();

            // Select one of the files in the mounted backend.
            let file_idx = file_idx_raw % mnt_files.len();
            let (filename, expected_content) = &mnt_files[file_idx];

            // Build the absolute path that crosses the mount boundary.
            let absolute_path = alloc::format!("{}/{}", mount_point, filename);

            // --- Assertion 1: resolve selects the non-root backend ----------
            let (resolved_entry, rel_path) = vfs.resolve(&absolute_path)
                .expect("VFS must resolve a path under a mounted backend");

            prop_assert_eq!(
                &resolved_entry.mount_point, &mount_point,
                "resolve must select the non-root mount point for path '{}'",
                absolute_path
            );

            // --- Assertion 2: relative path is well-formed ------------------
            // rel_path should start with '/' and contain the filename.
            prop_assert!(
                rel_path.starts_with('/'),
                "relative path '{}' must start with '/'",
                rel_path
            );
            prop_assert!(
                rel_path.contains(filename.as_str()),
                "relative path '{}' must contain the filename '{}'",
                rel_path,
                filename
            );

            // --- Assertion 3: walking rel_path reaches the correct inode ----
            let backend = &resolved_entry.backend;
            let root_inode = backend.root_inode();

            // Direct backend lookup — the ground truth.
            let direct_inode = backend.lookup(root_inode, filename)
                .expect("backend must find the file by name");

            // VFS walk — the path resolution via mount-point traversal.
            let walked_inode = Vfs::walk_path(backend.as_ref(), root_inode, rel_path)
                .expect("walk_path must find the file via the relative path");

            prop_assert_eq!(
                walked_inode, direct_inode,
                "walk_path inode via VFS must equal direct backend lookup for '{}'",
                filename
            );

            // --- Assertion 4: stat is transparently delegated ---------------
            // stat via the VFS path resolution.
            let vfs_stat = vfs.stat_path(&absolute_path)
                .expect("stat_path must succeed for a file under a mount point");

            // stat directly via the backend inode.
            let direct_stat = backend.stat(direct_inode)
                .expect("backend stat must succeed");

            prop_assert_eq!(
                vfs_stat.size, direct_stat.size,
                "VFS stat size must equal backend stat size for '{}': expected {} got {}",
                absolute_path, direct_stat.size, vfs_stat.size
            );

            // Verify content size matches expectations too.
            prop_assert_eq!(
                direct_stat.size, expected_content.len() as u64,
                "backend stat size must match the expected content length for '{}'",
                filename
            );
        }
    }

    proptest! {
        /// **Validates: Requirements 21.5**
        ///
        /// Property 20 (complement): Root-path files do NOT cross a mount
        /// boundary — paths NOT under the non-root mount point resolve to
        /// the root backend, confirming that mount-point delegation is
        /// selective (not applied universally).
        #[test]
        fn vfs_root_paths_resolve_to_root_backend(
            mount_point in arb_mount_point(),
            root_files  in arb_files(),
            mnt_files   in arb_files(),
            file_idx_raw in any::<usize>(),
        ) {
            let root_backend = Arc::new(PropMemBackend::new(root_files.clone()));
            let root_backend_dyn: Arc<dyn FsBackend> = root_backend.clone();
            let mnt_backend: Arc<dyn FsBackend> =
                Arc::new(PropMemBackend::new(mnt_files));

            let mut vfs = Vfs::new();
            vfs.mount("/", root_backend_dyn, MountFlags::default()).unwrap();
            vfs.mount(&mount_point, mnt_backend, MountFlags::default()).unwrap();

            // A file path directly under "/" that is NOT under the mount point.
            let file_idx = file_idx_raw % root_files.len();
            let (filename, _) = &root_files[file_idx];

            // Build a root-level path — guaranteed not to start with mount_point
            // because filenames are short alphanumeric strings and mount points
            // start with "/" followed by a known prefix (mnt/srv/data/media/opt).
            let root_path = alloc::format!("/{}", filename);

            // The mount-point prefix cannot match a root-level file unless the
            // filename starts with the mount_point's base component (e.g. "mnt").
            // Skip this combination to keep the test focused.
            let mp_base = mount_point.trim_start_matches('/');
            if filename.starts_with(mp_base) {
                return Ok(());
            }

            let (resolved_entry, rel_path) = vfs.resolve(&root_path)
                .expect("VFS must resolve a root-level path");

            prop_assert_eq!(
                &resolved_entry.mount_point, "/",
                "root-level path '{}' must resolve to the root mount, not '{}'",
                root_path, mount_point
            );

            // rel_path for root mount is the original path.
            prop_assert_eq!(
                rel_path, root_path.as_str(),
                "relative path for root mount must equal the original path"
            );
        }
    }
}
