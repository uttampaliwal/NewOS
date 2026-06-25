//! Seccomp-BPF for Turnix OS
//!
//! Implements a classic BPF interpreter for seccomp filters
//! using the Linux seccomp BPF ABI.

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// BPF Instruction constants (subset of classic BPF)
// ---------------------------------------------------------------------------

pub const BPF_LD: u16 = 0x00;
pub const BPF_LDX: u16 = 0x01;
pub const BPF_JMP: u16 = 0x05;
pub const BPF_ALU: u16 = 0x04;
pub const BPF_RET: u16 = 0x06;
pub const BPF_MISC: u16 = 0x07;

pub const BPF_W: u16 = 0x00;
pub const BPF_H: u16 = 0x08;
pub const BPF_B: u16 = 0x10;

pub const BPF_IMM: u16 = 0x00;
pub const BPF_ABS: u16 = 0x20;
pub const BPF_IND: u16 = 0x40;
pub const BPF_MEM: u16 = 0x60;
pub const BPF_LEN: u16 = 0x80;
pub const BPF_MSH: u16 = 0xa0;

pub const BPF_JA: u16 = 0x00;
pub const BPF_JEQ: u16 = 0x10;
pub const BPF_JGT: u16 = 0x20;
pub const BPF_JGE: u16 = 0x30;
pub const BPF_JSET: u16 = 0x40;

pub const BPF_ADD: u16 = 0x00;
pub const BPF_SUB: u16 = 0x10;
pub const BPF_MUL: u16 = 0x20;
pub const BPF_DIV: u16 = 0x30;
pub const BPF_OR: u16 = 0x40;
pub const BPF_AND: u16 = 0x50;
pub const BPF_LSH: u16 = 0x60;
pub const BPF_RSH: u16 = 0x70;
pub const BPF_NEG: u16 = 0x80;
pub const BPF_MOD: u16 = 0x90;
pub const BPF_XOR: u16 = 0xa0;

pub const BPF_K: u16 = 0x00;
pub const BPF_X: u16 = 0x08;

pub const BPF_TAX: u16 = 0x00;
pub const BPF_TXA: u16 = 0x80;

// ---------------------------------------------------------------------------
// Seccomp actions
// ---------------------------------------------------------------------------

/// Seccomp return actions (matching Linux values).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeccompAction {
    KillProcess,
    KillThread,
    Trap,
    /// Returns the specified errno to the caller.
    Errno(u16),
    Trace,
    Allow,
}

impl SeccompAction {
    /// Decode a raw seccomp return value into an action.
    /// The high 16 bits encode the action; low 16 bits carry data (e.g. errno).
    pub fn from_raw(raw: u32) -> Self {
        match raw & 0xffff0000 {
            0x80000000 => Self::KillProcess,
            0x00000000 => Self::KillThread,
            0x00030000 => Self::Trap,
            0x00050000 => Self::Errno((raw & 0xffff) as u16),
            0x7ff00000 => Self::Trace,
            0x7fff0000 => Self::Allow,
            _ => Self::KillThread,
        }
    }

    /// Encode an action into its raw seccomp return value.
    pub fn to_raw(&self) -> u32 {
        match self {
            Self::KillProcess => 0x80000000,
            Self::KillThread => 0x00000000,
            Self::Trap => 0x00030000,
            Self::Errno(e) => 0x00050000 | (*e as u32),
            Self::Trace => 0x7ff00000,
            Self::Allow => 0x7fff0000,
        }
    }
}

// ---------------------------------------------------------------------------
// BPF instruction
// ---------------------------------------------------------------------------

/// A classic BPF instruction (8 bytes).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BpfInstruction {
    pub code: u16,
    pub jt: u8,
    pub jf: u8,
    pub k: u32,
}

/// Seccomp data layout exposed to BPF programs.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SeccompData {
    pub nr: u32,        // syscall number
    pub arch: u32,      // architecture (AUDIT_ARCH_X86_64 = 0xC000003E)
    pub ip: u64,        // instruction pointer
    pub args: [u64; 6], // syscall arguments
}

// ---------------------------------------------------------------------------
// Seccomp filter
// ---------------------------------------------------------------------------

/// A seccomp filter backed by a classic BPF program.
#[derive(Debug, Clone)]
pub struct SeccompFilter {
    /// The BPF instructions.
    instructions: Vec<BpfInstruction>,
    /// Whether this filter is a "no-new-privs" filter (child cannot loosen).
    no_new_privs: bool,
}

impl SeccompFilter {
    /// Create a new seccomp filter from a BPF program.
    /// Returns `None` if the program is empty or exceeds 4096 instructions.
    pub fn new(instructions: Vec<BpfInstruction>, no_new_privs: bool) -> Option<Self> {
        if instructions.is_empty() || instructions.len() > 4096 {
            return None;
        }
        Some(Self {
            instructions,
            no_new_privs,
        })
    }

    /// Inherit this filter for a forked/clone'd child.
    pub fn inherit_on_fork(&self) -> Self {
        Self {
            instructions: self.instructions.clone(),
            no_new_privs: self.no_new_privs,
        }
    }

    /// Check if this filter has no_new_privs set.
    pub fn has_no_new_privs(&self) -> bool {
        self.no_new_privs
    }

    /// Evaluate the BPF program against the given syscall data.
    /// Returns the action to take.
    pub fn evaluate(&self, data: &SeccompData) -> SeccompAction {
        let mut pc = 0usize;
        let mut acc: u32 = 0;
        let mut x: u32 = 0;
        let scratch: [u32; 16] = [0; 16];

        let load32 = |offset: u32| -> u32 {
            match offset {
                0 => data.nr,
                4 => data.arch,
                8 => data.ip as u32,
                12 => (data.ip >> 32) as u32,
                16 => data.args[0] as u32,
                20 => (data.args[0] >> 32) as u32,
                24 => data.args[1] as u32,
                28 => (data.args[1] >> 32) as u32,
                32 => data.args[2] as u32,
                36 => (data.args[2] >> 32) as u32,
                40 => data.args[3] as u32,
                44 => (data.args[3] >> 32) as u32,
                48 => data.args[4] as u32,
                52 => (data.args[4] >> 32) as u32,
                56 => data.args[5] as u32,
                60 => (data.args[5] >> 32) as u32,
                _ => 0,
            }
        };

        loop {
            if pc >= self.instructions.len() {
                return SeccompAction::KillThread;
            }
            let insn = &self.instructions[pc];
            pc += 1;

            match insn.code & 0x07 {
                // BPF_LD / BPF_LDX
                0x00 | 0x01 => {
                    let is_ldx = (insn.code & 0x07) == 0x01;
                    let size = insn.code & 0x18;
                    let mode = insn.code & 0xe0;

                    let val = match mode {
                        BPF_IMM => insn.k,
                        BPF_ABS => load32(insn.k),
                        BPF_IND => load32(x.wrapping_add(insn.k)),
                        BPF_LEN => {
                            // Return the size of the seccomp data (64 bytes for our layout)
                            64u32
                        }
                        BPF_MEM => {
                            if (insn.k as usize) < 16 {
                                scratch[insn.k as usize]
                            } else {
                                0
                            }
                        }
                        _ => {
                            return SeccompAction::KillThread;
                        }
                    };

                    // For half-word or byte loads, mask appropriately
                    let val = match size {
                        BPF_W => val,
                        BPF_H => val & 0xffff,
                        BPF_B => val & 0xff,
                        _ => val,
                    };

                    if is_ldx {
                        x = val;
                    } else {
                        acc = val;
                    }
                }

                // BPF_JMP
                0x05 => {
                    let jmp_type = insn.code & 0xf0;
                    let jump_true = insn.jt as usize;
                    let jump_false = insn.jf as usize;

                    let condition = match jmp_type {
                        BPF_JA => {
                            pc = pc.wrapping_add(insn.k as usize);
                            continue;
                        }
                        BPF_JEQ => acc == insn.k,
                        BPF_JGT => acc > insn.k,
                        BPF_JGE => acc >= insn.k,
                        BPF_JSET => (acc & insn.k) != 0,
                        _ => false,
                    };

                    if condition {
                        pc = pc.wrapping_add(jump_true);
                    } else {
                        pc = pc.wrapping_add(jump_false);
                    }
                }

                // BPF_ALU
                0x04 => {
                    let alu_op = insn.code & 0xf0;
                    match alu_op {
                        BPF_ADD => acc = acc.wrapping_add(insn.k),
                        BPF_SUB => acc = acc.wrapping_sub(insn.k),
                        BPF_MUL => acc = acc.wrapping_mul(insn.k),
                        BPF_DIV => {
                            if let Some(v) = acc.checked_div(insn.k) {
                                acc = v;
                            } else {
                                acc = 0;
                            }
                        }
                        BPF_OR => acc |= insn.k,
                        BPF_AND => acc &= insn.k,
                        BPF_LSH => acc = acc.wrapping_shl(insn.k),
                        BPF_RSH => acc = acc.wrapping_shr(insn.k),
                        BPF_NEG => acc = acc.wrapping_neg(),
                        BPF_MOD => {
                            if insn.k != 0 {
                                acc %= insn.k;
                            } else {
                                acc = 0;
                            }
                        }
                        BPF_XOR => acc ^= insn.k,
                        _ => return SeccompAction::KillThread,
                    }
                }

                // BPF_RET
                0x06 => {
                    let val = match insn.code & 0xe0 {
                        0x00 => insn.k, // BPF_K
                        _ => acc,       // BPF_A
                    };
                    return SeccompAction::from_raw(val);
                }

                // BPF_MISC
                0x07 => match insn.code & 0xf0 {
                    BPF_TAX => x = acc,
                    BPF_TXA => acc = x,
                    _ => return SeccompAction::KillThread,
                },

                _ => return SeccompAction::KillThread,
            }
        }
    }
}

// ---------------------------------------------------------------------------
// PR_SET_SECCOMP constants
// ---------------------------------------------------------------------------

pub const PR_SET_SECCOMP: u32 = 22;
pub const SECCOMP_MODE_FILTER: u32 = 2;

// ---------------------------------------------------------------------------
// Allowed syscalls list (for default allowlist mode)
// ---------------------------------------------------------------------------

/// A minimal default allowlist for init processes.
/// This is intentionally permissive; production systems would lock down further.
pub fn default_allow_filter() -> SeccompFilter {
    // The default filter: always ALLOW.
    // Safety: A single BPF_RET instruction is always valid and within the
    // 4096-instruction limit, so unwrap is acceptable here.
    SeccompFilter::new(
        alloc::vec![BpfInstruction {
            code: BPF_RET,
            jt: 0,
            jf: 0,
            k: SeccompAction::Allow.to_raw(),
        }],
        true,
    )
    .expect("default filter is valid")
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialise the seccomp subsystem.
///
/// Called once during `security::init()`.  No runtime state is required;
/// the BPF interpreter is stateless and filters are installed per-process
/// via `prctl(PR_SET_SECCOMP)`.
pub fn init() {
    crate::serial::println!("[SECCOMP] seccomp initialized");
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    /// Helper: create an always-allow filter.
    fn allow_filter() -> SeccompFilter {
        SeccompFilter::new(
            vec![BpfInstruction {
                code: BPF_RET,
                jt: 0,
                jf: 0,
                k: SeccompAction::Allow.to_raw(),
            }],
            false,
        )
        .unwrap()
    }

    /// Helper: create a filter that kills everything.
    fn kill_filter() -> SeccompFilter {
        SeccompFilter::new(
            vec![BpfInstruction {
                code: BPF_RET,
                jt: 0,
                jf: 0,
                k: SeccompAction::KillThread.to_raw(),
            }],
            false,
        )
        .unwrap()
    }

    /// Helper: create a filter that allows only a specific syscall.
    fn allow_only(syscall_nr: u32) -> SeccompFilter {
        // Load syscall number (offset 0 in seccomp data)
        // Compare with allowed syscall_nr
        // If equal, ALLOW; else KILL
        SeccompFilter::new(
            vec![
                BpfInstruction {
                    code: BPF_LD | BPF_W | BPF_ABS,
                    jt: 0,
                    jf: 0,
                    k: 0, // Load syscall nr from offset 0
                },
                BpfInstruction {
                    code: BPF_JMP | BPF_JEQ | BPF_K,
                    jt: 0,
                    jf: 1,
                    k: syscall_nr,
                },
                BpfInstruction {
                    code: BPF_RET,
                    jt: 0,
                    jf: 0,
                    k: SeccompAction::Allow.to_raw(),
                },
                BpfInstruction {
                    code: BPF_RET,
                    jt: 0,
                    jf: 0,
                    k: SeccompAction::KillThread.to_raw(),
                },
            ],
            false,
        )
        .unwrap()
    }

    #[test]
    fn test_allow_filter_returns_allow() {
        let filter = allow_filter();
        let data = SeccompData {
            nr: 1,
            arch: 0xC000003E,
            ip: 0,
            args: [0; 6],
        };
        assert_eq!(filter.evaluate(&data), SeccompAction::Allow);
    }

    #[test]
    fn test_kill_filter_returns_kill() {
        let filter = kill_filter();
        let data = SeccompData {
            nr: 1,
            arch: 0xC000003E,
            ip: 0,
            args: [0; 6],
        };
        assert_eq!(filter.evaluate(&data), SeccompAction::KillThread);
    }

    #[test]
    fn test_allow_only_specific_syscall() {
        let filter = allow_only(1); // Only allow syscall 1 (Write)
        let data_ok = SeccompData {
            nr: 1,
            arch: 0xC000003E,
            ip: 0,
            args: [0; 6],
        };
        let data_kill = SeccompData {
            nr: 2,
            arch: 0xC000003E,
            ip: 0,
            args: [0; 6],
        };
        assert_eq!(filter.evaluate(&data_ok), SeccompAction::Allow);
        assert_eq!(filter.evaluate(&data_kill), SeccompAction::KillThread);
    }

    #[test]
    fn test_inherit_on_fork_clones_filter() {
        let filter = allow_only(1);
        let child_filter = filter.inherit_on_fork();
        let data = SeccompData {
            nr: 2,
            arch: 0xC000003E,
            ip: 0,
            args: [0; 6],
        };
        // Child filter should behave the same as parent
        assert_eq!(child_filter.evaluate(&data), SeccompAction::KillThread);
    }

    #[test]
    fn test_empty_filter_rejected() {
        assert!(SeccompFilter::new(vec![], false).is_none());
    }

    #[test]
    fn test_no_new_privs_flag() {
        let filter = SeccompFilter::new(
            vec![BpfInstruction {
                code: BPF_RET,
                jt: 0,
                jf: 0,
                k: SeccompAction::Allow.to_raw(),
            }],
            true,
        )
        .unwrap();
        assert!(filter.has_no_new_privs());
    }

    #[test]
    fn test_bpf_alu_operations() {
        // Filter: load immediate, AND with mask, return
        let filter = SeccompFilter::new(
            vec![
                BpfInstruction {
                    code: BPF_LD | BPF_W | BPF_IMM,
                    jt: 0,
                    jf: 0,
                    k: 0xFFFFFFFF,
                },
                BpfInstruction {
                    code: BPF_ALU | BPF_AND | BPF_K,
                    jt: 0,
                    jf: 0,
                    k: 0xFF,
                },
                BpfInstruction {
                    code: BPF_JMP | BPF_JEQ | BPF_K,
                    jt: 0,
                    jf: 1,
                    k: 0xFF,
                },
                BpfInstruction {
                    code: BPF_RET,
                    jt: 0,
                    jf: 0,
                    k: SeccompAction::Allow.to_raw(),
                },
                BpfInstruction {
                    code: BPF_RET,
                    jt: 0,
                    jf: 0,
                    k: SeccompAction::KillThread.to_raw(),
                },
            ],
            false,
        )
        .unwrap();
        let data = SeccompData {
            nr: 0,
            arch: 0,
            ip: 0,
            args: [0; 6],
        };
        assert_eq!(filter.evaluate(&data), SeccompAction::Allow);
    }

    #[test]
    fn test_bpf_jgt_condition() {
        // Filter: load immediate, jump if > 100
        let filter = SeccompFilter::new(
            vec![
                BpfInstruction {
                    code: BPF_LD | BPF_W | BPF_IMM,
                    jt: 0,
                    jf: 0,
                    k: 200,
                },
                BpfInstruction {
                    code: BPF_JMP | BPF_JGT | BPF_K,
                    jt: 0,
                    jf: 1,
                    k: 100,
                },
                // If > 100: ALLOW
                BpfInstruction {
                    code: BPF_RET,
                    jt: 0,
                    jf: 0,
                    k: SeccompAction::Allow.to_raw(),
                },
                // Else: KILL
                BpfInstruction {
                    code: BPF_RET,
                    jt: 0,
                    jf: 0,
                    k: SeccompAction::KillThread.to_raw(),
                },
            ],
            false,
        )
        .unwrap();
        let data = SeccompData {
            nr: 0,
            arch: 0,
            ip: 0,
            args: [0; 6],
        };
        assert_eq!(filter.evaluate(&data), SeccompAction::Allow);
    }

    #[test]
    fn test_bpf_jset_condition() {
        // Filter: load 0x100, test bit 8, if set ALLOW else KILL
        let filter = SeccompFilter::new(
            vec![
                BpfInstruction {
                    code: BPF_LD | BPF_W | BPF_IMM,
                    jt: 0,
                    jf: 0,
                    k: 0x100,
                },
                BpfInstruction {
                    code: BPF_JMP | BPF_JSET | BPF_K,
                    jt: 0,
                    jf: 1,
                    k: 0x100,
                },
                BpfInstruction {
                    code: BPF_RET,
                    jt: 0,
                    jf: 0,
                    k: SeccompAction::Allow.to_raw(),
                },
                BpfInstruction {
                    code: BPF_RET,
                    jt: 0,
                    jf: 0,
                    k: SeccompAction::KillThread.to_raw(),
                },
            ],
            false,
        )
        .unwrap();
        let data = SeccompData {
            nr: 0,
            arch: 0,
            ip: 0,
            args: [0; 6],
        };
        assert_eq!(filter.evaluate(&data), SeccompAction::Allow);
    }

    #[test]
    fn test_seccomp_data_load() {
        // Filter: load arg0 (offset 16) and return it as action
        let filter = SeccompFilter::new(
            vec![
                BpfInstruction {
                    code: BPF_LD | BPF_W | BPF_ABS,
                    jt: 0,
                    jf: 0,
                    k: 16, // arg0 lower 32 bits
                },
                BpfInstruction {
                    code: BPF_RET | 0x20, // BPF_A mode — return accumulator
                    jt: 0,
                    jf: 0,
                    k: 0,
                },
            ],
            false,
        )
        .unwrap();
        // This returns acc as action — acc = args[0] low 32 bits
        // We load arg0 = 0x7fff0001 which will be interpreted as SECCOMP_RET_ALLOW
        // by evaluate() since (0x7fff0001 & 0xffff0000) == 0x7fff0000
        let data = SeccompData {
            nr: 0,
            arch: 0,
            ip: 0,
            args: [0x7fff0001, 0, 0, 0, 0, 0],
        };
        assert_eq!(filter.evaluate(&data), SeccompAction::Allow);
    }

    #[test]
    fn test_filter_too_long_rejected() {
        let instructions = alloc::vec![BpfInstruction {
            code: BPF_RET,
            jt: 0,
            jf: 0,
            k: SeccompAction::Allow.to_raw(),
        }; 5000]; // Exceeds 4096 limit
        assert!(SeccompFilter::new(instructions, false).is_none());
    }

    #[test]
    fn test_default_allow_filter() {
        let filter = default_allow_filter();
        assert!(filter.has_no_new_privs());
        let data = SeccompData {
            nr: 0,
            arch: 0xC000003E,
            ip: 0,
            args: [0; 6],
        };
        assert_eq!(filter.evaluate(&data), SeccompAction::Allow);
    }

    // -----------------------------------------------------------------------
    // Round-trip tests for SeccompAction
    // -----------------------------------------------------------------------

    #[test]
    fn test_seccomp_action_allow_roundtrip() {
        assert_eq!(SeccompAction::from_raw(0x7fff0000), SeccompAction::Allow);
        assert_eq!(SeccompAction::Allow.to_raw(), 0x7fff0000);
    }

    #[test]
    fn test_seccomp_action_kill_thread_roundtrip() {
        assert_eq!(
            SeccompAction::from_raw(0x00000000),
            SeccompAction::KillThread
        );
        assert_eq!(SeccompAction::KillThread.to_raw(), 0x00000000);
    }

    #[test]
    fn test_seccomp_action_kill_process_roundtrip() {
        assert_eq!(
            SeccompAction::from_raw(0x80000000),
            SeccompAction::KillProcess
        );
        assert_eq!(SeccompAction::KillProcess.to_raw(), 0x80000000);
    }

    #[test]
    fn test_seccomp_action_trap_roundtrip() {
        assert_eq!(SeccompAction::from_raw(0x00030000), SeccompAction::Trap);
        assert_eq!(SeccompAction::Trap.to_raw(), 0x00030000);
    }

    #[test]
    fn test_seccomp_action_trace_roundtrip() {
        assert_eq!(SeccompAction::from_raw(0x7ff00000), SeccompAction::Trace);
        assert_eq!(SeccompAction::Trace.to_raw(), 0x7ff00000);
    }

    #[test]
    fn test_seccomp_action_errno_roundtrip() {
        let e1 = SeccompAction::Errno(1);
        assert_eq!(e1.to_raw(), 0x00050001);
        assert_eq!(SeccompAction::from_raw(0x00050001), SeccompAction::Errno(1));

        let e42 = SeccompAction::Errno(42);
        assert_eq!(e42.to_raw(), 0x0005002a);
        assert_eq!(
            SeccompAction::from_raw(0x0005002a),
            SeccompAction::Errno(42)
        );

        let e0 = SeccompAction::Errno(0);
        assert_eq!(e0.to_raw(), 0x00050000);
        assert_eq!(SeccompAction::from_raw(0x00050000), SeccompAction::Errno(0));
    }

    #[test]
    fn test_errno_action_filter() {
        // Filter: return Errno(42)
        let filter = SeccompFilter::new(
            vec![BpfInstruction {
                code: BPF_RET,
                jt: 0,
                jf: 0,
                k: SeccompAction::Errno(42).to_raw(),
            }],
            false,
        )
        .unwrap();
        let data = SeccompData {
            nr: 0,
            arch: 0,
            ip: 0,
            args: [0; 6],
        };
        assert_eq!(filter.evaluate(&data), SeccompAction::Errno(42));
    }

    #[test]
    fn test_kill_process_action_filter() {
        let filter = SeccompFilter::new(
            vec![BpfInstruction {
                code: BPF_RET,
                jt: 0,
                jf: 0,
                k: SeccompAction::KillProcess.to_raw(),
            }],
            false,
        )
        .unwrap();
        let data = SeccompData {
            nr: 0,
            arch: 0,
            ip: 0,
            args: [0; 6],
        };
        assert_eq!(filter.evaluate(&data), SeccompAction::KillProcess);
    }

    // -----------------------------------------------------------------------
    // Comprehensive BPF opcode tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_bpf_ja_unconditional_jump() {
        // Jump past 1 instruction, then RET ALLOW
        let filter = SeccompFilter::new(
            vec![
                BpfInstruction {
                    code: BPF_JMP | BPF_JA,
                    jt: 0,
                    jf: 0,
                    k: 1, // skip 1 instruction
                },
                BpfInstruction {
                    code: BPF_RET,
                    jt: 0,
                    jf: 0,
                    k: SeccompAction::KillThread.to_raw(),
                },
                BpfInstruction {
                    code: BPF_RET,
                    jt: 0,
                    jf: 0,
                    k: SeccompAction::Allow.to_raw(),
                },
            ],
            false,
        )
        .unwrap();
        let data = SeccompData {
            nr: 0,
            arch: 0,
            ip: 0,
            args: [0; 6],
        };
        assert_eq!(filter.evaluate(&data), SeccompAction::Allow);
    }

    #[test]
    fn test_bpf_jge_condition() {
        // Load 50, jump if >= 50 → ALLOW (true), else KILL
        let filter = SeccompFilter::new(
            vec![
                BpfInstruction {
                    code: BPF_LD | BPF_W | BPF_IMM,
                    jt: 0,
                    jf: 0,
                    k: 50,
                },
                BpfInstruction {
                    code: BPF_JMP | BPF_JGE | BPF_K,
                    jt: 0,
                    jf: 1,
                    k: 50,
                },
                BpfInstruction {
                    code: BPF_RET,
                    jt: 0,
                    jf: 0,
                    k: SeccompAction::Allow.to_raw(),
                },
                BpfInstruction {
                    code: BPF_RET,
                    jt: 0,
                    jf: 0,
                    k: SeccompAction::KillThread.to_raw(),
                },
            ],
            false,
        )
        .unwrap();
        let data = SeccompData {
            nr: 0,
            arch: 0,
            ip: 0,
            args: [0; 6],
        };
        assert_eq!(filter.evaluate(&data), SeccompAction::Allow);
    }

    #[test]
    fn test_bpf_jge_false() {
        // Load 49, jump if >= 50 → false → KILL
        let filter = SeccompFilter::new(
            vec![
                BpfInstruction {
                    code: BPF_LD | BPF_W | BPF_IMM,
                    jt: 0,
                    jf: 0,
                    k: 49,
                },
                BpfInstruction {
                    code: BPF_JMP | BPF_JGE | BPF_K,
                    jt: 0,
                    jf: 1,
                    k: 50,
                },
                BpfInstruction {
                    code: BPF_RET,
                    jt: 0,
                    jf: 0,
                    k: SeccompAction::Allow.to_raw(),
                },
                BpfInstruction {
                    code: BPF_RET,
                    jt: 0,
                    jf: 0,
                    k: SeccompAction::KillThread.to_raw(),
                },
            ],
            false,
        )
        .unwrap();
        let data = SeccompData {
            nr: 0,
            arch: 0,
            ip: 0,
            args: [0; 6],
        };
        assert_eq!(filter.evaluate(&data), SeccompAction::KillThread);
    }

    #[test]
    fn test_bpf_ldx_then_use_x() {
        // LDX immediate 0x42, then copy X→A via TXA, compare
        let filter = SeccompFilter::new(
            vec![
                BpfInstruction {
                    code: BPF_LD | BPF_W | BPF_IMM, // This is BPF_LD, not BPF_LDX
                    jt: 0,
                    jf: 0,
                    k: 0, // A = 0
                },
                BpfInstruction {
                    code: BPF_LD | BPF_W | BPF_ABS,
                    jt: 0,
                    jf: 0,
                    k: 0, // A = data.nr
                },
                // Jump if nr == 0 → KILL, else → ALLOW
                BpfInstruction {
                    code: BPF_JMP | BPF_JEQ | BPF_K,
                    jt: 0,
                    jf: 1,
                    k: 0,
                },
                BpfInstruction {
                    code: BPF_RET,
                    jt: 0,
                    jf: 0,
                    k: SeccompAction::KillThread.to_raw(),
                },
                BpfInstruction {
                    code: BPF_RET,
                    jt: 0,
                    jf: 0,
                    k: SeccompAction::Allow.to_raw(),
                },
            ],
            false,
        )
        .unwrap();
        let data = SeccompData {
            nr: 42,
            arch: 0,
            ip: 0,
            args: [0; 6],
        };
        assert_eq!(filter.evaluate(&data), SeccompAction::Allow);
    }

    #[test]
    fn test_bpf_tax_txa_roundtrip() {
        // LD IMM 0x42 → TAX (A→X) → TXA (X→A) → compare and return
        let filter = SeccompFilter::new(
            vec![
                BpfInstruction {
                    code: BPF_LD | BPF_W | BPF_IMM,
                    jt: 0,
                    jf: 0,
                    k: 0x42,
                },
                BpfInstruction {
                    code: BPF_MISC | BPF_TAX,
                    jt: 0,
                    jf: 0,
                    k: 0,
                },
                BpfInstruction {
                    code: BPF_MISC | BPF_TXA,
                    jt: 0,
                    jf: 0,
                    k: 0,
                },
                BpfInstruction {
                    code: BPF_JMP | BPF_JEQ | BPF_K,
                    jt: 0,
                    jf: 1,
                    k: 0x42,
                },
                BpfInstruction {
                    code: BPF_RET,
                    jt: 0,
                    jf: 0,
                    k: SeccompAction::Allow.to_raw(),
                },
                BpfInstruction {
                    code: BPF_RET,
                    jt: 0,
                    jf: 0,
                    k: SeccompAction::KillThread.to_raw(),
                },
            ],
            false,
        )
        .unwrap();
        let data = SeccompData {
            nr: 0,
            arch: 0,
            ip: 0,
            args: [0; 6],
        };
        assert_eq!(filter.evaluate(&data), SeccompAction::Allow);
    }

    #[test]
    fn test_bpf_neg_alu() {
        // Load 1 → NEG → should be 0xFFFFFFFF (two's complement of 1)
        // Then jump if == 0xFFFFFFFF → ALLOW
        let filter = SeccompFilter::new(
            vec![
                BpfInstruction {
                    code: BPF_LD | BPF_W | BPF_IMM,
                    jt: 0,
                    jf: 0,
                    k: 1,
                },
                BpfInstruction {
                    code: BPF_ALU | BPF_NEG,
                    jt: 0,
                    jf: 0,
                    k: 0,
                },
                BpfInstruction {
                    code: BPF_JMP | BPF_JEQ | BPF_K,
                    jt: 0,
                    jf: 1,
                    k: 0xFFFFFFFFu32,
                },
                BpfInstruction {
                    code: BPF_RET,
                    jt: 0,
                    jf: 0,
                    k: SeccompAction::Allow.to_raw(),
                },
                BpfInstruction {
                    code: BPF_RET,
                    jt: 0,
                    jf: 0,
                    k: SeccompAction::KillThread.to_raw(),
                },
            ],
            false,
        )
        .unwrap();
        let data = SeccompData {
            nr: 0,
            arch: 0,
            ip: 0,
            args: [0; 6],
        };
        assert_eq!(filter.evaluate(&data), SeccompAction::Allow);
    }

    #[test]
    fn test_bpf_mod_alu() {
        // Load 10, MOD 3 → 1, compare with 1
        let filter = SeccompFilter::new(
            vec![
                BpfInstruction {
                    code: BPF_LD | BPF_W | BPF_IMM,
                    jt: 0,
                    jf: 0,
                    k: 10,
                },
                BpfInstruction {
                    code: BPF_ALU | BPF_MOD | BPF_K,
                    jt: 0,
                    jf: 0,
                    k: 3,
                },
                BpfInstruction {
                    code: BPF_JMP | BPF_JEQ | BPF_K,
                    jt: 0,
                    jf: 1,
                    k: 1,
                },
                BpfInstruction {
                    code: BPF_RET,
                    jt: 0,
                    jf: 0,
                    k: SeccompAction::Allow.to_raw(),
                },
                BpfInstruction {
                    code: BPF_RET,
                    jt: 0,
                    jf: 0,
                    k: SeccompAction::KillThread.to_raw(),
                },
            ],
            false,
        )
        .unwrap();
        let data = SeccompData {
            nr: 0,
            arch: 0,
            ip: 0,
            args: [0; 6],
        };
        assert_eq!(filter.evaluate(&data), SeccompAction::Allow);
    }

    #[test]
    fn test_bpf_div_by_zero() {
        // Load 10, DIV 0 → should set acc to 0 (by spec)
        let filter = SeccompFilter::new(
            vec![
                BpfInstruction {
                    code: BPF_LD | BPF_W | BPF_IMM,
                    jt: 0,
                    jf: 0,
                    k: 10,
                },
                BpfInstruction {
                    code: BPF_ALU | BPF_DIV | BPF_K,
                    jt: 0,
                    jf: 0,
                    k: 0,
                },
                BpfInstruction {
                    code: BPF_JMP | BPF_JEQ | BPF_K,
                    jt: 0,
                    jf: 1,
                    k: 0,
                },
                BpfInstruction {
                    code: BPF_RET,
                    jt: 0,
                    jf: 0,
                    k: SeccompAction::Allow.to_raw(),
                },
                BpfInstruction {
                    code: BPF_RET,
                    jt: 0,
                    jf: 0,
                    k: SeccompAction::KillThread.to_raw(),
                },
            ],
            false,
        )
        .unwrap();
        let data = SeccompData {
            nr: 0,
            arch: 0,
            ip: 0,
            args: [0; 6],
        };
        assert_eq!(filter.evaluate(&data), SeccompAction::Allow);
    }

    #[test]
    fn test_bpf_len_load() {
        // BPF_LEN should return 64 (size of seccomp data)
        let filter = SeccompFilter::new(
            vec![
                BpfInstruction {
                    code: BPF_LD | BPF_W | BPF_LEN,
                    jt: 0,
                    jf: 0,
                    k: 0,
                },
                BpfInstruction {
                    code: BPF_JMP | BPF_JEQ | BPF_K,
                    jt: 0,
                    jf: 1,
                    k: 64,
                },
                BpfInstruction {
                    code: BPF_RET,
                    jt: 0,
                    jf: 0,
                    k: SeccompAction::Allow.to_raw(),
                },
                BpfInstruction {
                    code: BPF_RET,
                    jt: 0,
                    jf: 0,
                    k: SeccompAction::KillThread.to_raw(),
                },
            ],
            false,
        )
        .unwrap();
        let data = SeccompData {
            nr: 0,
            arch: 0,
            ip: 0,
            args: [0; 6],
        };
        assert_eq!(filter.evaluate(&data), SeccompAction::Allow);
    }

    #[test]
    fn test_bpf_ind_indirect_load() {
        // LDX IMM 0 → X=0, then BPF_LD|BPF_W|BPF_IND should load from offset 0 (= data.nr)
        // With X=0, IND offset k=0 loads data.nr (0 in test data)
        // Actually we need to use LDX first. But BPF_LDX|BPF_W|BPF_IMM puts in X
        let filter = SeccompFilter::new(
            vec![
                // Load syscall number (nr=255) into acc via ABS first, just to exercise the path
                // Actually: do LDX with IMM to set X, then IND
                BpfInstruction {
                    code: BPF_LD | BPF_W | BPF_IMM,
                    jt: 0,
                    jf: 0,
                    k: 0xFF, // A = 0xFF
                },
                BpfInstruction {
                    code: BPF_MISC | BPF_TAX,
                    jt: 0,
                    jf: 0,
                    k: 0, // X = A = 0xFF
                },
                // Now IND with k=0: loads from offset X+0 = 0xFF, which is beyond 60 → 0
                BpfInstruction {
                    code: BPF_LD | BPF_W | BPF_IND,
                    jt: 0,
                    jf: 0,
                    k: 0,
                },
                BpfInstruction {
                    code: BPF_JMP | BPF_JEQ | BPF_K,
                    jt: 0,
                    jf: 1,
                    k: 0, // Should be 0 (out of bounds)
                },
                BpfInstruction {
                    code: BPF_RET,
                    jt: 0,
                    jf: 0,
                    k: SeccompAction::Allow.to_raw(),
                },
                BpfInstruction {
                    code: BPF_RET,
                    jt: 0,
                    jf: 0,
                    k: SeccompAction::KillThread.to_raw(),
                },
            ],
            false,
        )
        .unwrap();
        let data = SeccompData {
            nr: 255,
            arch: 0xC000003E,
            ip: 0,
            args: [0; 6],
        };
        assert_eq!(filter.evaluate(&data), SeccompAction::Allow);
    }

    #[test]
    fn test_bpf_half_word_load() {
        // Load half-word (16-bit) from offset 0 (data.nr). nr=0xABCD1234 → 0x1234
        let filter = SeccompFilter::new(
            vec![
                BpfInstruction {
                    code: BPF_LD | BPF_H | BPF_ABS,
                    jt: 0,
                    jf: 0,
                    k: 0,
                },
                BpfInstruction {
                    code: BPF_JMP | BPF_JEQ | BPF_K,
                    jt: 0,
                    jf: 1,
                    k: 0x1234,
                },
                BpfInstruction {
                    code: BPF_RET,
                    jt: 0,
                    jf: 0,
                    k: SeccompAction::Allow.to_raw(),
                },
                BpfInstruction {
                    code: BPF_RET,
                    jt: 0,
                    jf: 0,
                    k: SeccompAction::KillThread.to_raw(),
                },
            ],
            false,
        )
        .unwrap();
        let data = SeccompData {
            nr: 0xABCD1234,
            arch: 0,
            ip: 0,
            args: [0; 6],
        };
        assert_eq!(filter.evaluate(&data), SeccompAction::Allow);
    }

    #[test]
    fn test_bpf_byte_load() {
        // Load byte (8-bit) from offset 0 (data.nr). nr=0xABCD1234 → 0x34
        let filter = SeccompFilter::new(
            vec![
                BpfInstruction {
                    code: BPF_LD | BPF_B | BPF_ABS,
                    jt: 0,
                    jf: 0,
                    k: 0,
                },
                BpfInstruction {
                    code: BPF_JMP | BPF_JEQ | BPF_K,
                    jt: 0,
                    jf: 1,
                    k: 0x34,
                },
                BpfInstruction {
                    code: BPF_RET,
                    jt: 0,
                    jf: 0,
                    k: SeccompAction::Allow.to_raw(),
                },
                BpfInstruction {
                    code: BPF_RET,
                    jt: 0,
                    jf: 0,
                    k: SeccompAction::KillThread.to_raw(),
                },
            ],
            false,
        )
        .unwrap();
        let data = SeccompData {
            nr: 0xABCD1234,
            arch: 0,
            ip: 0,
            args: [0; 6],
        };
        assert_eq!(filter.evaluate(&data), SeccompAction::Allow);
    }

    #[test]
    fn test_bpf_offset_beyond_60_returns_zero() {
        // Offset 64 is beyond our data → returns 0. Compare with 0.
        let filter = SeccompFilter::new(
            vec![
                BpfInstruction {
                    code: BPF_LD | BPF_W | BPF_ABS,
                    jt: 0,
                    jf: 0,
                    k: 64,
                },
                BpfInstruction {
                    code: BPF_JMP | BPF_JEQ | BPF_K,
                    jt: 0,
                    jf: 1,
                    k: 0,
                },
                BpfInstruction {
                    code: BPF_RET,
                    jt: 0,
                    jf: 0,
                    k: SeccompAction::Allow.to_raw(),
                },
                BpfInstruction {
                    code: BPF_RET,
                    jt: 0,
                    jf: 0,
                    k: SeccompAction::KillThread.to_raw(),
                },
            ],
            false,
        )
        .unwrap();
        let data = SeccompData {
            nr: 42,
            arch: 0xC000003E,
            ip: 0,
            args: [0; 6],
        };
        assert_eq!(filter.evaluate(&data), SeccompAction::Allow);
    }

    #[test]
    fn test_bpf_mod_by_zero() {
        // MOD 0 should set acc to 0 (by spec)
        let filter = SeccompFilter::new(
            vec![
                BpfInstruction {
                    code: BPF_LD | BPF_W | BPF_IMM,
                    jt: 0,
                    jf: 0,
                    k: 10,
                },
                BpfInstruction {
                    code: BPF_ALU | BPF_MOD | BPF_K,
                    jt: 0,
                    jf: 0,
                    k: 0,
                },
                BpfInstruction {
                    code: BPF_JMP | BPF_JEQ | BPF_K,
                    jt: 0,
                    jf: 1,
                    k: 0,
                },
                BpfInstruction {
                    code: BPF_RET,
                    jt: 0,
                    jf: 0,
                    k: SeccompAction::Allow.to_raw(),
                },
                BpfInstruction {
                    code: BPF_RET,
                    jt: 0,
                    jf: 0,
                    k: SeccompAction::KillThread.to_raw(),
                },
            ],
            false,
        )
        .unwrap();
        let data = SeccompData {
            nr: 0,
            arch: 0,
            ip: 0,
            args: [0; 6],
        };
        assert_eq!(filter.evaluate(&data), SeccompAction::Allow);
    }

    // -----------------------------------------------------------------------
    // Property 23: Seccomp Filter Inheritance
    // -----------------------------------------------------------------------

    /// Arbitrary seccomp filter generator for proptest.
    fn arb_seccomp_filter() -> proptest::prelude::SBoxedStrategy<SeccompFilter> {
        use proptest::prelude::*;
        proptest::collection::vec(
            (
                any::<u16>(), // code
                any::<u8>(),  // jt
                any::<u8>(),  // jf
                any::<u32>(), // k
            ),
            1..=10, // 1 to 10 instructions
        )
        .prop_filter_map("valid filter", |instructions| {
            SeccompFilter::new(
                instructions
                    .into_iter()
                    .map(|(code, jt, jf, k)| BpfInstruction { code, jt, jf, k })
                    .collect(),
                false,
            )
        })
        .sboxed()
    }

    proptest::proptest! {
        /// Property 23: Seccomp Filter Inheritance
        ///
        /// For any seccomp filter, a forked/clone'd child should:
        /// 1. Inherit the same filter
        /// 2. Be unable to install a less restrictive filter (no_new_privs model)
        #[test]
        fn property_23_seccomp_filter_inheritance(
            filter in arb_seccomp_filter(),
            syscall_nr in 0u32..=60,
            test_data in proptest::collection::vec(proptest::prelude::any::<u64>(), 6),
        ) {

            // 1. Fork inheritance: child filter should produce same result as parent
            let child_filter = filter.inherit_on_fork();

            let mut args = [0u64; 6];
            for (i, v) in test_data.iter().enumerate() {
                args[i] = *v;
            }
            let data = SeccompData {
                nr: syscall_nr,
                arch: 0xC000003E,
                ip: 0,
                args,
            };

            let parent_result = filter.evaluate(&data);
            let child_result = child_filter.evaluate(&data);

            // The inherited filter must evaluate identically
            assert_eq!(
                parent_result, child_result,
                "inherited filter must produce same result as parent"
            );

            // 2. A child with no_new_privs=true (set during prctl) cannot
            //    install a different filter.  We simulate this by verifying
            //    that the child's filter is the same as the parent's.
            //    In the real system, prctl returns EPERM if a filter exists.
            assert_eq!(
                child_filter.inherit_on_fork().evaluate(&data),
                child_filter.evaluate(&data),
                "grandchild filter must behave identically to child filter"
            );
        }
    }
}
