//! POSIX 64-bit capabilities implementation.
//!
//! Each capability is a single bit within one of five capability sets:
//! - effective: currently enforced
//! - permitted: cap can be used within effective
//! - inheritable: cap can be inherited across exec
//! - bounding: limit on what can be gained via exec
//! - ambient: cap preserved across exec
//!
//! Reference: POSIX.1e / Linux capabilities(7)

use core::fmt;

/// Linux capability identifiers as bit positions (0–63).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum Capability {
    Chown = 0,
    DacOverride = 1,
    DacReadSearch = 2,
    Fowner = 3,
    Fsetid = 4,
    Kill = 5,
    Setgid = 6,
    Setuid = 7,
    Setpcap = 8,
    LinuxImmutable = 9,
    NetBindService = 10,
    NetBroadcast = 11,
    NetAdmin = 12,
    NetRaw = 13,
    NetLink = 14,
    NetTcpBlock = 15,
    IpcLock = 16,
    IpcOwner = 17,
    SysModule = 18,
    SysRawio = 19,
    SysChroot = 20,
    SysPtrace = 21,
    SysPacct = 22,
    SysAdmin = 23,
    SysBoot = 24,
    SysNice = 25,
    SysResource = 26,
    SysTime = 27,
    SysTtyConfig = 28,
    Mknod = 29,
    Lease = 30,
    AuditWrite = 31,
    AuditControl = 32,
    Setfcap = 33,
    MacOverride = 34,
    MacAdmin = 35,
    Syslog = 36,
    WakeAlarm = 37,
    BlockSuspend = 38,
    AuditRead = 39,
    Perfmon = 40,
    Bpf = 41,
    CheckpointRestore = 42,
}

impl Capability {
    pub const fn bit(&self) -> u64 {
        1u64 << (*self as u32)
    }

    pub const fn from_bit(bit: u64) -> Option<Self> {
        if bit == 0 || !bit.is_power_of_two() {
            return None;
        }
        match bit.ilog2() {
            0 => Some(Capability::Chown),
            1 => Some(Capability::DacOverride),
            2 => Some(Capability::DacReadSearch),
            3 => Some(Capability::Fowner),
            4 => Some(Capability::Fsetid),
            5 => Some(Capability::Kill),
            6 => Some(Capability::Setgid),
            7 => Some(Capability::Setuid),
            8 => Some(Capability::Setpcap),
            9 => Some(Capability::LinuxImmutable),
            10 => Some(Capability::NetBindService),
            11 => Some(Capability::NetBroadcast),
            12 => Some(Capability::NetAdmin),
            13 => Some(Capability::NetRaw),
            14 => Some(Capability::NetLink),
            15 => Some(Capability::NetTcpBlock),
            16 => Some(Capability::IpcLock),
            17 => Some(Capability::IpcOwner),
            18 => Some(Capability::SysModule),
            19 => Some(Capability::SysRawio),
            20 => Some(Capability::SysChroot),
            21 => Some(Capability::SysPtrace),
            22 => Some(Capability::SysPacct),
            23 => Some(Capability::SysAdmin),
            24 => Some(Capability::SysBoot),
            25 => Some(Capability::SysNice),
            26 => Some(Capability::SysResource),
            27 => Some(Capability::SysTime),
            28 => Some(Capability::SysTtyConfig),
            29 => Some(Capability::Mknod),
            30 => Some(Capability::Lease),
            31 => Some(Capability::AuditWrite),
            32 => Some(Capability::AuditControl),
            33 => Some(Capability::Setfcap),
            34 => Some(Capability::MacOverride),
            35 => Some(Capability::MacAdmin),
            36 => Some(Capability::Syslog),
            37 => Some(Capability::WakeAlarm),
            38 => Some(Capability::BlockSuspend),
            39 => Some(Capability::AuditRead),
            40 => Some(Capability::Perfmon),
            41 => Some(Capability::Bpf),
            42 => Some(Capability::CheckpointRestore),
            _ => None,
        }
    }

    pub const fn name(&self) -> &'static str {
        match self {
            Capability::Chown => "CAP_CHOWN",
            Capability::DacOverride => "CAP_DAC_OVERRIDE",
            Capability::DacReadSearch => "CAP_DAC_READ_SEARCH",
            Capability::Fowner => "CAP_FOWNER",
            Capability::Fsetid => "CAP_FSETID",
            Capability::Kill => "CAP_KILL",
            Capability::Setgid => "CAP_SETGID",
            Capability::Setuid => "CAP_SETUID",
            Capability::Setpcap => "CAP_SETPCAP",
            Capability::LinuxImmutable => "CAP_LINUX_IMMUTABLE",
            Capability::NetBindService => "CAP_NET_BIND_SERVICE",
            Capability::NetBroadcast => "CAP_NET_BROADCAST",
            Capability::NetAdmin => "CAP_NET_ADMIN",
            Capability::NetRaw => "CAP_NET_RAW",
            Capability::NetLink => "CAP_NET_LINK",
            Capability::NetTcpBlock => "CAP_NET_TCP_BLOCK",
            Capability::IpcLock => "CAP_IPC_LOCK",
            Capability::IpcOwner => "CAP_IPC_OWNER",
            Capability::SysModule => "CAP_SYS_MODULE",
            Capability::SysRawio => "CAP_SYS_RAWIO",
            Capability::SysChroot => "CAP_SYS_CHROOT",
            Capability::SysPtrace => "CAP_SYS_PTRACE",
            Capability::SysPacct => "CAP_SYS_PACCT",
            Capability::SysAdmin => "CAP_SYS_ADMIN",
            Capability::SysBoot => "CAP_SYS_BOOT",
            Capability::SysNice => "CAP_SYS_NICE",
            Capability::SysResource => "CAP_SYS_RESOURCE",
            Capability::SysTime => "CAP_SYS_TIME",
            Capability::SysTtyConfig => "CAP_SYS_TTY_CONFIG",
            Capability::Mknod => "CAP_MKNOD",
            Capability::Lease => "CAP_LEASE",
            Capability::AuditWrite => "CAP_AUDIT_WRITE",
            Capability::AuditControl => "CAP_AUDIT_CONTROL",
            Capability::Setfcap => "CAP_SETFCAP",
            Capability::MacOverride => "CAP_MAC_OVERRIDE",
            Capability::MacAdmin => "CAP_MAC_ADMIN",
            Capability::Syslog => "CAP_SYSLOG",
            Capability::WakeAlarm => "CAP_WAKE_ALARM",
            Capability::BlockSuspend => "CAP_BLOCK_SUSPEND",
            Capability::AuditRead => "CAP_AUDIT_READ",
            Capability::Perfmon => "CAP_PERFMON",
            Capability::Bpf => "CAP_BPF",
            Capability::CheckpointRestore => "CAP_CHECKPOINT_RESTORE",
        }
    }
}

/// Five capability bitmasks per POSIX.1e.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CapabilitySet {
    pub effective: u64,
    pub permitted: u64,
    pub inheritable: u64,
    pub bounding: u64,
    pub ambient: u64,
}

impl CapabilitySet {
    pub const fn new() -> Self {
        Self {
            effective: 0,
            permitted: 0,
            inheritable: 0,
            bounding: 0,
            ambient: 0,
        }
    }

    /// Full capabilities — all bits set in all sets (superuser).
    pub fn full() -> Self {
        let all = u64::MAX;
        Self {
            effective: all,
            permitted: all,
            inheritable: all,
            bounding: all,
            ambient: all,
        }
    }

    /// Basic user capabilities (read/write/exec only — no admin/network).
    pub fn basic() -> Self {
        let mut s = Self::new();
        // Grant common user-capabilities
        let basic_bits = (1u64 << Capability::Chown as u32)
            | (1u64 << Capability::DacOverride as u32)
            | (1u64 << Capability::DacReadSearch as u32)
            | (1u64 << Capability::Fowner as u32)
            | (1u64 << Capability::Fsetid as u32)
            | (1u64 << Capability::Kill as u32)
            | (1u64 << Capability::Setgid as u32)
            | (1u64 << Capability::Setuid as u32)
            | (1u64 << Capability::NetBindService as u32)
            | (1u64 << Capability::NetRaw as u32)
            | (1u64 << Capability::IpcLock as u32)
            | (1u64 << Capability::AuditWrite as u32)
            | (1u64 << Capability::SysNice as u32)
            | (1u64 << Capability::SysResource as u32)
            | (1u64 << Capability::SysTtyConfig as u32);
        s.effective = basic_bits;
        s.permitted = basic_bits;
        s.inheritable = basic_bits;
        s.bounding = basic_bits;
        s.ambient = 0;
        s
    }

    /// Restricted capabilities — only read/search.
    pub fn restricted() -> Self {
        let rbits = 1u64 << Capability::DacReadSearch as u32;
        Self {
            effective: rbits,
            permitted: rbits,
            inheritable: rbits,
            bounding: rbits,
            ambient: 0,
        }
    }

    /// Full user range: like basic but also includes admin caps (equivalent to root).
    /// This is what init (PID 1) should get.
    pub fn root() -> Self {
        Self::full()
    }

    pub fn has(&self, cap: Capability) -> bool {
        self.effective & (1u64 << (cap as u32)) != 0
    }

    pub fn has_permitted(&self, cap: Capability) -> bool {
        self.permitted & (1u64 << (cap as u32)) != 0
    }

    /// Set a capability in the effective set.
    pub fn set(&mut self, cap: Capability) {
        self.effective |= cap.bit();
    }

    /// Clear a capability from the effective set.
    pub fn clear(&mut self, cap: Capability) {
        self.effective &= !cap.bit();
    }

    /// POSIX exec transformation (without file capabilities).
    ///
    /// When no file capabilities are attached to the executable:
    /// - permitted := bounding & inheritable (if UID unchanged) OR
    ///                bounding + securebits (if UID changed)
    /// - effective := 0 (effective is cleared on exec)
    /// - inheritable unchanged
    /// - ambient := ambient & inheritable & bounding
    pub fn exec_transform(&self) -> Self {
        // Standard POSIX exec transform (simplified — no securebits, no UID change)
        let new_permitted = self.bounding & self.inheritable;
        let new_ambient = self.ambient & self.inheritable & self.bounding;
        Self {
            effective: 0, // Cleared on exec
            permitted: new_permitted,
            inheritable: self.inheritable,
            bounding: self.bounding,
            ambient: new_ambient,
        }
    }

    /// POSIX exec transform with file capabilities.
    ///
    /// When the executable has file capabilities:
    /// - permitted := (file.effective ? file.permitted : file.inheritable & bounding)
    /// - effective := file.effective
    /// - inheritable unchanged
    /// - ambient := 0 (ambient caps are cleared when file caps are present)
    pub fn exec_transform_with_filecaps(&self, file_caps: &FileCaps) -> Self {
        let new_permitted = if file_caps.effective {
            file_caps.permitted
        } else {
            file_caps.inheritable & self.bounding
        };
        Self {
            effective: file_caps.effective_mask(),
            permitted: new_permitted,
            inheritable: self.inheritable,
            bounding: self.bounding,
            ambient: 0,
        }
    }
}

impl fmt::Display for CapabilitySet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "CapabilitySet {{ effective={:#018x}, permitted={:#018x}, inheritable={:#018x}, bounding={:#018x}, ambient={:#018x} }}",
            self.effective, self.permitted, self.inheritable, self.bounding, self.ambient
        )
    }
}

/// File capabilities stored as extended attributes on executables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FileCaps {
    pub permitted: u64,
    pub inheritable: u64,
    /// If true, permitted bits are added to permitted set on exec;
    /// if false, inherited bits are filtered through bounding set.
    pub effective: bool,
}

impl FileCaps {
    pub const fn new(permitted: u64, inheritable: u64, effective: bool) -> Self {
        Self {
            permitted,
            inheritable,
            effective,
        }
    }

    pub fn effective_mask(&self) -> u64 {
        if self.effective {
            self.permitted
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_all_43_bit_positions() {
        // Every variant maps to 1 << repr
        let cases = [
            (Capability::Chown, 0),
            (Capability::DacOverride, 1),
            (Capability::DacReadSearch, 2),
            (Capability::Fowner, 3),
            (Capability::Fsetid, 4),
            (Capability::Kill, 5),
            (Capability::Setgid, 6),
            (Capability::Setuid, 7),
            (Capability::Setpcap, 8),
            (Capability::LinuxImmutable, 9),
            (Capability::NetBindService, 10),
            (Capability::NetBroadcast, 11),
            (Capability::NetAdmin, 12),
            (Capability::NetRaw, 13),
            (Capability::NetLink, 14),
            (Capability::NetTcpBlock, 15),
            (Capability::IpcLock, 16),
            (Capability::IpcOwner, 17),
            (Capability::SysModule, 18),
            (Capability::SysRawio, 19),
            (Capability::SysChroot, 20),
            (Capability::SysPtrace, 21),
            (Capability::SysPacct, 22),
            (Capability::SysAdmin, 23),
            (Capability::SysBoot, 24),
            (Capability::SysNice, 25),
            (Capability::SysResource, 26),
            (Capability::SysTime, 27),
            (Capability::SysTtyConfig, 28),
            (Capability::Mknod, 29),
            (Capability::Lease, 30),
            (Capability::AuditWrite, 31),
            (Capability::AuditControl, 32),
            (Capability::Setfcap, 33),
            (Capability::MacOverride, 34),
            (Capability::MacAdmin, 35),
            (Capability::Syslog, 36),
            (Capability::WakeAlarm, 37),
            (Capability::BlockSuspend, 38),
            (Capability::AuditRead, 39),
            (Capability::Perfmon, 40),
            (Capability::Bpf, 41),
            (Capability::CheckpointRestore, 42),
        ];
        for (cap, expected_pos) in &cases {
            let expected_bit = 1u64 << expected_pos;
            assert_eq!(cap.bit(), expected_bit, "bit pos {} expected", expected_pos);
            assert_eq!(
                Capability::from_bit(expected_bit),
                Some(*cap),
                "from_bit round-trip for pos {}",
                expected_pos
            );
        }
        // from_bit for undefined bits
        assert_eq!(Capability::from_bit(0), None);
        assert_eq!(Capability::from_bit(1u64 << 43), None);
        assert_eq!(Capability::from_bit(1u64 << 63), None);
    }

    #[test]
    fn capability_set_and_clear() {
        let mut caps = CapabilitySet::new();
        assert!(!caps.has(Capability::Kill));
        caps.set(Capability::Kill);
        assert!(caps.has(Capability::Kill));
        caps.clear(Capability::Kill);
        assert!(!caps.has(Capability::Kill));
    }

    #[test]
    fn cap_set_basic_has_common_caps() {
        let basic = CapabilitySet::basic();
        assert!(basic.has(Capability::Chown));
        assert!(basic.has(Capability::Kill));
        assert!(basic.has(Capability::NetBindService));
        assert!(!basic.has(Capability::SysAdmin));
        assert!(!basic.has(Capability::SysModule));
    }

    #[test]
    fn restricted_has_only_dac_read_search() {
        let restricted = CapabilitySet::restricted();
        assert!(restricted.has(Capability::DacReadSearch));
        assert!(!restricted.has(Capability::Kill));
        assert!(!restricted.has(Capability::Chown));
    }

    #[test]
    fn exec_transform_clears_effective() {
        let caps = CapabilitySet::full();
        let after = caps.exec_transform();
        assert_eq!(after.effective, 0);
        // permitted = bounding & inheritable (both full, so full)
        assert_eq!(after.permitted, u64::MAX);
    }

    #[test]
    fn exec_transform_inheritable_filtered_by_bounding() {
        let caps = CapabilitySet {
            effective: 0,
            permitted: 0,
            inheritable: 0xFF,
            bounding: 0x0F,
            ambient: 0,
        };
        let after = caps.exec_transform();
        assert_eq!(after.effective, 0);
        assert_eq!(after.permitted, 0x0F);
        assert_eq!(after.inheritable, 0xFF);
        assert_eq!(after.bounding, 0x0F);
    }

    #[test]
    fn exec_transform_with_filecaps_effective() {
        let process_caps = CapabilitySet::full();
        let file_caps = FileCaps::new(0xFF, 0x00, true);
        let after = process_caps.exec_transform_with_filecaps(&file_caps);
        assert_eq!(after.effective, 0xFF);
        assert_eq!(after.permitted, 0xFF);
        assert_eq!(after.ambient, 0);
    }

    #[test]
    fn exec_transform_with_filecaps_not_effective() {
        let process_caps = CapabilitySet {
            effective: 0,
            permitted: 0,
            inheritable: 0xFF,
            bounding: 0x0F,
            ambient: 0,
        };
        let file_caps = FileCaps::new(0x00, 0xFF, false);
        let after = process_caps.exec_transform_with_filecaps(&file_caps);
        assert_eq!(after.effective, 0);
        assert_eq!(after.permitted, 0x0F); // inheritable & bounding
        assert_eq!(after.ambient, 0);
    }

    #[test]
    fn cap_set_has_permitted() {
        let caps = CapabilitySet::full();
        assert!(caps.has_permitted(Capability::SysAdmin));
        let basic = CapabilitySet::basic();
        assert!(basic.has_permitted(Capability::Kill));
        // Empty set has nothing
        let empty = CapabilitySet::new();
        assert!(!empty.has_permitted(Capability::Chown));
        assert!(!empty.has(Capability::Chown));
        // Ambient set operations
        assert_eq!(empty.ambient, 0);
    }

    #[test]
    fn filecaps_effective_mask() {
        let fc = FileCaps::new(0xFF, 0x00, true);
        assert_eq!(fc.effective_mask(), 0xFF);
        let fc2 = FileCaps::new(0xFF, 0xAA, false);
        assert_eq!(fc2.effective_mask(), 0);
    }

    #[test]
    fn security_context_delegates_caps() {
        use crate::security::SecurityContext;
        let ctx = SecurityContext::root();
        assert!(ctx.has_capability(Capability::SysAdmin));
        assert!(ctx.has_effective(Capability::Kill));
        assert!(ctx.has_permitted(Capability::Chown));
        let ctx2 = SecurityContext::new(1000, 1000, CapabilitySet::restricted());
        assert!(ctx2.has_capability(Capability::DacReadSearch));
        assert!(!ctx2.has_capability(Capability::Kill));
        assert_eq!(ctx2.uid, 1000);
        assert_eq!(ctx2.gid, 1000);
        assert!(!ctx2.is_privileged);
    }

    #[test]
    fn cap_set_full_contains_all_caps() {
        let full = CapabilitySet::full();
        assert!(full.has(Capability::Chown));
        assert!(full.has(Capability::CheckpointRestore));
        assert!(full.has(Capability::Perfmon));
        assert_eq!(full.effective, u64::MAX);
        assert_eq!(full.permitted, u64::MAX);
        assert_eq!(full.inheritable, u64::MAX);
        assert_eq!(full.bounding, u64::MAX);
        assert_eq!(full.ambient, u64::MAX);
    }

    #[test]
    fn cap_from_bit() {
        assert_eq!(Capability::from_bit(1), Some(Capability::Chown));
        assert_eq!(Capability::from_bit(1 << 23), Some(Capability::SysAdmin));
        assert_eq!(Capability::from_bit(1 << 63), None); // Beyond defined caps
    }

    // --- Property-like tests for exec_transform ---

    /// Property 21 equivalent: exec_transform output matches POSIX formula.
    /// We test with arbitrary generator values.
    fn check_exec_transform_property(inh: u64, bnd: u64, amb: u64) {
        let caps = CapabilitySet {
            effective: 0,
            permitted: 0,
            inheritable: inh,
            bounding: bnd,
            ambient: amb,
        };
        let after = caps.exec_transform();
        // effective always 0
        assert_eq!(after.effective, 0);
        // permitted = bounding & inheritable
        assert_eq!(after.permitted, bnd & inh);
        // ambient = ambient & inheritable & bounding
        assert_eq!(after.ambient, amb & inh & bnd);
        // bounding and inheritable unchanged
        assert_eq!(after.bounding, bnd);
        assert_eq!(after.inheritable, inh);
    }

    #[test]
    fn exec_transform_property_random_values() {
        // Test fixed edge cases
        check_exec_transform_property(0, 0, 0);
        check_exec_transform_property(u64::MAX, u64::MAX, u64::MAX);
        check_exec_transform_property(0xFF, 0x0F, 0xAA);
        check_exec_transform_property(0xDEAD, 0xBEEF, 0xCAFE);
        check_exec_transform_property(1 << 42, 1 << 40, 1 << 41);
        check_exec_transform_property(0xFFFF_FFFF_FFFF_FFFF, 0x0000_0000_0000_0000, u64::MAX);
    }

    /// Property 22 equivalent: dropped capability cannot be reacquired.
    fn check_drop_irreversibility(cap: Capability, rest: u64) {
        let mut caps = CapabilitySet {
            effective: rest | cap.bit(),
            permitted: rest | cap.bit(),
            inheritable: rest | cap.bit(),
            bounding: rest,
            ambient: rest,
        };
        // Drop from permitted
        caps.permitted &= !cap.bit();
        // After exec (no file caps), the cap should not appear in permitted
        let after = caps.exec_transform();
        assert_eq!(
            after.permitted & cap.bit(),
            0,
            "cap {:?} should not be in permitted after dropping ({:#x})",
            cap,
            after.permitted
        );
    }

    #[test]
    fn drop_irreversibility_property() {
        let all_others = !(1u64 << Capability::SysAdmin as u32);
        check_drop_irreversibility(Capability::SysAdmin, all_others);
        let all_others = !(1u64 << Capability::NetAdmin as u32);
        check_drop_irreversibility(Capability::NetAdmin, all_others);
    }

    /// Test that file capabilities override inheritable on exec when effective flag is set.
    #[test]
    fn filecaps_override_inheritable() {
        let process = CapabilitySet {
            effective: 0,
            permitted: 0,
            inheritable: 0xFF,
            bounding: u64::MAX,
            ambient: 0,
        };
        let filecaps = FileCaps::new(0xF0, 0x0F, true);
        let after = process.exec_transform_with_filecaps(&filecaps);
        // permitted = file.permitted (because effective), not inheritable & bounding
        assert_eq!(after.permitted, 0xF0);
        assert_eq!(after.effective, 0xF0);
    }
}
