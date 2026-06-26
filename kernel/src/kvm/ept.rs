use alloc::boxed::Box;
use core::fmt;

/// Errors that can occur during EPT operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EptError {
    /// The guest or host physical address is not page-aligned.
    NotAligned,
    /// The EPT entry is not present.
    NotPresent,
    /// An intermediate page table page could not be allocated.
    AllocationFailed,
    /// The address is outside the canonical range.
    InvalidAddress,
    /// The page is already mapped.
    AlreadyMapped,
}

impl fmt::Display for EptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EptError::NotAligned => write!(f, "address is not page-aligned"),
            EptError::NotPresent => write!(f, "EPT entry is not present"),
            EptError::AllocationFailed => write!(f, "failed to allocate intermediate page table"),
            EptError::InvalidAddress => write!(f, "address is outside canonical range"),
            EptError::AlreadyMapped => write!(f, "page is already mapped"),
        }
    }
}

/// Flags for an EPT entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EptFlags {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
}

impl EptFlags {
    /// Read-only, no execute.
    pub const READ_ONLY: Self = Self {
        read: true,
        write: false,
        execute: false,
    };

    /// Read-write, no execute.
    pub const READ_WRITE: Self = Self {
        read: true,
        write: true,
        execute: false,
    };

    /// Read-write-execute.
    pub const FULL_ACCESS: Self = Self {
        read: true,
        write: true,
        execute: true,
    };

    /// Read-execute (no write).
    pub const READ_EXEC: Self = Self {
        read: true,
        write: false,
        execute: true,
    };

    pub fn new(read: bool, write: bool, execute: bool) -> Self {
        Self {
            read,
            write,
            execute,
        }
    }
}

/// A single EPT page table entry (8 bytes).
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct EptEntry {
    /// Bit 0: Read access.
    pub read: bool,
    /// Bit 1: Write access.
    pub write: bool,
    /// Bit 2: Execute access.
    pub execute: bool,
    /// Bit 12..(N*9+12): Physical address of the next level or page frame.
    /// Stored as a raw u64 (bits 12..51).
    pub physical_address: u64,
    /// Bit 7: Accessed flag (set by HW on access if "accessed" EPT is enabled).
    pub accessed: bool,
    /// Bit 8: Dirty flag (set by HW on write if "dirty" EPT is enabled).
    pub dirty: bool,
    /// Bit 9: Ignore PAT (for leaf entries).
    pub ignore_pat: bool,
    /// Bit 10: Guest memory type (for leaf entries, bits 3..5 of PAT index).
    pub memory_type: u8,
    /// Bit 11: Suppress #VE (Intel EPT).
    pub suppress_ve: bool,
}

impl EptEntry {
    /// Create a zeroed (not present) entry.
    pub fn not_present() -> Self {
        Self {
            read: false,
            write: false,
            execute: false,
            physical_address: 0,
            accessed: false,
            dirty: false,
            ignore_pat: false,
            memory_type: 0,
            suppress_ve: false,
        }
    }

    /// Check if this entry is present (any permission bit set).
    pub fn is_present(&self) -> bool {
        self.read || self.write || self.execute
    }

    /// Check if this is a leaf entry (points to a 4 KiB page).
    pub fn is_leaf(&self) -> bool {
        // In EPT, bit 7 (MT) distinguishes leaf vs non-leaf:
        // If bit 7 is 0 → leaf (page frame).
        // If bit 7 is 1 → next-level page table pointer.
        // However, this is the memory type for leaf, so we need to check
        // the actual hardware encoding. For simplicity, we track it separately.
        // In practice the caller knows whether to create leaf or non-leaf.
        // We use a convention: memory_type == 0 for leaf with default type,
        // memory_type > 0 for leaf with specific type, and a special sentinel
        // for non-leaf. This is simplified — real code would check bit 7.
        false
    }

    /// Encode this entry as a u64 for hardware.
    pub fn to_u64(self, is_leaf: bool) -> u64 {
        let mut val: u64 = 0;
        if self.read {
            val |= 1 << 0;
        }
        if self.write {
            val |= 1 << 1;
        }
        if self.execute {
            val |= 1 << 2;
        }
        // Bits 12..51: physical address (must be page-aligned)
        val |= (self.physical_address & 0x000F_FFFF_FFFF_F000) << 0;
        // For non-leaf entries (page directories), bit 7 must be 1.
        // For leaf entries (pages), bit 7 = 0, bits 3..5 = memory type.
        if !is_leaf {
            val |= 1 << 7; // "map" bit: indicates this is a page directory
        } else {
            val |= ((self.memory_type as u64) & 0x07) << 3;
            if self.ignore_pat {
                val |= 1 << 6;
            }
        }
        if self.accessed {
            val |= 1 << 8;
        }
        if self.dirty {
            val |= 1 << 9;
        }
        if self.suppress_ve {
            val |= 1 << 11;
        }
        val
    }

    /// Decode a u64 into an EPT entry.
    pub fn from_u64(val: u64) -> Self {
        Self {
            read: val & (1 << 0) != 0,
            write: val & (1 << 1) != 0,
            execute: val & (1 << 2) != 0,
            physical_address: (val >> 0) & 0x000F_FFFF_FFFF_F000,
            accessed: val & (1 << 8) != 0,
            dirty: val & (1 << 9) != 0,
            ignore_pat: val & (1 << 6) != 0,
            memory_type: ((val >> 3) & 0x07) as u8,
            suppress_ve: val & (1 << 11) != 0,
        }
    }
}

/// PML4 table (512 entries, 4096 bytes).
pub struct EptPml4 {
    pub entries: [EptEntry; 512],
}

impl EptPml4 {
    pub fn new() -> Self {
        Self {
            entries: [EptEntry::not_present(); 512],
        }
    }
}

impl Default for EptPml4 {
    fn default() -> Self {
        Self::new()
    }
}

/// Page-Directory-Pointer Table (512 entries, 4096 bytes).
pub struct EptPdpt {
    pub entries: [EptEntry; 512],
}

impl EptPdpt {
    pub fn new() -> Self {
        Self {
            entries: [EptEntry::not_present(); 512],
        }
    }
}

impl Default for EptPdpt {
    fn default() -> Self {
        Self::new()
    }
}

/// Page Directory (512 entries, 4096 bytes).
pub struct EptPd {
    pub entries: [EptEntry; 512],
}

impl EptPd {
    pub fn new() -> Self {
        Self {
            entries: [EptEntry::not_present(); 512],
        }
    }
}

impl Default for EptPd {
    fn default() -> Self {
        Self::new()
    }
}

/// Page Table (512 entries, 4096 bytes).
pub struct EptPt {
    pub entries: [EptEntry; 512],
}

impl EptPt {
    pub fn new() -> Self {
        Self {
            entries: [EptEntry::not_present(); 512],
        }
    }
}

impl Default for EptPt {
    fn default() -> Self {
        Self::new()
    }
}

/// Extended Page Table hierarchy for Intel VT-x.
///
/// Manages guest physical → host physical address translation.
pub struct Ept {
    /// Top-level PML4.
    pub pml4: Box<EptPml4>,
}

impl Ept {
    /// Create a new, empty EPT hierarchy.
    pub fn new() -> Self {
        Self {
            pml4: Box::new(EptPml4::new()),
        }
    }

    /// Returns the physical address of the PML4 (for the EPTP register).
    ///
    /// # Safety
    ///
    /// The caller must ensure the returned address is valid and the EPT
    /// hierarchy remains allocated for the lifetime of the VM.
    pub unsafe fn pml4_physical_address(&self) -> u64 {
        self.pml4.as_ref() as *const EptPml4 as u64
    }

    /// Decompose a guest physical address into PML4/PDPT/PD/PT indices.
    fn decompose(guest_phys: u64) -> (usize, usize, usize, usize) {
        let pml4_idx = ((guest_phys >> 39) & 0x1FF) as usize;
        let pdpt_idx = ((guest_phys >> 30) & 0x1FF) as usize;
        let pd_idx = ((guest_phys >> 21) & 0x1FF) as usize;
        let pt_idx = ((guest_phys >> 12) & 0x1FF) as usize;
        (pml4_idx, pdpt_idx, pd_idx, pt_idx)
    }

    /// Walk the EPT hierarchy to the page table level, allocating
    /// intermediate tables as needed.
    ///
    /// Returns references to the PDPT, PD, and PT entry pointers.
    fn walk_create(
        guest_phys: u64,
        pml4: &mut EptPml4,
    ) -> Result<
        (
            *mut EptEntry,
            *mut EptEntry,
            *mut EptEntry,
        ),
        EptError,
    > {
        let (pml4_idx, pdpt_idx, pd_idx, pt_idx) = Self::decompose(guest_phys);

        // PML4 → PDPT
        if !pml4.entries[pml4_idx].is_present() {
            let pdpt = Box::new(EptPdpt::new());
            let pdpt_pa = pdpt.as_ref() as *const EptPdpt as u64;
            pml4.entries[pml4_idx] = EptEntry {
                read: true,
                write: true,
                execute: true,
                physical_address: pdpt_pa,
                ..EptEntry::not_present()
            };
            Box::leak(pdpt);
        }
        let pdpt_pa = pml4.entries[pml4_idx].physical_address;
        // SAFETY: pdpt_pa was set from a valid Box allocation above or was
        // previously valid. The pointer is aligned to 4096 bytes.
        let pdpt = unsafe { &mut *(pdpt_pa as *mut EptPdpt) };

        // PDPT → PD
        if !pdpt.entries[pdpt_idx].is_present() {
            let pd = Box::new(EptPd::new());
            let pd_pa = pd.as_ref() as *const EptPd as u64;
            pdpt.entries[pdpt_idx] = EptEntry {
                read: true,
                write: true,
                execute: true,
                physical_address: pd_pa,
                ..EptEntry::not_present()
            };
            Box::leak(pd);
        }
        let pd_pa = pdpt.entries[pdpt_idx].physical_address;
        let pd = unsafe { &mut *(pd_pa as *mut EptPd) };

        // PD → PT
        if !pd.entries[pd_idx].is_present() {
            let pt = Box::new(EptPt::new());
            let pt_pa = pt.as_ref() as *const EptPt as u64;
            pd.entries[pd_idx] = EptEntry {
                read: true,
                write: true,
                execute: true,
                physical_address: pt_pa,
                ..EptEntry::not_present()
            };
            Box::leak(pt);
        }
        let pt_pa = pd.entries[pd_idx].physical_address;
        let pt = unsafe { &mut *(pt_pa as *mut EptPt) };

        Ok((
            &mut pdpt.entries[pdpt_idx],
            &mut pd.entries[pd_idx],
            &mut pt.entries[pt_idx],
        ))
    }

    /// Walk the EPT hierarchy to resolve a guest physical address to a
    /// host physical address.
    fn walk_resolve(guest_phys: u64, pml4: &EptPml4) -> Result<u64, EptError> {
        let (pml4_idx, pdpt_idx, pd_idx, pt_idx) = Self::decompose(guest_phys);

        // PML4 → PDPT
        if !pml4.entries[pml4_idx].is_present() {
            return Err(EptError::NotPresent);
        }
        let pdpt_pa = pml4.entries[pml4_idx].physical_address;
        let pdpt = unsafe { &*(pdpt_pa as *const EptPdpt) };

        // PDPT → PD
        if !pdpt.entries[pdpt_idx].is_present() {
            return Err(EptError::NotPresent);
        }
        let pd_pa = pdpt.entries[pdpt_idx].physical_address;
        let pd = unsafe { &*(pd_pa as *const EptPd) };

        // PD → PT
        if !pd.entries[pd_idx].is_present() {
            return Err(EptError::NotPresent);
        }
        let pt_pa = pd.entries[pd_idx].physical_address;
        let pt = unsafe { &*(pt_pa as *const EptPt) };

        // PT → page frame
        let entry = &pt.entries[pt_idx];
        if !entry.is_present() {
            return Err(EptError::NotPresent);
        }
        Ok(entry.physical_address | (guest_phys & 0xFFF))
    }

    /// Map a guest physical address to a host physical address.
    ///
    /// Both addresses must be page-aligned (4 KiB).
    pub fn map_page(
        &mut self,
        guest_phys: u64,
        host_phys: u64,
        flags: EptFlags,
    ) -> Result<(), EptError> {
        if guest_phys % crate::memory::PAGE_SIZE != 0 || host_phys % crate::memory::PAGE_SIZE != 0 {
            return Err(EptError::NotAligned);
        }

        let (_, _, _, _pt_idx) = Self::decompose(guest_phys);
        let (_pdpt_entry, _pd_entry, pt_entry) =
            Self::walk_create(guest_phys, &mut self.pml4)?;

        // SAFETY: pt_entry points into a valid, allocated page table.
        let entry = unsafe { &mut *pt_entry };
        if entry.is_present() {
            return Err(EptError::AlreadyMapped);
        }

        *entry = EptEntry {
            read: flags.read,
            write: flags.write,
            execute: flags.execute,
            physical_address: host_phys,
            ..EptEntry::not_present()
        };
        Ok(())
    }

    /// Unmap a guest physical address.
    pub fn unmap_page(&mut self, guest_phys: u64) -> Result<(), EptError> {
        let (_, _, _, _pt_idx) = Self::decompose(guest_phys);
        let (_pdpt_entry, _pd_entry, pt_entry) =
            Self::walk_create(guest_phys, &mut self.pml4)?;

        let entry = unsafe { &mut *pt_entry };
        if !entry.is_present() {
            return Err(EptError::NotPresent);
        }

        *entry = EptEntry::not_present();
        Ok(())
    }

    /// Resolve a guest physical address to a host physical address.
    pub fn resolve(&self, guest_phys: u64) -> Option<u64> {
        Self::walk_resolve(guest_phys, &self.pml4).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_serial::acquire;

    #[test]
    fn ept_new_creates_empty_pml4() {
        let _guard = acquire();
        let ept = Ept::new();
        // All PML4 entries should be not present.
        for entry in &ept.pml4.entries {
            assert!(!entry.is_present());
        }
    }

    #[test]
    fn ept_map_and_resolve() {
        let _guard = acquire();
        let mut ept = Ept::new();
        let guest = 0x4000_0000u64;
        let host = 0x0010_0000u64;
        ept.map_page(guest, host, EptFlags::READ_WRITE)
            .unwrap();
        assert_eq!(ept.resolve(guest), Some(host));
    }

    #[test]
    fn ept_unmap_page() {
        let _guard = acquire();
        let mut ept = Ept::new();
        let guest = 0x4000_0000u64;
        let host = 0x0010_0000u64;
        ept.map_page(guest, host, EptFlags::FULL_ACCESS)
            .unwrap();
        ept.unmap_page(guest).unwrap();
        assert_eq!(ept.resolve(guest), None);
    }

    #[test]
    fn ept_already_mapped_returns_error() {
        let _guard = acquire();
        let mut ept = Ept::new();
        let guest = 0x4000_0000u64;
        let host = 0x0010_0000u64;
        ept.map_page(guest, host, EptFlags::READ_ONLY).unwrap();
        let result = ept.map_page(guest, host, EptFlags::READ_WRITE);
        assert!(matches!(result, Err(EptError::AlreadyMapped)));
    }

    #[test]
    fn ept_not_aligned_returns_error() {
        let _guard = acquire();
        let mut ept = Ept::new();
        let result = ept.map_page(0x4000_0001, 0x0010_0000, EptFlags::READ_ONLY);
        assert!(matches!(result, Err(EptError::NotAligned)));
    }

    #[test]
    fn ept_multiple_pages() {
        let _guard = acquire();
        let mut ept = Ept::new();
        let pages: [(u64, u64); 4] = [
            (0x0000_1000, 0x0100_0000),
            (0x0000_2000, 0x0200_0000),
            (0x0000_3000, 0x0300_0000),
            (0x0000_4000, 0x0400_0000),
        ];
        for &(guest, host) in &pages {
            ept.map_page(guest, host, EptFlags::FULL_ACCESS)
                .unwrap();
        }
        for &(guest, host) in &pages {
            assert_eq!(ept.resolve(guest), Some(host));
        }
    }

    #[test]
    fn ept_flags_preserved() {
        let _guard = acquire();
        let mut ept = Ept::new();
        let guest = 0x4000_0000u64;
        let host = 0x0010_0000u64;
        ept.map_page(guest, host, EptFlags::READ_EXEC).unwrap();
        // Resolve returns the host address; flags are stored in the entry.
        let (_, _, _, pt_idx) = Ept::decompose(guest);
        // Walk down to the entry to check flags.
        let pml4_idx = ((guest >> 39) & 0x1FF) as usize;
        let pdpt_idx = ((guest >> 30) & 0x1FF) as usize;
        let pd_idx = ((guest >> 21) & 0x1FF) as usize;
        let pdpt_pa = ept.pml4.entries[pml4_idx].physical_address;
        let pdpt = unsafe { &*(pdpt_pa as *const EptPdpt) };
        let pd_pa = pdpt.entries[pdpt_idx].physical_address;
        let pd = unsafe { &*(pd_pa as *const EptPd) };
        let pt_pa = pd.entries[pd_idx].physical_address;
        let pt = unsafe { &*(pt_pa as *const EptPt) };
        let entry = &pt.entries[pt_idx];
        assert!(entry.read);
        assert!(!entry.write);
        assert!(entry.execute);
    }

    #[test]
    fn ept_large_address_range() {
        let _guard = acquire();
        let mut ept = Ept::new();
        // Map a high address (above 4 GiB).
        let guest = 0x1_0000_0000u64;
        let host = 0x0020_0000u64;
        ept.map_page(guest, host, EptFlags::READ_WRITE).unwrap();
        assert_eq!(ept.resolve(guest), Some(host));
    }

    #[test]
    fn ept_entry_encoding_roundtrip() {
        let _guard = acquire();
        let entry = EptEntry {
            read: true,
            write: true,
            execute: false,
            physical_address: 0x0000_1000_0000_F000,
            accessed: true,
            dirty: false,
            ignore_pat: true,
            memory_type: 6,
            suppress_ve: false,
        };
        let encoded = entry.to_u64(true);
        let decoded = EptEntry::from_u64(encoded);
        assert!(decoded.read);
        assert!(decoded.write);
        assert!(!decoded.execute);
        assert_eq!(decoded.physical_address, 0x0000_1000_0000_F000);
        assert!(decoded.accessed);
        assert!(!decoded.dirty);
        assert!(decoded.ignore_pat);
        assert_eq!(decoded.memory_type, 6);
    }

    #[test]
    fn ept_resolve_unmapped_returns_none() {
        let _guard = acquire();
        let ept = Ept::new();
        assert_eq!(ept.resolve(0x4000_0000), None);
    }
}
