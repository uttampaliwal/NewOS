//! TPM 2.0 TIS (TPM Interface Specification) driver.
//!
//! Provides access to the Trusted Platform Module for secure key storage,
//! attestation, and random number generation. Implements the TIS MMIO
//! interface for QEMU's TPM emulation.

extern crate alloc;

use alloc::vec::Vec;

/// TIS register offsets (from TPM base address).
const TIS_REG_ACCESS: u64 = 0x00;
const _TIS_REG_INT_ENABLE: u64 = 0x08;
const _TIS_REG_INT_VECTOR: u64 = 0x0C;
const _TIS_REG_INT_STATUS: u64 = 0x10;
const _TIS_REG_INT_CAPABILITY: u64 = 0x14;
const TIS_REG_STATUS: u64 = 0x18;
const TIS_REG_DATA_FIFO: u64 = 0x24;
const _TIS_REG_INTERFACE_ID: u64 = 0x30;
const _TIS_REG_XDATA_FIFO: u64 = 0x80;
const TIS_REG_VID: u64 = 0xF00;
const TIS_REG_DID: u64 = 0xF04;

/// TIS access register bits.
const TIS_ACCESS_REQUEST_USE: u8 = 0x02;
const TIS_ACCESS_RELINQUISH: u8 = 0x02;
const _TIS_ACCESS_SEIZE: u8 = 0x01;
const TIS_ACCESS_ACTIVE: u8 = 0x20;
const _TIS_ACCESS_TPM_ESTABLISHMENT: u8 = 0x01;

/// TIS status register bits.
const _TIS_STATUS_STS_VALID: u8 = 0x80;
const TIS_STATUS_STS_READY: u8 = 0x40;
const TIS_STATUS_DATA_AVAIL: u8 = 0x10;
const _TIS_STATUS_DATA_EXPECT: u8 = 0x08;
const _TIS_STATUS_SELF_TEST_DONE: u8 = 0x04;
const _TIS_STATUS_CMD_RETRY: u8 = 0x02;

/// TIS interface identifiers.
const _TIS_IFACE_ID_IFACE_TIS: u8 = 0x00;
const _TIS_IFACE_ID_IFACE_TPM2: u8 = 0x01;

/// TPM command codes (TPM2).
pub mod commands {
    pub const TPM2_CC_STARTUP: u16 = 0x0144;
    pub const TPM2_CC_SHUTDOWN: u16 = 0x0145;
    pub const TPM2_CC_SELF_TEST: u16 = 0x0143;
    pub const TPM2_CC_GET_RANDOM: u16 = 0x017B;
    pub const TPM2_CC_READ_PUBLIC: u16 = 0x0081;
    pub const TPM2_CC_LOAD: u16 = 0x00D5;
    pub const TPM2_CC_UNSEAL: u16 = 0x015E;
    pub const TPM2_CC_FLUSH_CONTEXT: u16 = 0x0176;
}

/// TPM startup types.
#[derive(Debug, Clone, Copy)]
pub enum StartupType {
    Clear,
    State,
}

impl StartupType {
    fn to_bytes(self) -> [u8; 2] {
        match self {
            StartupType::Clear => 0x0000u16.to_le_bytes(),
            StartupType::State => 0x0001u16.to_le_bytes(),
        }
    }
}

/// TPM result codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TpmError {
    /// TPM device not present or not responding.
    NotPresent,
    /// Timeout waiting for TPM.
    Timeout,
    /// TPM returned an error response.
    TpmError(u16),
    /// Data transfer error.
    TransferError,
    /// TPM is not initialized.
    NotInitialized,
    /// Command not supported.
    UnsupportedCommand,
}

/// TPM 2.0 TIS driver.
pub struct TpmDriver {
    /// MMIO base address of the TPM.
    base_addr: u64,
    /// Whether the TPM is initialized.
    initialized: bool,
    /// TPM manufacturer ID (from VID register).
    manufacturer_id: u32,
    /// TPM device ID (from DID register).
    device_id: u32,
}

unsafe impl Send for TpmDriver {}
unsafe impl Sync for TpmDriver {}

impl TpmDriver {
    /// Create a new TPM driver for the given MMIO base address.
    ///
    /// # Safety
    ///
    /// `base_addr` must point to a valid TPM TIS MMIO region that is
    /// properly mapped and accessible for the lifetime of the driver.
    pub unsafe fn new(base_addr: u64) -> Self {
        Self {
            base_addr,
            initialized: false,
            manufacturer_id: 0,
            device_id: 0,
        }
    }

    /// Read a byte from a TIS register.
    unsafe fn read_u8(&self, reg: u64) -> u8 {
        unsafe {
            let ptr = (self.base_addr + reg) as *const u8;
            core::ptr::read_volatile(ptr)
        }
    }

    /// Write a byte to a TIS register.
    unsafe fn write_u8(&self, reg: u64, value: u8) {
        unsafe {
            let ptr = (self.base_addr + reg) as *mut u8;
            core::ptr::write_volatile(ptr, value);
        }
    }

    /// Read a 32-bit value from a TIS register (4 consecutive reads).
    unsafe fn read_u32(&self, reg: u64) -> u32 {
        unsafe {
            let mut val = 0u32;
            for i in 0..4 {
                val |= (self.read_u8(reg + i) as u32) << (i * 8);
            }
            val
        }
    }

    /// Probe the TPM device and read identification registers.
    pub fn probe(&mut self) -> Result<(), TpmError> {
        unsafe {
            // Read VID and DID registers
            self.manufacturer_id = self.read_u32(TIS_REG_VID);
            self.device_id = self.read_u32(TIS_REG_DID);

            // Check if this looks like a valid TPM
            // QEMU TPM: VID = 0x1022 (AMD) or 0x15D8 (Fujitsu), DID varies
            // VID=0 or DID=0 means no device is present at this address
            if self.manufacturer_id == 0xFFFFFFFF
                || self.device_id == 0xFFFFFFFF
                || self.manufacturer_id == 0
                || self.device_id == 0
            {
                return Err(TpmError::NotPresent);
            }

            crate::serial::println!(
                "[TPM] Found device: VID={:#010x} DID={:#010x}",
                self.manufacturer_id,
                self.device_id
            );
        }
        Ok(())
    }

    /// Request access to the TPM.
    pub fn request_access(&self) -> Result<(), TpmError> {
        unsafe {
            self.write_u8(TIS_REG_ACCESS, TIS_ACCESS_REQUEST_USE);

            // Wait for ACCESS_ACTIVE bit
            let deadline = crate::task::scheduler::get_uptime_ticks() + 1000;
            loop {
                let access = self.read_u8(TIS_REG_ACCESS);
                if access & TIS_ACCESS_ACTIVE != 0 {
                    return Ok(());
                }
                if crate::task::scheduler::get_uptime_ticks() > deadline {
                    return Err(TpmError::Timeout);
                }
                core::hint::spin_loop();
            }
        }
    }

    /// Relinquish access to the TPM.
    pub fn relinquish(&self) {
        unsafe {
            self.write_u8(TIS_REG_ACCESS, TIS_ACCESS_RELINQUISH);
        }
    }

    /// Wait for the TPM to be ready to accept a command.
    pub fn wait_ready(&self) -> Result<(), TpmError> {
        unsafe {
            let deadline = crate::task::scheduler::get_uptime_ticks() + 5000;
            loop {
                let status = self.read_u8(TIS_REG_STATUS);
                if status & TIS_STATUS_STS_READY != 0 {
                    return Ok(());
                }
                if crate::task::scheduler::get_uptime_ticks() > deadline {
                    return Err(TpmError::Timeout);
                }
                core::hint::spin_loop();
            }
        }
    }

    /// Send a TPM2 command and receive the response.
    pub fn send_command(&mut self, command: u16, input: &[u8]) -> Result<Vec<u8>, TpmError> {
        if !self.initialized {
            return Err(TpmError::NotInitialized);
        }

        // Build TPM2 command header (10 bytes)
        let mut cmd = Vec::new();
        // Tag (TPM2_ST_NO_SESSIONS = 0x8001)
        cmd.push(0x81);
        cmd.push(0x80);
        // Total length (header 10 + input)
        let total_len = (10 + input.len()) as u32;
        cmd.extend_from_slice(&total_len.to_be_bytes());
        // Command code
        cmd.extend_from_slice(&command.to_be_bytes());
        // Input data
        cmd.extend_from_slice(input);

        // Wait for ready
        self.wait_ready()?;

        // Write command to FIFO
        unsafe {
            for &byte in &cmd {
                self.write_u8(TIS_REG_DATA_FIFO, byte);
            }
        }

        // Read response
        self.read_response()
    }

    /// Read the TPM response.
    fn read_response(&self) -> Result<Vec<u8>, TpmError> {
        // Wait for data available
        let deadline = crate::task::scheduler::get_uptime_ticks() + 5000;
        loop {
            let status = unsafe { self.read_u8(TIS_REG_STATUS) };
            if status & TIS_STATUS_DATA_AVAIL != 0 {
                break;
            }
            if crate::task::scheduler::get_uptime_ticks() > deadline {
                return Err(TpmError::Timeout);
            }
            core::hint::spin_loop();
        }

        // Read response header (10 bytes: tag, length, response code)
        let mut header = [0u8; 10];
        unsafe {
            for slot in &mut header {
                *slot = self.read_u8(TIS_REG_DATA_FIFO);
            }
        }

        let resp_len = u32::from_be_bytes([header[4], header[5], header[6], header[7]]) as usize;
        let resp_code = u16::from_be_bytes([header[8], header[9]]);

        if resp_code != 0 {
            return Err(TpmError::TpmError(resp_code));
        }

        // Read remaining data
        let data_len = resp_len.saturating_sub(10);
        let mut data = Vec::with_capacity(data_len);
        unsafe {
            for _ in 0..data_len {
                // Wait for data available for each byte
                let deadline = crate::task::scheduler::get_uptime_ticks() + 1000;
                loop {
                    let status = self.read_u8(TIS_REG_STATUS);
                    if status & TIS_STATUS_DATA_AVAIL != 0 {
                        break;
                    }
                    if crate::task::scheduler::get_uptime_ticks() > deadline {
                        return Err(TpmError::Timeout);
                    }
                    core::hint::spin_loop();
                }
                data.push(self.read_u8(TIS_REG_DATA_FIFO));
            }
        }

        Ok(data)
    }

    /// Initialize the TPM (startup + self-test).
    pub fn initialize(&mut self) -> Result<(), TpmError> {
        self.probe()?;
        self.request_access()?;

        // TPM2_Startup(Clear)
        crate::serial::println!("[TPM] Sending startup command...");
        self.send_command(commands::TPM2_CC_STARTUP, &StartupType::Clear.to_bytes())?;

        // TPM2_SelfTest
        crate::serial::println!("[TPM] Running self-test...");
        self.send_command(commands::TPM2_CC_SELF_TEST, &1u8.to_le_bytes())?;

        self.initialized = true;
        crate::serial::println!("[TPM] Initialized successfully");
        Ok(())
    }

    /// Get random bytes from the TPM.
    pub fn get_random(&mut self, count: usize) -> Result<Vec<u8>, TpmError> {
        if !self.initialized {
            return Err(TpmError::NotInitialized);
        }

        let count_u16 = core::cmp::min(count, 65535) as u16;
        let data = self.send_command(commands::TPM2_CC_GET_RANDOM, &count_u16.to_be_bytes())?;

        // Response: tag(2) + len(4) + rc(2) + size(2) + data
        if data.len() < 2 {
            return Err(TpmError::TransferError);
        }
        let rand_size = u16::from_be_bytes([data[0], data[1]]) as usize;
        if data.len() < 2 + rand_size {
            return Err(TpmError::TransferError);
        }

        Ok(data[2..2 + rand_size].to_vec())
    }

    /// Seal data with a TPM-stored key.
    /// Returns the sealed blob.
    pub fn seal(&mut self, data: &[u8]) -> Result<Vec<u8>, TpmError> {
        if !self.initialized {
            return Err(TpmError::NotInitialized);
        }

        // In a real implementation:
        // 1. TPM2_Create (create sealed object)
        // 2. TPM2_Load (load into handle space)
        // 3. Return sealed blob

        // For now, return the data as-is (placeholder)
        Ok(data.to_vec())
    }

    /// Unseal a TPM-sealed blob.
    pub fn unseal(&mut self, sealed: &[u8]) -> Result<Vec<u8>, TpmError> {
        if !self.initialized {
            return Err(TpmError::NotInitialized);
        }

        // In a real implementation:
        // 1. TPM2_LoadExternal (load sealed object)
        // 2. TPM2_Unseal (unwrap)
        // 3. Return plaintext

        // For now, return the data as-is (placeholder)
        Ok(sealed.to_vec())
    }

    /// Check if the TPM is initialized.
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Get the manufacturer ID.
    pub fn manufacturer_id(&self) -> u32 {
        self.manufacturer_id
    }

    /// Get the device ID.
    pub fn device_id(&self) -> u32 {
        self.device_id
    }
}

/// Initialize the TPM subsystem at the given MMIO base address.
pub fn init(base_addr: u64) -> Result<(), TpmError> {
    unsafe {
        let mut tpm = TpmDriver::new(base_addr);
        tpm.initialize()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tpm_error_equality() {
        let err = TpmError::NotPresent;
        assert_eq!(err, TpmError::NotPresent);

        let err = TpmError::TpmError(0x01E0);
        match err {
            TpmError::TpmError(code) => assert_eq!(code, 0x01E0),
            _ => panic!("expected TpmError"),
        }
    }

    #[test]
    fn test_startup_type_values() {
        assert_eq!(StartupType::Clear.to_bytes(), [0x00, 0x00]);
        assert_eq!(StartupType::State.to_bytes(), [0x01, 0x00]);
    }

    #[test]
    fn test_command_codes() {
        assert_eq!(commands::TPM2_CC_STARTUP, 0x0144);
        assert_eq!(commands::TPM2_CC_GET_RANDOM, 0x017B);
        assert_eq!(commands::TPM2_CC_SELF_TEST, 0x0143);
    }

    #[test]
    fn test_tpm_driver_new() {
        let tpm = unsafe { TpmDriver::new(0xFED40000) };
        assert!(!tpm.is_initialized());
        assert_eq!(tpm.manufacturer_id(), 0);
        assert_eq!(tpm.device_id(), 0);
    }
}
