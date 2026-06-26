//! HugeTLB Filesystem Interface
//!
//! Provides mount/unmount and allocation primitives for HugeTLB-backed
//! file systems. Each mount represents a pool of huge pages (2 MiB or
//! 1 GiB) that can be allocated and freed through a standard file
//! descriptor interface.

use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

/// A single HugeTLB mount point.
#[derive(Debug, Clone)]
pub struct HugetlbMount {
    pub mount_point: String,
    pub fs_type: String,
    pub page_size: usize,
    pub allocated: u64,
    pub capacity: u64,
}

/// Pool of all active HugeTLB mounts.
#[derive(Debug)]
pub struct HugetlbPool {
    pub mounts: Vec<HugetlbMount>,
    pub next_id: u32,
}

impl HugetlbPool {
    const fn new() -> Self {
        HugetlbPool {
            mounts: Vec::new(),
            next_id: 0,
        }
    }

    fn find_mount(&self, mount_point: &str) -> Option<usize> {
        self.mounts
            .iter()
            .position(|m| m.mount_point == mount_point)
    }
}

/// Errors that can occur during HugeTLB operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HugetlbError {
    NotFound,
    AlreadyMounted,
    OutOfMemory,
    InvalidPageSize,
}

static HUGETLB_POOL: Mutex<Option<HugetlbPool>> = Mutex::new(None);

/// Initialise the HugeTLB subsystem.
pub fn init_hugetlb() {
    let mut guard = HUGETLB_POOL.lock();
    if guard.is_none() {
        *guard = Some(HugetlbPool::new());
    }
}

/// Reset the HugeTLB subsystem, removing all mounts.
pub fn reset_hugetlb() {
    let mut guard = HUGETLB_POOL.lock();
    *guard = Some(HugetlbPool::new());
}

/// Mount a HugeTLB filesystem at the given mount point.
///
/// `page_size` must be 2 MiB (2097152) or 1 GiB (1073741824).
/// Returns the mount ID on success.
pub fn hugetlb_mount(mount_point: &str, page_size: usize) -> Result<u32, HugetlbError> {
    if page_size != 2 * 1024 * 1024 && page_size != 1024 * 1024 * 1024 {
        return Err(HugetlbError::InvalidPageSize);
    }

    let mut guard = HUGETLB_POOL.lock();
    let pool = guard.as_mut().ok_or(HugetlbError::NotFound)?;

    if pool.find_mount(mount_point).is_some() {
        return Err(HugetlbError::AlreadyMounted);
    }

    let id = pool.next_id;
    pool.next_id += 1;

    let capacity = if page_size == 2 * 1024 * 1024 { 64 } else { 4 };

    pool.mounts.push(HugetlbMount {
        mount_point: String::from(mount_point),
        fs_type: String::from("hugetlbfs"),
        page_size,
        allocated: 0,
        capacity,
    });

    Ok(id)
}

/// Unmount a HugeTLB filesystem. Returns `true` if it was found and removed.
pub fn hugetlb_unmount(mount_point: &str) -> bool {
    let mut guard = HUGETLB_POOL.lock();
    let pool = match guard.as_mut() {
        Some(p) => p,
        None => return false,
    };

    if let Some(idx) = pool.find_mount(mount_point) {
        pool.mounts.swap_remove(idx);
        true
    } else {
        false
    }
}

/// Allocate huge pages from a mount. Returns the number of pages allocated.
pub fn hugetlb_alloc(mount_point: &str, count: u64) -> Result<u64, HugetlbError> {
    let mut guard = HUGETLB_POOL.lock();
    let pool = guard.as_mut().ok_or(HugetlbError::NotFound)?;

    let idx = pool.find_mount(mount_point).ok_or(HugetlbError::NotFound)?;
    let mount = &mut pool.mounts[idx];

    let available = mount.capacity.saturating_sub(mount.allocated);
    let to_alloc = if count <= available { count } else { available };

    if to_alloc == 0 {
        return Err(HugetlbError::OutOfMemory);
    }

    mount.allocated += to_alloc;
    Ok(to_alloc)
}

/// Free huge pages back to a mount. Returns `true` on success.
pub fn hugetlb_free(mount_point: &str, count: u64) -> bool {
    let mut guard = HUGETLB_POOL.lock();
    let pool = match guard.as_mut() {
        Some(p) => p,
        None => return false,
    };

    let idx = match pool.find_mount(mount_point) {
        Some(i) => i,
        None => return false,
    };

    let mount = &mut pool.mounts[idx];
    let to_free = if count <= mount.allocated {
        count
    } else {
        mount.allocated
    };
    mount.allocated -= to_free;
    true
}

/// Return `(allocated, capacity)` for the given mount point.
pub fn hugetlb_stat(mount_point: &str) -> Option<(u64, u64)> {
    let guard = HUGETLB_POOL.lock();
    let pool = guard.as_ref()?;
    let idx = pool.find_mount(mount_point)?;
    let mount = &pool.mounts[idx];
    Some((mount.allocated, mount.capacity))
}

/// List all mounts as `(mount_point, page_size, allocated, capacity)`.
pub fn list_hugetlb_mounts() -> Vec<(String, usize, u64, u64)> {
    let guard = HUGETLB_POOL.lock();
    let pool = match guard.as_ref() {
        Some(p) => p,
        None => return Vec::new(),
    };

    pool.mounts
        .iter()
        .map(|m| {
            (
                m.mount_point.clone(),
                m.page_size,
                m.allocated,
                m.capacity,
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hugetlb_error_equality() {
        let _guard = crate::test_serial::acquire();
        assert_eq!(HugetlbError::NotFound, HugetlbError::NotFound);
        assert_ne!(HugetlbError::NotFound, HugetlbError::AlreadyMounted);
        assert_ne!(HugetlbError::OutOfMemory, HugetlbError::InvalidPageSize);
    }

    #[test]
    fn test_init_and_reset() {
        let _guard = crate::test_serial::acquire();
        init_hugetlb();
        assert!(HUGETLB_POOL.lock().is_some());
        reset_hugetlb();
        let pool = HUGETLB_POOL.lock();
        assert_eq!(pool.as_ref().unwrap().mounts.len(), 0);
    }

    #[test]
    fn test_mount_valid_page_size() {
        let _guard = crate::test_serial::acquire();
        reset_hugetlb();
        let id = hugetlb_mount("/dev/hugepages", 2 * 1024 * 1024);
        assert_eq!(id, Ok(0));
        let id2 = hugetlb_mount("/dev/hugepages1g", 1024 * 1024 * 1024);
        assert_eq!(id2, Ok(1));
    }

    #[test]
    fn test_mount_invalid_page_size() {
        let _guard = crate::test_serial::acquire();
        reset_hugetlb();
        let result = hugetlb_mount("/mnt", 4096);
        assert_eq!(result, Err(HugetlbError::InvalidPageSize));
    }

    #[test]
    fn test_mount_duplicate() {
        let _guard = crate::test_serial::acquire();
        reset_hugetlb();
        let _ = hugetlb_mount("/dev/hugepages", 2 * 1024 * 1024);
        let result = hugetlb_mount("/dev/hugepages", 2 * 1024 * 1024);
        assert_eq!(result, Err(HugetlbError::AlreadyMounted));
    }

    #[test]
    fn test_unmount() {
        let _guard = crate::test_serial::acquire();
        reset_hugetlb();
        let _ = hugetlb_mount("/dev/hugepages", 2 * 1024 * 1024);
        assert!(hugetlb_unmount("/dev/hugepages"));
        assert!(!hugetlb_unmount("/dev/hugepages"));
    }

    #[test]
    fn test_alloc_and_free() {
        let _guard = crate::test_serial::acquire();
        reset_hugetlb();
        let _ = hugetlb_mount("/dev/hugepages", 2 * 1024 * 1024);
        let allocated = hugetlb_alloc("/dev/hugepages", 10).unwrap();
        assert_eq!(allocated, 10);
        let stat = hugetlb_stat("/dev/hugepages").unwrap();
        assert_eq!(stat, (10, 64));
        assert!(hugetlb_free("/dev/hugepages", 5));
        let stat = hugetlb_stat("/dev/hugepages").unwrap();
        assert_eq!(stat, (5, 64));
    }

    #[test]
    fn test_alloc_exceeds_capacity() {
        let _guard = crate::test_serial::acquire();
        reset_hugetlb();
        let _ = hugetlb_mount("/dev/hugepages", 2 * 1024 * 1024);
        let allocated = hugetlb_alloc("/dev/hugepages", 100).unwrap();
        assert_eq!(allocated, 64, "should only allocate up to capacity");
        let result = hugetlb_alloc("/dev/hugepages", 1);
        assert_eq!(result, Err(HugetlbError::OutOfMemory));
    }

    #[test]
    fn test_list_mounts() {
        let _guard = crate::test_serial::acquire();
        reset_hugetlb();
        let _ = hugetlb_mount("/a", 2 * 1024 * 1024);
        let _ = hugetlb_mount("/b", 1024 * 1024 * 1024);
        let mounts = list_hugetlb_mounts();
        assert_eq!(mounts.len(), 2);
        assert_eq!(mounts[0].0, "/a");
        assert_eq!(mounts[1].1, 1024 * 1024 * 1024);
    }

    #[test]
    fn test_stat_not_found() {
        let _guard = crate::test_serial::acquire();
        reset_hugetlb();
        assert!(hugetlb_stat("/nonexistent").is_none());
    }
}
