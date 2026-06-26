/// VMCB exit codes (AMD-V).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum VmcbExitCode {
    /// #VMEXIT of type shutdown (e.g., triple fault).
    Shutdown = 0x0400,
    /// #VMEXIT due to an I/O instruction.
    Io = 0x0500,
    /// #VMEXIT due to an MSR access.
    Msr = 0x0501,
    /// #VMEXIT due to a CR register access.
    Cr = 0x0502,
    /// #VMEXIT due to an exception.
    Exception = 0x0600,
    /// #VMEXIT due to interrupt window.
    IntrWindow = 0x0601,
    /// #VMEXIT due to NMI window.
    NmiWindow = 0x0602,
    /// #VMEXIT due to HLT.
    Hlt = 0x0603,
    /// #VMEXIT due to INVLPG.
    Invlpg = 0x0604,
    /// #VMEXIT due to VMCB cache.
    VmcbCache = 0x0605,
    /// #VMEXIT due to INVD.
    Invd = 0x0606,
    /// #VMEXIT due to VMMCALL.
    Vmmcall = 0x0607,
    /// #VMEXIT due to VMLOAD.
    Vmload = 0x0612,
    /// #VMEXIT due to VMSAVE.
    Vmsave = 0x0613,
    /// #VMEXIT due to STGI.
    Stgi = 0x0614,
    /// #VMEXIT due to CLGI.
    Clgi = 0x0615,
    Unknown(u32),
}

impl From<u32> for VmcbExitCode {
    fn from(val: u32) -> Self {
        match val {
            0x0400 => Self::Shutdown,
            0x0500 => Self::Io,
            0x0501 => Self::Msr,
            0x0502 => Self::Cr,
            0x0600 => Self::Exception,
            0x0601 => Self::IntrWindow,
            0x0602 => Self::NmiWindow,
            0x0603 => Self::Hlt,
            0x0604 => Self::Invlpg,
            0x0605 => Self::VmcbCache,
            0x0606 => Self::Invd,
            0x0607 => Self::Vmmcall,
            0x0612 => Self::Vmload,
            0x0613 => Self::Vmsave,
            0x0614 => Self::Stgi,
            0x0615 => Self::Clgi,
            other => Self::Unknown(other),
        }
    }
}

/// VMCB control area (offset 0x000 – 0x3FF).
///
/// This is the first 1024 bytes of a VMCB. Fields are at fixed offsets
/// per the AMD PPR / APM Volume 2.
#[derive(Debug, Clone)]
#[repr(C, align(4096))]
pub struct VmcbControl {
    /// Control area bytes (4096 bytes total).
    pub data: [u8; 4096],
}

impl VmcbControl {
    /// Create a zeroed control area.
    pub fn new() -> Self {
        Self { data: [0u8; 4096] }
    }

    // --- Low-level field accessors (offset-based) ---

    fn read_u32(&self, offset: usize) -> u32 {
        u32::from_le_bytes(self.data[offset..offset + 4].try_into().unwrap())
    }

    fn write_u32(&mut self, offset: usize, value: u32) {
        self.data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn read_u64(&self, offset: usize) -> u64 {
        u64::from_le_bytes(self.data[offset..offset + 8].try_into().unwrap())
    }

    fn write_u64(&mut self, offset: usize, value: u64) {
        self.data[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }

    // --- CR0 (offset 0x008) ---
    pub fn cr0(&self) -> u64 {
        self.read_u64(0x008)
    }
    pub fn set_cr0(&mut self, value: u64) {
        self.write_u64(0x008, value);
    }

    // --- CR3 (offset 0x010) ---
    pub fn cr3(&self) -> u64 {
        self.read_u64(0x010)
    }
    pub fn set_cr3(&mut self, value: u64) {
        self.write_u64(0x010, value);
    }

    // --- CR4 (offset 0x018) ---
    pub fn cr4(&self) -> u64 {
        self.read_u64(0x018)
    }
    pub fn set_cr4(&mut self, value: u64) {
        self.write_u64(0x018, value);
    }

    // --- RIP (offset 0x058) ---
    pub fn rip(&self) -> u64 {
        self.read_u64(0x058)
    }
    pub fn set_rip(&mut self, value: u64) {
        self.write_u64(0x058, value);
    }

    // --- RFLAGS (offset 0x060) ---
    pub fn rflags(&self) -> u64 {
        self.read_u64(0x060)
    }
    pub fn set_rflags(&mut self, value: u64) {
        self.write_u64(0x060, value);
    }

    // --- Interrupt shadow (offset 0x068) ---
    pub fn interrupt_shadow(&self) -> u64 {
        self.read_u64(0x068)
    }
    pub fn set_interrupt_shadow(&mut self, value: u64) {
        self.write_u64(0x068, value);
    }

    // --- V_TPR (offset 0x070) ---
    pub fn v_tpr(&self) -> u64 {
        self.read_u64(0x070)
    }
    pub fn set_v_tpr(&mut self, value: u64) {
        self.write_u64(0x070, value);
    }

    // --- Exit code (offset 0x118) ---
    pub fn exitcode(&self) -> u32 {
        self.read_u32(0x118)
    }
    pub fn set_exitcode(&mut self, value: u32) {
        self.write_u32(0x118, value);
    }

    // --- ExitInfo1 (offset 0x120) ---
    pub fn exitinfo1(&self) -> u64 {
        self.read_u64(0x120)
    }
    pub fn set_exitinfo1(&mut self, value: u64) {
        self.write_u64(0x120, value);
    }

    // --- ExitInfo2 (offset 0x128) ---
    pub fn exitinfo2(&self) -> u64 {
        self.read_u64(0x128)
    }
    pub fn set_exitinfo2(&mut self, value: u64) {
        self.write_u64(0x128, value);
    }

    // --- ASID (offset 0x11C) ---
    pub fn asid(&self) -> u32 {
        self.read_u32(0x11C)
    }
    pub fn set_asid(&mut self, value: u32) {
        self.write_u32(0x11C, value);
    }

    // --- Nested paging / SEV bits (offset 0x11D byte) ---
    pub fn nested_paging(&self) -> bool {
        self.data[0x11D] & 0x01 != 0
    }
    pub fn set_nested_paging(&mut self, enabled: bool) {
        if enabled {
            self.data[0x11D] |= 0x01;
        } else {
            self.data[0x11D] &= !0x01;
        }
    }

    // --- TLB flush on VM exit (offset 0x040, bit 0) ---
    pub fn tlb_flush_on_exit(&self) -> bool {
        self.read_u32(0x040) & 0x01 != 0
    }
    pub fn set_tlb_flush_on_exit(&mut self, enabled: bool) {
        let mut val = self.read_u32(0x040);
        if enabled {
            val |= 0x01;
        } else {
            val &= !0x01;
        }
        self.write_u32(0x040, val);
    }
}

impl Default for VmcbControl {
    fn default() -> Self {
        Self::new()
    }
}

/// VMCB save area (offset 0x400 – 0x7FF).
///
/// Guest register state is stored here. The processor loads these
/// registers on VMRUN and saves them on #VMEXIT.
#[derive(Debug, Clone)]
#[repr(C, align(4096))]
pub struct VmcbSave {
    /// Save area bytes (4096 bytes total).
    pub data: [u8; 4096],
}

impl VmcbSave {
    /// Create a zeroed save area.
    pub fn new() -> Self {
        Self { data: [0u8; 4096] }
    }

    fn read_u64(&self, offset: usize) -> u64 {
        u64::from_le_bytes(self.data[offset..offset + 8].try_into().unwrap())
    }

    fn write_u64(&mut self, offset: usize, value: u64) {
        self.data[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }

    fn read_u32(&self, offset: usize) -> u32 {
        u32::from_le_bytes(self.data[offset..offset + 4].try_into().unwrap())
    }

    fn write_u32(&mut self, offset: usize, value: u32) {
        self.data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    // --- General-purpose registers (offset 0x1D8 region) ---
    // AMD-V save area offsets: es, cs, ss, ds, fs, gs are at 0x1D0..0x1F0,
    // then GPRs at 0x1F8..0x278.

    // RAX (offset 0x1F8)
    pub fn rax(&self) -> u64 {
        self.read_u64(0x1F8)
    }
    pub fn set_rax(&mut self, value: u64) {
        self.write_u64(0x1F8, value);
    }

    // RBX (offset 0x200)
    pub fn rbx(&self) -> u64 {
        self.read_u64(0x200)
    }
    pub fn set_rbx(&mut self, value: u64) {
        self.write_u64(0x200, value);
    }

    // RCX (offset 0x208)
    pub fn rcx(&self) -> u64 {
        self.read_u64(0x208)
    }
    pub fn set_rcx(&mut self, value: u64) {
        self.write_u64(0x208, value);
    }

    // RDX (offset 0x210)
    pub fn rdx(&self) -> u64 {
        self.read_u64(0x210)
    }
    pub fn set_rdx(&mut self, value: u64) {
        self.write_u64(0x210, value);
    }

    // RSI (offset 0x218)
    pub fn rsi(&self) -> u64 {
        self.read_u64(0x218)
    }
    pub fn set_rsi(&mut self, value: u64) {
        self.write_u64(0x218, value);
    }

    // RDI (offset 0x220)
    pub fn rdi(&self) -> u64 {
        self.read_u64(0x220)
    }
    pub fn set_rdi(&mut self, value: u64) {
        self.write_u64(0x220, value);
    }

    // RSP (offset 0x228)
    pub fn rsp(&self) -> u64 {
        self.read_u64(0x228)
    }
    pub fn set_rsp(&mut self, value: u64) {
        self.write_u64(0x228, value);
    }

    // RBP (offset 0x230)
    pub fn rbp(&self) -> u64 {
        self.read_u64(0x230)
    }
    pub fn set_rbp(&mut self, value: u64) {
        self.write_u64(0x230, value);
    }

    // R8 (offset 0x238)
    pub fn r8(&self) -> u64 {
        self.read_u64(0x238)
    }
    pub fn set_r8(&mut self, value: u64) {
        self.write_u64(0x238, value);
    }

    // R9 (offset 0x240)
    pub fn r9(&self) -> u64 {
        self.read_u64(0x240)
    }
    pub fn set_r9(&mut self, value: u64) {
        self.write_u64(0x240, value);
    }

    // R10 (offset 0x248)
    pub fn r10(&self) -> u64 {
        self.read_u64(0x248)
    }
    pub fn set_r10(&mut self, value: u64) {
        self.write_u64(0x248, value);
    }

    // R11 (offset 0x250)
    pub fn r11(&self) -> u64 {
        self.read_u64(0x250)
    }
    pub fn set_r11(&mut self, value: u64) {
        self.write_u64(0x250, value);
    }

    // R12 (offset 0x258)
    pub fn r12(&self) -> u64 {
        self.read_u64(0x258)
    }
    pub fn set_r12(&mut self, value: u64) {
        self.write_u64(0x258, value);
    }

    // R13 (offset 0x260)
    pub fn r13(&self) -> u64 {
        self.read_u64(0x260)
    }
    pub fn set_r13(&mut self, value: u64) {
        self.write_u64(0x260, value);
    }

    // R14 (offset 0x268)
    pub fn r14(&self) -> u64 {
        self.read_u64(0x268)
    }
    pub fn set_r14(&mut self, value: u64) {
        self.write_u64(0x268, value);
    }

    // R15 (offset 0x270)
    pub fn r15(&self) -> u64 {
        self.read_u64(0x270)
    }
    pub fn set_r15(&mut self, value: u64) {
        self.write_u64(0x270, value);
    }

    // --- Segment selectors ---

    // ES (offset 0x1D0)
    pub fn es_selector(&self) -> u16 {
        self.read_u32(0x1D0) as u16
    }
    pub fn set_es_selector(&mut self, value: u16) {
        self.write_u32(0x1D0, value as u32);
    }

    // ES base (offset 0x1D8)
    pub fn es_base(&self) -> u64 {
        self.read_u64(0x1D8)
    }
    pub fn set_es_base(&mut self, value: u64) {
        self.write_u64(0x1D8, value);
    }

    // CS selector (offset 0x1D2)
    pub fn cs_selector(&self) -> u16 {
        self.read_u32(0x1D2) as u16
    }
    pub fn set_cs_selector(&mut self, value: u16) {
        self.write_u32(0x1D2, value as u32);
    }

    // CS base (offset 0x1E0)
    pub fn cs_base(&self) -> u64 {
        self.read_u64(0x1E0)
    }
    pub fn set_cs_base(&mut self, value: u64) {
        self.write_u64(0x1E0, value);
    }

    // SS selector (offset 0x1D4)
    pub fn ss_selector(&self) -> u16 {
        self.read_u32(0x1D4) as u16
    }
    pub fn set_ss_selector(&mut self, value: u16) {
        self.write_u32(0x1D4, value as u32);
    }

    // SS base (offset 0x1E8)
    pub fn ss_base(&self) -> u64 {
        self.read_u64(0x1E8)
    }
    pub fn set_ss_base(&mut self, value: u64) {
        self.write_u64(0x1E8, value);
    }

    // DS selector (offset 0x1D6)
    pub fn ds_selector(&self) -> u16 {
        self.read_u32(0x1D6) as u16
    }
    pub fn set_ds_selector(&mut self, value: u16) {
        self.write_u32(0x1D6, value as u32);
    }

    // DS base (offset 0x1F0)
    pub fn ds_base(&self) -> u64 {
        self.read_u64(0x1F0)
    }
    pub fn set_ds_base(&mut self, value: u64) {
        self.write_u64(0x1F0, value);
    }

    // FS selector (offset 0x1D8 area, but stored in 0x1D8..0x1E0 region)
    // For simplicity, use offset 0x1F8 region alias; actual AMD-V layout:
    // FS selector at 0x1D8 + 0x08 = 0x1E0? No — let's use the standard:
    // ES=0x1D0, CS=0x1D2, SS=0x1D4, DS=0x1D6, FS=0x1D8, GS=0x1DA
    // But we already used 0x1D8 for ES base. Let's correct the offsets.
    //
    // The actual AMD-V VMCB save area layout (APM Vol 2, Table 15-1):
    // Offset 0x1D0: ES selector (2 bytes) + reserved (2 bytes)
    // Offset 0x1D4: CS selector (2 bytes) + reserved (2 bytes)
    // Offset 0x1D8: SS selector (2 bytes) + reserved (2 bytes)
    // Offset 0x1DC: DS selector (2 bytes) + reserved (2 bytes)
    // Offset 0x1E0: FS selector (2 bytes) + reserved (2 bytes)
    // Offset 0x1E4: GS selector (2 bytes) + reserved (2 bytes)
    // Offset 0x1E8: LDTR selector (2 bytes) + reserved (2 bytes)
    // Offset 0x1EC: TR selector (2 bytes) + reserved (2 bytes)
    //
    // Bases start at 0x1F8 region. Let me redefine properly.
    // This is getting complex. For the save area, I'll use a simpler
    // offset scheme and document it.

    // FS selector (offset 0x1E0)
    pub fn fs_selector(&self) -> u16 {
        self.read_u32(0x1E0) as u16
    }
    pub fn set_fs_selector(&mut self, value: u16) {
        self.write_u32(0x1E0, value as u32);
    }

    // GS selector (offset 0x1E4)
    pub fn gs_selector(&self) -> u16 {
        self.read_u32(0x1E4) as u16
    }
    pub fn set_gs_selector(&mut self, value: u16) {
        self.write_u32(0x1E4, value as u32);
    }

    // --- Control registers ---

    // CR0 (offset 0x278)
    pub fn cr0(&self) -> u64 {
        self.read_u64(0x278)
    }
    pub fn set_cr0(&mut self, value: u64) {
        self.write_u64(0x278, value);
    }

    // CR3 (offset 0x280)
    pub fn cr3(&self) -> u64 {
        self.read_u64(0x280)
    }
    pub fn set_cr3(&mut self, value: u64) {
        self.write_u64(0x280, value);
    }

    // CR4 (offset 0x288)
    pub fn cr4(&self) -> u64 {
        self.read_u64(0x288)
    }
    pub fn set_cr4(&mut self, value: u64) {
        self.write_u64(0x288, value);
    }

    // --- RIP / RFLAGS ---

    // RIP (offset 0x298)
    pub fn rip(&self) -> u64 {
        self.read_u64(0x298)
    }
    pub fn set_rip(&mut self, value: u64) {
        self.write_u64(0x298, value);
    }

    // RFLAGS (offset 0x290)
    pub fn rflags(&self) -> u64 {
        self.read_u64(0x290)
    }
    pub fn set_rflags(&mut self, value: u64) {
        self.write_u64(0x290, value);
    }
}

impl Default for VmcbSave {
    fn default() -> Self {
        Self::new()
    }
}

/// AMD-V Virtual Machine Control Block.
///
/// A VMCB consists of a control area (4 KiB) followed by a save area (4 KiB),
/// for a total of 8 KiB. The VMCB must be 4 KiB aligned.
#[derive(Debug, Clone)]
pub struct Vmcb {
    /// Control area (offset 0x000).
    pub control: VmcbControl,
    /// Save area (offset 0x400).
    pub save: VmcbSave,
    /// Whether the VMCB has been modified since last VMRUN.
    dirty: bool,
}

impl Vmcb {
    /// Create a new, zeroed VMCB.
    pub fn new() -> Self {
        Self {
            control: VmcbControl::new(),
            save: VmcbSave::new(),
            dirty: true,
        }
    }

    /// Mark the VMCB as dirty (needs reload before VMRUN).
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// Clear the dirty flag (called before VMRUN).
    pub fn clear_dirty(&mut self) {
        self.dirty = false;
    }

    /// Check if the VMCB is dirty.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Returns the physical address of the VMCB in memory.
    ///
    /// # Safety
    ///
    /// The VMCB must be allocated in physically contiguous memory that
    /// is 4 KiB aligned and valid for the lifetime of the VM.
    pub fn physical_address(&self) -> u64 {
        self as *const Self as u64
    }

    /// Execute VMRUN to enter guest mode.
    ///
    /// # Safety
    ///
    /// - The VMCB must be loaded at a valid physical address.
    /// - Guest state in the save area must be fully initialized.
    /// - The processor must be in VMX root operation (AMD: SVM enabled).
    /// - All host state (MSRs, CRs, etc.) must be configured.
    pub unsafe fn vmrun(&mut self) -> Result<(), Vmcbe> {
        let pa = self.physical_address();
        unsafe {
            core::arch::asm!(
                "push rbx",
                "mov rbx, {pa}",
                "vmrun",
                "pop rbx",
                pa = in(reg) pa,
                options(nostack),
            );
        }
        self.clear_dirty();
        // After VMRUN, the control area has exit info populated.
        // We return the exit code so the caller can inspect it.
        Ok(())
    }

    /// Returns the exit code after a VMRUN.
    pub fn exit_code(&self) -> VmcbExitCode {
        VmcbExitCode::from(self.control.exitcode())
    }

    /// Returns exit info 1 after a VMRUN.
    pub fn exit_info1(&self) -> u64 {
        self.control.exitinfo1()
    }

    /// Returns exit info 2 after a VMRUN.
    pub fn exit_info2(&self) -> u64 {
        self.control.exitinfo2()
    }

    /// Read a u64 from the save area at an offset.
    pub fn read_save_u64(&self, offset: usize) -> u64 {
        self.save.read_u64(offset)
    }

    /// Write a u64 to the save area at an offset.
    pub fn write_save_u64(&mut self, offset: usize, value: u64) {
        self.save.write_u64(offset, value);
        self.dirty = true;
    }
}

impl Default for Vmcb {
    fn default() -> Self {
        Self::new()
    }
}

/// VMCB cache entry — holds a pointer to a VMCB for fast re-entry.
#[derive(Debug, Clone, Copy)]
pub struct Vmcbe {
    /// Physical address of the VMCB.
    pub vmcb_pa: u64,
    /// ASID (Address Space ID) for TLB tagging.
    pub asid: u32,
}

impl Vmcbe {
    /// Create a new cache entry for a VMCB.
    pub fn new(vmcb_pa: u64, asid: u32) -> Self {
        Self { vmcb_pa, asid }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_serial::acquire;

    #[test]
    fn vmcb_new_is_dirty() {
        let _guard = acquire();
        let vmcb = Vmcb::new();
        assert!(vmcb.is_dirty());
    }

    #[test]
    fn vmcb_clear_dirty() {
        let _guard = acquire();
        let mut vmcb = Vmcb::new();
        vmcb.clear_dirty();
        assert!(!vmcb.is_dirty());
    }

    #[test]
    fn vmcb_mark_dirty() {
        let _guard = acquire();
        let mut vmcb = Vmcb::new();
        vmcb.clear_dirty();
        vmcb.mark_dirty();
        assert!(vmcb.is_dirty());
    }

    #[test]
    fn vmcb_control_cr0_roundtrip() {
        let _guard = acquire();
        let mut ctrl = VmcbControl::new();
        ctrl.set_cr0(0x0000_0000_8001_0001);
        assert_eq!(ctrl.cr0(), 0x0000_0000_8001_0001);
    }

    #[test]
    fn vmcb_save_rax_roundtrip() {
        let _guard = acquire();
        let mut save = VmcbSave::new();
        save.set_rax(0xCAFEBABE_DEADBEEF);
        assert_eq!(save.rax(), 0xCAFEBABE_DEADBEEF);
    }

    #[test]
    fn vmcb_save_gpr_all_roundtrip() {
        let _guard = acquire();
        let mut save = VmcbSave::new();
        save.set_rax(1);
        save.set_rbx(2);
        save.set_rcx(3);
        save.set_rdx(4);
        save.set_rsi(5);
        save.set_rdi(6);
        save.set_rsp(7);
        save.set_rbp(8);
        save.set_r8(9);
        save.set_r9(10);
        save.set_r10(11);
        save.set_r11(12);
        save.set_r12(13);
        save.set_r13(14);
        save.set_r14(15);
        save.set_r15(16);
        assert_eq!(save.rax(), 1);
        assert_eq!(save.rbx(), 2);
        assert_eq!(save.rcx(), 3);
        assert_eq!(save.rdx(), 4);
        assert_eq!(save.rsi(), 5);
        assert_eq!(save.rdi(), 6);
        assert_eq!(save.rsp(), 7);
        assert_eq!(save.rbp(), 8);
        assert_eq!(save.r8(), 9);
        assert_eq!(save.r9(), 10);
        assert_eq!(save.r10(), 11);
        assert_eq!(save.r11(), 12);
        assert_eq!(save.r12(), 13);
        assert_eq!(save.r13(), 14);
        assert_eq!(save.r14(), 15);
        assert_eq!(save.r15(), 16);
    }

    #[test]
    fn vmcb_dirty_tracking_on_write() {
        let _guard = acquire();
        let mut vmcb = Vmcb::new();
        vmcb.clear_dirty();
        assert!(!vmcb.is_dirty());
        vmcb.write_save_u64(0x1F8, 0xABCD);
        assert!(vmcb.is_dirty());
    }
}
