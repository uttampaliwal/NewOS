//! Fuzz target for the Turnix seccomp BPF interpreter.
//!
//! Reads raw bytes from stdin, interprets them as BPF instructions,
//! and evaluates them against a synthetic SeccompData.

#![allow(dead_code, unused_variables, unused_assignments)]

use std::io::{self, Read};

const BPF_LD: u16 = 0x00;
const BPF_LDX: u16 = 0x01;
const BPF_JMP: u16 = 0x05;
const BPF_ALU: u16 = 0x04;
const BPF_RET: u16 = 0x06;
const BPF_MISC: u16 = 0x07;

const BPF_W: u16 = 0x00;
const BPF_H: u16 = 0x08;
const BPF_B: u16 = 0x10;

const BPF_IMM: u16 = 0x00;
const BPF_ABS: u16 = 0x20;
const BPF_IND: u16 = 0x40;
const BPF_MEM: u16 = 0x60;
const BPF_LEN: u16 = 0x80;

const BPF_JA: u16 = 0x00;
const BPF_JEQ: u16 = 0x10;
const BPF_JGT: u16 = 0x20;
const BPF_JGE: u16 = 0x30;
const BPF_JSET: u16 = 0x40;

const BPF_ST: u16 = 0x02;
const BPF_STX: u16 = 0x03;

const BPF_ADD: u16 = 0x00;
const BPF_SUB: u16 = 0x10;
const BPF_MUL: u16 = 0x20;
const BPF_DIV: u16 = 0x30;
const BPF_OR: u16 = 0x40;
const BPF_AND: u16 = 0x50;
const BPF_LSH: u16 = 0x60;
const BPF_RSH: u16 = 0x70;
const BPF_NEG: u16 = 0x80;
const BPF_MOD: u16 = 0x90;
const BPF_XOR: u16 = 0xa0;

const BPF_TAX: u16 = 0x00;
const BPF_TXA: u16 = 0x80;

#[derive(Debug, Clone, Copy)]
struct BpfInstruction {
    code: u16,
    jt: u8,
    jf: u8,
    k: u32,
}

#[derive(Debug, Clone, Copy)]
struct SeccompData {
    nr: i32,
    arch: u32,
    ip: u64,
    args: [u64; 6],
}

#[derive(Debug, PartialEq)]
enum SeccompAction {
    Allow,
    KillProcess,
    KillThread,
    Errno(u16),
    Trap,
    Trace,
}

const MAX_INSTRUCTIONS: usize = 4096;
const STACK_SIZE: usize = 256;

fn evaluate(instructions: &[BpfInstruction], data: &SeccompData) -> SeccompAction {
    if instructions.is_empty() {
        return SeccompAction::KillThread;
    }

    let mut pc: usize = 0;
    let mut a: u32 = 0;
    let mut x: u32 = 0;
    let mut mem = [0u32; STACK_SIZE];

    loop {
        if pc >= instructions.len() {
            return SeccompAction::KillThread;
        }

        let inst = instructions[pc];
        let class = inst.code & 0x07;
        let size = inst.code & 0x18;
        let mode = inst.code & 0xe0;

        match class {
            BPF_RET => {
                let ret = if inst.code == BPF_RET {
                    inst.k
                } else {
                    return SeccompAction::KillThread;
                };

                return match ret {
                    0x00000000 => SeccompAction::KillThread,
                    0x7fff0000 => SeccompAction::Allow,
                    0x00030000 => SeccompAction::KillProcess,
                    0x00050000 => SeccompAction::Trap,
                    0x7fff0001 => SeccompAction::Trace,
                    _ => {
                        if (ret & 0x000f0000) == 0x00060000 {
                            SeccompAction::Errno((ret & 0xffff) as u16)
                        } else {
                            SeccompAction::KillThread
                        }
                    }
                };
            }
            BPF_LD | BPF_LDX => {
                match mode {
                    BPF_IMM => {
                        a = inst.k;
                    }
                    BPF_ABS => {
                        let offset = inst.k as usize;
                        if offset + 4 > std::mem::size_of::<SeccompData>() {
                            return SeccompAction::KillThread;
                        }
                        let bytes = unsafe {
                            std::slice::from_raw_parts(
                                (data as *const SeccompData as *const u8).add(offset),
                                4,
                            )
                        };
                        a = u32::from_ne_bytes(bytes.try_into().unwrap());
                    }
                    BPF_IND => {
                        let offset = (inst.k as usize).wrapping_add(x as usize);
                        if offset + 4 > std::mem::size_of::<SeccompData>() {
                            return SeccompAction::KillThread;
                        }
                        let bytes = unsafe {
                            std::slice::from_raw_parts(
                                (data as *const SeccompData as *const u8).add(offset),
                                4,
                            )
                        };
                        a = u32::from_ne_bytes(bytes.try_into().unwrap());
                    }
                    BPF_LEN => {
                        a = std::mem::size_of::<SeccompData>() as u32;
                    }
                    BPF_MEM => {
                        let idx = inst.k as usize;
                        if idx < STACK_SIZE {
                            a = mem[idx];
                        }
                    }
                    _ => return SeccompAction::KillThread,
                }
                if class == BPF_LDX {
                    x = a;
                }
            }
            BPF_ST | BPF_STX => {
                let idx = inst.k as usize;
                if idx < STACK_SIZE {
                    mem[idx] = if class == BPF_ST { a } else { x };
                }
            }
            BPF_JMP => {
                let jt = inst.jt as usize;
                let jf = inst.jf as usize;
                match mode {
                    BPF_JA => {
                        pc = pc.wrapping_add(1).wrapping_add(inst.k as usize);
                        continue;
                    }
                    BPF_JEQ => {
                        if a == inst.k {
                            pc = pc.wrapping_add(1).wrapping_add(jt);
                        } else {
                            pc = pc.wrapping_add(1).wrapping_add(jf);
                        }
                        continue;
                    }
                    BPF_JGT => {
                        if a > inst.k {
                            pc = pc.wrapping_add(1).wrapping_add(jt);
                        } else {
                            pc = pc.wrapping_add(1).wrapping_add(jf);
                        }
                        continue;
                    }
                    BPF_JGE => {
                        if a >= inst.k {
                            pc = pc.wrapping_add(1).wrapping_add(jt);
                        } else {
                            pc = pc.wrapping_add(1).wrapping_add(jf);
                        }
                        continue;
                    }
                    BPF_JSET => {
                        if (a & inst.k) != 0 {
                            pc = pc.wrapping_add(1).wrapping_add(jt);
                        } else {
                            pc = pc.wrapping_add(1).wrapping_add(jf);
                        }
                        continue;
                    }
                    _ => return SeccompAction::KillThread,
                }
            }
            BPF_ALU => {
                let source = inst.code & 0x08;
                let operand = if source == 0 { inst.k } else { x };

                a = match mode {
                    BPF_ADD => a.wrapping_add(operand),
                    BPF_SUB => a.wrapping_sub(operand),
                    BPF_MUL => a.wrapping_mul(operand),
                    BPF_DIV => {
                        if operand == 0 {
                            0
                        } else {
                            a / operand
                        }
                    }
                    BPF_MOD => {
                        if operand == 0 {
                            0
                        } else {
                            a % operand
                        }
                    }
                    BPF_OR => a | operand,
                    BPF_AND => a & operand,
                    BPF_LSH => a.wrapping_shl(operand & 0x1f),
                    BPF_RSH => a.wrapping_shr(operand & 0x1f),
                    BPF_XOR => a ^ operand,
                    BPF_NEG => (-(a as i32)) as u32,
                    _ => return SeccompAction::KillThread,
                };
            }
            BPF_MISC => {
                match inst.code & 0xf8 {
                    BPF_TAX => x = a,
                    BPF_TXA => a = x,
                    _ => return SeccompAction::KillThread,
                }
            }
            _ => return SeccompAction::KillThread,
        }

        pc = pc.wrapping_add(1);

        // Prevent infinite loops
        if pc > instructions.len() + MAX_INSTRUCTIONS {
            return SeccompAction::KillThread;
        }
    }
}

fn main() {
    let mut buf = Vec::new();
    io::stdin().read_to_end(&mut buf).unwrap();

    // Parse input as array of BPF instructions (8 bytes each)
    let instruction_count = buf.len() / 8;
    if instruction_count == 0 || instruction_count > MAX_INSTRUCTIONS {
        return;
    }

    let instructions: Vec<BpfInstruction> = (0..instruction_count)
        .map(|i| {
            let offset = i * 8;
            BpfInstruction {
                code: u16::from_le_bytes([buf[offset], buf[offset + 1]]),
                jt: buf[offset + 2],
                jf: buf[offset + 3],
                k: u32::from_le_bytes([
                    buf[offset + 4],
                    buf[offset + 5],
                    buf[offset + 6],
                    buf[offset + 7],
                ]),
            }
        })
        .collect();

    let data = SeccompData {
        nr: 1, // write syscall
        arch: 0,
        ip: 0,
        args: [1, 0, 0, 0, 0, 0],
    };

    let _action = evaluate(&instructions, &data);
}
