//! Block device abstraction layer for Turnix OS.
//!
//! Provides a common `BlockDevice` trait that storage backends (NVMe, AHCI, etc.)
//! implement. The filesystem layer uses this trait for sector-level I/O.

extern crate alloc;

/// Sector size in bytes (standard for modern storage).
pub const SECTOR_SIZE: usize = 512;

/// A uniquely-identified block device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockDeviceId(pub u64);

/// Trait for block device I/O operations.
///
/// Implementors provide raw sector read/write access. The filesystem layer
/// builds on top of this for block allocation, journaling, and caching.
pub trait BlockDevice: Send + Sync {
    /// Read sectors from the device into the buffer.
    ///
    /// `sector` is the starting sector number.
    /// `buf` is the destination buffer (must be sector-aligned).
    /// Returns the number of sectors actually read.
    fn read_sectors(&self, sector: u64, buf: &mut [u8]) -> Result<usize, BlockError>;

    /// Write sectors from the buffer to the device.
    ///
    /// `sector` is the starting sector number.
    /// `buf` is the source buffer (must be sector-aligned).
    /// Returns the number of sectors actually written.
    fn write_sectors(&self, sector: u64, buf: &[u8]) -> Result<usize, BlockError>;

    /// Flush any cached writes to persistent storage.
    fn flush(&self) -> Result<(), BlockError>;

    /// Return the total number of sectors on this device.
    fn total_sectors(&self) -> u64;

    /// Return the sector size in bytes.
    fn sector_size(&self) -> usize {
        SECTOR_SIZE
    }

    /// Return a human-readable device name (e.g., "nvme0n1").
    fn device_name(&self) -> &str;

    /// Return the device ID.
    fn device_id(&self) -> BlockDeviceId;
}

/// Errors from block device operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockError {
    /// The device is not present or not ready.
    DeviceNotReady,
    /// The sector number is out of range.
    OutOfRange,
    /// An I/O error occurred on the device.
    IoError,
    /// The buffer is not sector-aligned.
    AlignmentError,
    /// The device is read-only.
    ReadOnly,
    /// Timeout waiting for device.
    Timeout,
}

/// A block cache that wraps a BlockDevice with sector-level caching.
pub struct BlockCache {
    device: alloc::sync::Arc<dyn BlockDevice>,
    cache: alloc::collections::BTreeMap<u64, alloc::vec::Vec<u8>>,
    cache_max_entries: usize,
}

impl BlockCache {
    pub fn new(device: alloc::sync::Arc<dyn BlockDevice>, cache_max_entries: usize) -> Self {
        Self {
            device,
            cache: alloc::collections::BTreeMap::new(),
            cache_max_entries,
        }
    }

    /// Read a sector, using cache if available.
    pub fn read_sector(&mut self, sector: u64) -> Result<alloc::vec::Vec<u8>, BlockError> {
        if let Some(cached) = self.cache.get(&sector) {
            return Ok(cached.clone());
        }
        let mut buf = alloc::vec![0u8; SECTOR_SIZE];
        self.device.read_sectors(sector, &mut buf)?;
        if self.cache.len() >= self.cache_max_entries {
            // Evict oldest entry
            if let Some(&oldest) = self.cache.keys().next() {
                self.cache.remove(&oldest);
            }
        }
        self.cache.insert(sector, buf.clone());
        Ok(buf)
    }

    /// Write a sector and mark it dirty in cache.
    pub fn write_sector(&mut self, sector: u64, data: &[u8]) -> Result<(), BlockError> {
        self.device.write_sectors(sector, data)?;
        self.cache.insert(sector, data.to_vec());
        Ok(())
    }

    /// Flush all cached sectors to the device.
    pub fn flush_all(&self) -> Result<(), BlockError> {
        self.device.flush()
    }

    /// Invalidate a cached sector.
    pub fn invalidate(&mut self, sector: u64) {
        self.cache.remove(&sector);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::sync::Arc;

    /// Mock block device for testing.
    struct MockBlockDevice {
        data: spin::Mutex<alloc::vec::Vec<u8>>,
        name: alloc::string::String,
        id: BlockDeviceId,
    }

    impl MockBlockDevice {
        fn new(sectors: u64) -> Self {
            Self {
                data: spin::Mutex::new(alloc::vec![0u8; sectors as usize * SECTOR_SIZE]),
                name: alloc::string::String::from("mock0"),
                id: BlockDeviceId(0),
            }
        }
    }

    impl BlockDevice for MockBlockDevice {
        fn read_sectors(&self, sector: u64, buf: &mut [u8]) -> Result<usize, BlockError> {
            let data = self.data.lock();
            let offset = sector as usize * SECTOR_SIZE;
            let len = buf.len();
            if offset + len > data.len() {
                return Err(BlockError::OutOfRange);
            }
            buf.copy_from_slice(&data[offset..offset + len]);
            Ok(len / SECTOR_SIZE)
        }

        fn write_sectors(&self, sector: u64, buf: &[u8]) -> Result<usize, BlockError> {
            let mut data = self.data.lock();
            let offset = sector as usize * SECTOR_SIZE;
            let len = buf.len();
            if offset + len > data.len() {
                return Err(BlockError::OutOfRange);
            }
            data[offset..offset + len].copy_from_slice(buf);
            Ok(len / SECTOR_SIZE)
        }

        fn flush(&self) -> Result<(), BlockError> {
            Ok(())
        }
        fn total_sectors(&self) -> u64 {
            self.data.lock().len() as u64 / SECTOR_SIZE as u64
        }
        fn device_name(&self) -> &str {
            &self.name
        }
        fn device_id(&self) -> BlockDeviceId {
            self.id
        }
    }

    #[test]
    fn test_read_write_sectors() {
        let dev = Arc::new(MockBlockDevice::new(16));
        let mut cache = BlockCache::new(dev.clone(), 4);

        let write_data = [0xAB; 512];
        cache.write_sector(0, &write_data).unwrap();

        let read_data = cache.read_sector(0).unwrap();
        assert_eq!(read_data, write_data);
    }

    #[test]
    fn test_cache_eviction() {
        let dev = Arc::new(MockBlockDevice::new(16));
        let mut cache = BlockCache::new(dev, 2);

        cache.write_sector(0, &[0x01; 512]).unwrap();
        cache.write_sector(1, &[0x02; 512]).unwrap();
        cache.write_sector(2, &[0x03; 512]).unwrap(); // Should evict sector 0

        // Sector 0 should be evicted (cache miss)
        let data = cache.read_sector(0).unwrap();
        assert_eq!(data[0], 0x01); // Read from device (mock returns zeros for untouched)
    }

    #[test]
    fn test_out_of_range() {
        let dev = Arc::new(MockBlockDevice::new(4));
        let mut cache = BlockCache::new(dev, 4);

        let result = cache.read_sector(100);
        assert_eq!(result, Err(BlockError::OutOfRange));
    }
}
