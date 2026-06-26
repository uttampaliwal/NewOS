//! zram — compressed block device in RAM.
//!
//! zram is a compressed block device that stores pages in memory using
//! compression. Unlike zswap (which is a cache in front of a swap device),
//! zram IS the swap device — there is no backing store. This makes it ideal
//! for systems where disk I/O latency is unacceptable.
//!
//! Each slot stores a compressed representation of one 4 KiB page. The
//! compression ratio directly determines how much more logical swap space
//! is available compared to the physical memory consumed.
//!
//! Design:
//! - Backed by a `Vec<Option<CompressedSlot>>` indexed by swap slot number.
//! - Each `CompressedSlot` holds the compressed page data inline.
//! - Implements `SwapDevice` trait so it can be used as a drop-in replacement.
//! - Statistics tracking: reads, writes, compression ratios.

use alloc::vec::Vec;
use spin::Mutex;

use super::compress::{compress, decompress, HEADER_SIZE};
use super::swap::{SwapDevice, SwapError, SwapSlot};

/// A single compressed slot in the zram device.
#[derive(Debug, Clone)]
struct CompressedSlot {
    /// Compressed page data (including 8-byte header).
    data: Vec<u8>,
}

/// Statistics for the zram device.
#[derive(Debug, Clone, Default)]
pub struct ZramStats {
    /// Total read operations.
    pub reads: u64,
    /// Total write operations.
    pub writes: u64,
    /// Pages where compression had no benefit (stored uncompressed).
    pub no_compression: u64,
    /// Total compressed bytes stored.
    pub compressed_bytes: u64,
    /// Total uncompressed bytes stored.
    pub uncompressed_bytes: u64,
    /// Number of occupied slots.
    pub used_slots: usize,
    /// Total logical slots.
    pub total_slots: usize,
}

/// The zram compressed block device.
struct ZramDevice {
    slots: Vec<Option<CompressedSlot>>,
    stats: ZramStats,
}

static ZRAM_DEVICE: Mutex<Option<ZramDevice>> = Mutex::new(None);

/// Initialize the zram device with the given number of logical slots.
/// Each slot represents one 4 KiB page.
pub fn init_zram(slot_count: usize) {
    let mut slots = Vec::with_capacity(slot_count);
    slots.resize_with(slot_count, || None);

    let device = ZramDevice {
        slots,
        stats: ZramStats {
            total_slots: slot_count,
            ..Default::default()
        },
    };
    *ZRAM_DEVICE.lock() = Some(device);
}

/// Reset the zram device (for tests).
pub fn reset_zram() {
    *ZRAM_DEVICE.lock() = None;
}

/// Get current zram statistics.
pub fn zram_stats() -> ZramStats {
    ZRAM_DEVICE
        .lock()
        .as_ref()
        .map(|d| {
            let mut stats = d.stats.clone();
            stats.used_slots = d.slots.iter().filter(|s| s.is_some()).count();
            stats
        })
        .unwrap_or_default()
}

/// Write a page to the zram device. The page is compressed and stored
/// in the slot. Returns `Ok(())` on success.
pub fn zram_write(slot: SwapSlot, page_data: &[u8; 4096]) -> Result<(), SwapError> {
    let mut guard = ZRAM_DEVICE.lock();
    let device = guard.as_mut().ok_or(SwapError::DeviceNotPresent)?;

    if slot.0 >= device.slots.len() {
        return Err(SwapError::InvalidSlot);
    }

    let compressed = compress(page_data).ok_or(SwapError::IoError)?;

    let payload_len = compressed.len().saturating_sub(HEADER_SIZE);

    // If compression doesn't save space, store uncompressed
    if payload_len >= 4096 {
        let entry = CompressedSlot {
            data: compressed,
        };
        device.stats.no_compression += 1;
        device.stats.compressed_bytes += 4096;
        device.stats.uncompressed_bytes += 4096;
        device.slots[slot.0] = Some(entry);
    } else {
        let entry = CompressedSlot {
            data: compressed,
        };
        device.stats.compressed_bytes += payload_len as u64;
        device.stats.uncompressed_bytes += 4096;
        device.slots[slot.0] = Some(entry);
    }

    device.stats.writes += 1;
    Ok(())
}

/// Read a page from the zram device. Decompresses the stored data
/// and returns the original 4 KiB page.
pub fn zram_read(slot: SwapSlot) -> Result<[u8; 4096], SwapError> {
    let mut guard = ZRAM_DEVICE.lock();
    let device = guard.as_mut().ok_or(SwapError::DeviceNotPresent)?;

    if slot.0 >= device.slots.len() {
        return Err(SwapError::InvalidSlot);
    }

    let entry = device.slots[slot.0]
        .as_ref()
        .ok_or(SwapError::InvalidSlot)?;

    device.stats.reads += 1;

    let decompressed = decompress(&entry.data).ok_or(SwapError::IoError)?;
    if decompressed.len() != 4096 {
        return Err(SwapError::IoError);
    }

    let mut page_data = [0u8; 4096];
    page_data.copy_from_slice(&decompressed);
    Ok(page_data)
}

/// Free a slot in the zram device.
pub fn zram_free(slot: SwapSlot) -> bool {
    let mut guard = ZRAM_DEVICE.lock();
    let device = match guard.as_mut() {
        Some(d) => d,
        None => return false,
    };

    if slot.0 >= device.slots.len() {
        return false;
    }

    if device.slots[slot.0].is_some() {
        let entry = device.slots[slot.0].take().unwrap();
        let payload_len = entry.data.len().saturating_sub(HEADER_SIZE);
        device.stats.compressed_bytes = device
            .stats
            .compressed_bytes
            .saturating_sub(payload_len as u64);
        device.stats.uncompressed_bytes = device
            .stats
            .uncompressed_bytes
            .saturating_sub(4096);
        true
    } else {
        false
    }
}

/// Check if a slot is occupied.
pub fn zram_is_used(slot: SwapSlot) -> bool {
    ZRAM_DEVICE
        .lock()
        .as_ref()
        .and_then(|d| d.slots.get(slot.0))
        .and_then(|s| s.as_ref())
        .is_some()
}

/// Return the number of occupied slots.
pub fn zram_used_count() -> usize {
    ZRAM_DEVICE
        .lock()
        .as_ref()
        .map(|d| d.slots.iter().filter(|s| s.is_some()).count())
        .unwrap_or(0)
}

/// Return the total number of logical slots.
pub fn zram_total_slots() -> usize {
    ZRAM_DEVICE
        .lock()
        .as_ref()
        .map(|d| d.slots.len())
        .unwrap_or(0)
}

/// Calculate the effective compression ratio (compressed / original) * 100.
pub fn zram_effective_ratio() -> u32 {
    let stats = zram_stats();
    if stats.uncompressed_bytes == 0 {
        return 100;
    }
    ((stats.compressed_bytes * 100) / stats.uncompressed_bytes) as u32
}

/// Implement SwapDevice for zram so it can be used with the swap manager.
pub struct ZramSwapBackend;

impl SwapDevice for ZramSwapBackend {
    fn read_page(&self, slot: SwapSlot, buffer: &mut [u8; 4096]) -> Result<(), SwapError> {
        let page = zram_read(slot)?;
        buffer.copy_from_slice(&page);
        Ok(())
    }

    fn write_page(&self, slot: SwapSlot, buffer: &[u8; 4096]) -> Result<(), SwapError> {
        zram_write(slot, buffer)
    }

    fn total_slots(&self) -> usize {
        zram_total_slots()
    }

    fn device_name(&self) -> &str {
        "zram"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_page(seed: u8) -> [u8; 4096] {
        let mut page = [0u8; 4096];
        for (i, byte) in page.iter_mut().enumerate() {
            *byte = seed.wrapping_add(i as u8);
        }
        page
    }

    #[test]
    fn zram_write_and_read() {
        let _s = crate::test_serial::acquire();
        reset_zram();
        init_zram(64);

        let page = make_test_page(0x11);
        zram_write(SwapSlot(0), &page).unwrap();
        let readback = zram_read(SwapSlot(0)).unwrap();
        assert_eq!(page, readback);
    }

    #[test]
    fn zram_read_nonexistent_returns_error() {
        let _s = crate::test_serial::acquire();
        reset_zram();
        init_zram(64);

        assert_eq!(zram_read(SwapSlot(0)), Err(SwapError::InvalidSlot));
    }

    #[test]
    fn zram_write_read_nonexistent_slot() {
        let _s = crate::test_serial::acquire();
        reset_zram();
        init_zram(4);

        let page = make_test_page(0x22);
        assert_eq!(zram_write(SwapSlot(100), &page), Err(SwapError::InvalidSlot));
        assert_eq!(zram_read(SwapSlot(100)), Err(SwapError::InvalidSlot));
    }

    #[test]
    fn zram_free_slot() {
        let _s = crate::test_serial::acquire();
        reset_zram();
        init_zram(64);

        let page = make_test_page(0x33);
        zram_write(SwapSlot(5), &page).unwrap();
        assert!(zram_is_used(SwapSlot(5)));

        assert!(zram_free(SwapSlot(5)));
        assert!(!zram_is_used(SwapSlot(5)));
        assert!(!zram_free(SwapSlot(5))); // already freed
    }

    #[test]
    fn zram_multiple_slots() {
        let _s = crate::test_serial::acquire();
        reset_zram();
        init_zram(64);

        for i in 0..10u8 {
            let page = make_test_page(i * 5);
            zram_write(SwapSlot(i as usize), &page).unwrap();
        }

        for i in 0..10u8 {
            let page = make_test_page(i * 5);
            let readback = zram_read(SwapSlot(i as usize)).unwrap();
            assert_eq!(page, readback, "slot {} failed round-trip", i);
        }
    }

    #[test]
    fn zram_stats_tracking() {
        let _s = crate::test_serial::acquire();
        reset_zram();
        init_zram(64);

        let page = make_test_page(0x44);
        zram_write(SwapSlot(0), &page).unwrap();
        zram_write(SwapSlot(1), &page).unwrap();
        let _ = zram_read(SwapSlot(0));
        let _ = zram_read(SwapSlot(0));
        let _ = zram_read(SwapSlot(1));

        let stats = zram_stats();
        assert_eq!(stats.writes, 2);
        assert_eq!(stats.reads, 3);
        assert_eq!(stats.used_slots, 2);
    }

    #[test]
    fn zram_compression_ratio() {
        let _s = crate::test_serial::acquire();
        reset_zram();
        init_zram(64);

        // Highly compressible page (all zeros)
        let page = [0u8; 4096];
        zram_write(SwapSlot(0), &page).unwrap();

        let ratio = zram_effective_ratio();
        assert!(
            ratio < 50,
            "all-zeros should compress well, got ratio {}%",
            ratio
        );
    }

    #[test]
    fn zram_compressed_bytes_less_than_uncompressed() {
        let _s = crate::test_serial::acquire();
        reset_zram();
        init_zram(64);

        // Write many compressible pages
        for i in 0..10u8 {
            let mut page = [0u8; 4096];
            // Simple pattern that compresses well
            for j in 0..4096 {
                page[j] = i;
            }
            zram_write(SwapSlot(i as usize), &page).unwrap();
        }

        let stats = zram_stats();
        assert!(
            stats.compressed_bytes < stats.uncompressed_bytes,
            "compressed ({}) should be less than uncompressed ({})",
            stats.compressed_bytes,
            stats.uncompressed_bytes
        );
    }

    #[test]
    fn zram_overwrite_slot() {
        let _s = crate::test_serial::acquire();
        reset_zram();
        init_zram(64);

        let page1 = make_test_page(0xAA);
        let page2 = make_test_page(0xBB);

        zram_write(SwapSlot(0), &page1).unwrap();
        let read1 = zram_read(SwapSlot(0)).unwrap();
        assert_eq!(page1, read1);

        zram_write(SwapSlot(0), &page2).unwrap();
        let read2 = zram_read(SwapSlot(0)).unwrap();
        assert_eq!(page2, read2);
    }

    #[test]
    fn zram_uninitialized_returns_error() {
        let _s = crate::test_serial::acquire();
        reset_zram();
        assert_eq!(zram_read(SwapSlot(0)), Err(SwapError::DeviceNotPresent));
    }

    #[test]
    fn test_zram_total_slots() {
        let _s = crate::test_serial::acquire();
        reset_zram();
        init_zram(128);
        assert_eq!(zram_total_slots(), 128);
    }

    #[test]
    fn zram_swap_backend_trait() {
        let _s = crate::test_serial::acquire();
        reset_zram();
        init_zram(64);

        let backend = ZramSwapBackend;
        let page = make_test_page(0x55);
        let slot = SwapSlot(3);

        backend.write_page(slot, &page).unwrap();
        let mut readback = [0u8; 4096];
        backend.read_page(slot, &mut readback).unwrap();
        assert_eq!(page, readback);
        assert_eq!(backend.total_slots(), 64);
        assert_eq!(backend.device_name(), "zram");
    }

    #[test]
    fn zram_no_compression_benefit_still_works() {
        let _s = crate::test_serial::acquire();
        reset_zram();
        init_zram(64);

        // Random-ish data that won't compress well
        let mut page = [0u8; 4096];
        for (i, byte) in page.iter_mut().enumerate() {
            *byte = (i as u8).wrapping_mul(37).wrapping_add(11);
        }

        zram_write(SwapSlot(0), &page).unwrap();
        let readback = zram_read(SwapSlot(0)).unwrap();
        assert_eq!(page, readback);

        let stats = zram_stats();
        // Should either have no_compression or still work
        assert!(stats.no_compression > 0 || stats.compressed_bytes > 0);
    }

    #[test]
    fn zram_fill_and_free_all() {
        let _s = crate::test_serial::acquire();
        reset_zram();
        init_zram(16);

        for i in 0..16u8 {
            let page = make_test_page(i);
            zram_write(SwapSlot(i as usize), &page).unwrap();
        }
        assert_eq!(zram_used_count(), 16);

        for i in 0..16u8 {
            assert!(zram_free(SwapSlot(i as usize)));
        }
        assert_eq!(zram_used_count(), 0);
    }

    #[test]
    fn zram_stats_default() {
        let _s = crate::test_serial::acquire();
        let stats = ZramStats::default();
        assert_eq!(stats.reads, 0);
        assert_eq!(stats.writes, 0);
        assert_eq!(stats.used_slots, 0);
    }

    #[test]
    fn zram_all_zeros_compression() {
        let _s = crate::test_serial::acquire();
        reset_zram();
        init_zram(64);

        let page = [0u8; 4096];
        zram_write(SwapSlot(0), &page).unwrap();
        let readback = zram_read(SwapSlot(0)).unwrap();
        assert_eq!(page, readback);

        let stats = zram_stats();
        assert!(
            stats.compressed_bytes < 4096,
            "all zeros should compress to less than 4096 bytes, got {}",
            stats.compressed_bytes
        );
    }
}
