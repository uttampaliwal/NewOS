use core::fmt;

/// Errors that can occur during VMCS operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmcsError {
    /// VMCS region is not 4096-byte aligned.
    Unaligned,
    /// VMREAD instruction failed.
    VmreadFailed,
    /// VMWRITE instruction failed.
    VmwriteFailed,
    /// VMCLEAR instruction failed.
    VmclearFailed,
    /// VMPTRLD instruction failed.
    VmpltdFailed,
    /// VMCS field encoding is invalid.
    InvalidField,
}

impl fmt::Display for VmcsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VmcsError::Unaligned => write!(f, "VMCS region is not 4096-byte aligned"),
            VmcsError::VmreadFailed => write!(f, "VMREAD instruction failed"),
            VmcsError::VmwriteFailed => write!(f, "VMWRITE instruction failed"),
            VmcsError::VmclearFailed => write!(f, "VMCLEAR instruction failed"),
            VmcsError::VmpltdFailed => write!(f, "VMPTRLD instruction failed"),
            VmcsError::InvalidField => write!(f, "VMCS field encoding is invalid"),
        }
    }
}

/// VMCS field encodings per Intel SDM Appendix B.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum VmcsField {
    // Guest state area
    GuestRip = 0x0000_681E,
    GuestRsp = 0x0000_681F,
    GuestRflags = 0x0000_6820,
    GuestCr0 = 0x0000_6800,
    GuestCr3 = 0x0000_6802,
    GuestCr4 = 0x0000_6804,

    GuestCsSelector = 0x0000_0802,
    GuestCsBase = 0x0000_682E,
    GuestCsLimit = 0x0000_4802,
    GuestCsAccessRights = 0x0000_4816,

    GuestDsSelector = 0x0000_0804,
    GuestDsBase = 0x0000_6830,
    GuestDsLimit = 0x0000_4804,
    GuestDsAccessRights = 0x0000_4818,

    GuestSsSelector = 0x0000_080C,
    GuestSsBase = 0x0000_6838,
    GuestSsLimit = 0x0000_480C,
    GuestSsAccessRights = 0x0000_4820,

    GuestEsSelector = 0x0000_0800,
    GuestEsBase = 0x0000_682C,
    GuestEsLimit = 0x0000_4800,
    GuestEsAccessRights = 0x0000_4814,

    GuestFsSelector = 0x0000_0808,
    GuestFsBase = 0x0000_6832,
    GuestFsLimit = 0x0000_4808,
    GuestFsAccessRights = 0x0000_481C,

    GuestGsSelector = 0x0000_080A,
    GuestGsBase = 0x0000_6834,
    GuestGsLimit = 0x0000_480A,
    GuestGsAccessRights = 0x0000_481E,

    // VM-exit / VM-entry info
    ExitReason = 0x0000_4402,
    ExitQualification = 0x0000_6400,
    GuestInterruptibilityInfo = 0x0000_6824,

    // Pin-based / processor-based VM-execution controls
    PinBasedVmExecControl = 0x0000_4000,
    ProcBasedVmExecControl = 0x0000_4002,
    ProcBasedVmExecControl2 = 0x0000_401E,

    // VM-exit controls
    VmExitControls = 0x0000_4004,
    VmExitMsrStoreCount = 0x0000_400E,
    VmExitMsrLoadCount = 0x0000_4014,

    // VM-entry controls
    VmEntryControls = 0x0000_4006,
    VmEntryMsrLoadCount = 0x0000_4012,

    // I/O bitmap
    IoBitmapA = 0x0000_2000,
    IoBitmapB = 0x0000_2002,

    // EPT pointer
    EptPointer = 0x0000_2012,

    // Virtual APIC page
    VirtualApicPage = 0x0000_2014,

    // Guest / host state
    HostEsSelector = 0x0000_0C00,
    HostCsSelector = 0x0000_0C02,
    HostSsSelector = 0x0000_0C0C,
    HostFsSelector = 0x0000_0C08,
    HostGsSelector = 0x0000_0C0A,
    HostTrSelector = 0x0000_0C0E,
    HostCr0 = 0x0000_6C00,
    HostCr3 = 0x0000_6C02,
    HostCr4 = 0x0000_6C04,
    HostRip = 0x0000_6C16,
    HostRsp = 0x0000_6C14,

    // GDT / IDT
    GuestGdtrBase = 0x0000_683A,
    GuestGdtrLimit = 0x0000_4812,
    GuestIdtrBase = 0x0000_683C,

    // LDTR / TR
    GuestLdtrSelector = 0x0000_080E,
    GuestTrSelector = 0x0000_0810,

    // Segment limits stored in natural-width fields
    GuestCsLimitNat = 0x0000_6836,
}

/// A 4096-byte aligned VMCS region.
///
/// The VMCS is a hardware data structure used by Intel VT-x to manage
/// VM transitions. The region must be 4096-byte aligned and contain
/// the VMCS revision identifier in its first 4 bytes.
pub struct Vmcs {
    region: [u8; 4096],
}

unsafe impl Sync for Vmcs {}
unsafe impl Send for Vmcs {}

impl Vmcs {
    /// Create a new VMCS with the revision identifier set.
    ///
    /// The revision ID is stored in the first 4 bytes and must match the
    /// processor's VMCS revision (obtained from `IA32_VMX_BASIC` MSR).
    pub fn new(vmcs_revision_id: u32) -> Self {
        let mut vmcs = Self {
            region: [0u8; 4096],
        };
        // Store the VMCS revision ID in the first 4 bytes (little-endian).
        vmcs.region[0..4].copy_from_slice(&vmcs_revision_id.to_le_bytes());
        vmcs
    }

    /// Clear the VMCS by issuing VMCLEAR.
    ///
    /// # Safety
    ///
    /// The caller must ensure the VMCS pointer is valid.
    pub fn clear(&mut self) -> Result<(), VmcsError> {
        let ptr = self.region.as_ptr() as u64;
        let mut error: u32;
        unsafe {
            core::arch::asm!(
                "vmclear [{ptr}]",
                "setna al",
                "movzx eax, al",
                ptr = in(reg) ptr,
                out("eax") error,
                options(nostack),
            );
        }
        if error != 0 {
            Err(VmcsError::VmclearFailed)
        } else {
            Ok(())
        }
    }

    /// Load this VMCS as the current VMCS via VMPTRLD.
    ///
    /// # Safety
    ///
    /// The caller must be in VMX operation and the VMCS must be valid.
    pub fn load(&self) -> Result<(), VmcsError> {
        let ptr = self.region.as_ptr() as u64;
        let mut error: u32;
        unsafe {
            core::arch::asm!(
                "vmptrld [{ptr}]",
                "setna al",
                "movzx eax, al",
                ptr = in(reg) ptr,
                out("eax") error,
                options(nostack),
            );
        }
        if error != 0 {
            Err(VmcsError::VmpltdFailed)
        } else {
            Ok(())
        }
    }

    /// Read a 64-bit value from the currently loaded VMCS.
    pub fn read(&self, field: VmcsField) -> Result<u64, VmcsError> {
        let value: u64;
        let error: u32;
        unsafe {
            core::arch::asm!(
                "vmread {value}, {field}",
                "setna al",
                "movzx eax, al",
                value = out(reg) value,
                field = in(reg) field as u32 as u64,
                out("eax") error,
                options(nostack),
            );
        }
        if error != 0 {
            Err(VmcsError::VmreadFailed)
        } else {
            Ok(value)
        }
    }

    /// Write a 64-bit value to the currently loaded VMCS.
    pub fn write(&mut self, field: VmcsField, value: u64) -> Result<(), VmcsError> {
        let error: u32;
        unsafe {
            core::arch::asm!(
                "vmwrite {field}, {value}",
                "setna al",
                "movzx eax, al",
                field = in(reg) field as u32 as u64,
                value = in(reg) value,
                out("eax") error,
                options(nostack),
            );
        }
        if error != 0 {
            Err(VmcsError::VmwriteFailed)
        } else {
            Ok(())
        }
    }

    /// Returns the physical address of this VMCS region.
    pub fn physical_address(&self) -> u64 {
        self.region.as_ptr() as u64
    }

    /// Returns a reference to the raw VMCS region bytes.
    pub fn as_bytes(&self) -> &[u8; 4096] {
        &self.region
    }
}

/// Read a raw u64 from the VMCS at a given offset (no hardware VMREAD).
pub fn vmread_raw(vmcs: &Vmcs, offset: usize) -> Result<u64, VmcsError> {
    if offset + 8 > 4096 {
        return Err(VmcsError::InvalidField);
    }
    Ok(u64::from_le_bytes([
        vmcs.region[offset],
        vmcs.region[offset + 1],
        vmcs.region[offset + 2],
        vmcs.region[offset + 3],
        vmcs.region[offset + 4],
        vmcs.region[offset + 5],
        vmcs.region[offset + 6],
        vmcs.region[offset + 7],
    ]))
}

/// Write a raw u64 to the VMCS at a given offset (no hardware VMWRITE).
pub fn vmwrite_raw(vmcs: &mut Vmcs, offset: usize, value: u64) -> Result<(), VmcsError> {
    if offset + 8 > 4096 {
        return Err(VmcsError::InvalidField);
    }
    let bytes = value.to_le_bytes();
    vmcs.region[offset..offset + 8].copy_from_slice(&bytes);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_serial::acquire;

    #[test]
    fn vmcs_new_sets_revision_id() {
        let _guard = acquire();
        let vmcs = Vmcs::new(0x1A);
        let rev = u32::from_le_bytes(vmcs.region[0..4].try_into().unwrap());
        assert_eq!(rev, 0x1A);
    }

    #[test]
    fn vmcs_region_is_4096_bytes() {
        let _guard = acquire();
        let vmcs = Vmcs::new(0);
        assert_eq!(core::mem::size_of_val(&vmcs.region), 4096);
    }

    #[test]
    fn vmcs_raw_write_read_roundtrip() {
        let _guard = acquire();
        let mut vmcs = Vmcs::new(0);
        let offset = 0x100;
        let value: u64 = 0xDEAD_BEEF_CAFE_BABE;
        vmwrite_raw(&mut vmcs, offset, value).unwrap();
        let read_back = vmread_raw(&vmcs, offset).unwrap();
        assert_eq!(read_back, value);
    }

    #[test]
    fn vmcs_raw_out_of_bounds_returns_error() {
        let _guard = acquire();
        let mut vmcs = Vmcs::new(0);
        let result = vmwrite_raw(&mut vmcs, 4096, 42);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), VmcsError::InvalidField);
    }

    #[test]
    fn vmcs_raw_multiple_fields() {
        let _guard = acquire();
        let mut vmcs = Vmcs::new(1);
        let fields: [(usize, u64); 4] = [(0, 0x1), (64, 0xFF), (256, 0xFFFF), (4088, 0)];
        for &(offset, value) in &fields {
            vmwrite_raw(&mut vmcs, offset, value).unwrap();
        }
        for &(offset, expected) in &fields {
            assert_eq!(vmread_raw(&vmcs, offset).unwrap(), expected);
        }
    }

    #[test]
    fn vmcs_zero_fill_on_creation() {
        let _guard = acquire();
        let vmcs = Vmcs::new(0);
        // Bytes after the revision ID should be zero.
        assert_eq!(vmcs.region[4], 0);
        assert_eq!(vmcs.region[4095], 0);
    }

    #[test]
    fn vmcs_field_values_are_unique() {
        let _guard = acquire();
        let a = VmcsField::GuestRip;
        let b = VmcsField::GuestRsp;
        let c = VmcsField::ExitReason;
        assert_ne!(a as u32, b as u32);
        assert_ne!(a as u32, c as u32);
        assert_ne!(b as u32, c as u32);
    }

    #[test]
    fn vmcs_physical_address_nonzero() {
        let _guard = acquire();
        let vmcs = Vmcs::new(0);
        // In test binary the region lives in .bss, so its address is nonzero.
        assert_ne!(vmcs.physical_address(), 0);
    }
}
