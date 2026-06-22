//! Fuzz target for the Turnix ELF parser.
//!
//! Reads raw bytes from stdin and feeds them to the ELF header and
//! program header parsers. Panics on unexpected crashes (not parse errors).

#![allow(dead_code)]

use std::io::{self, Read};

const ELF_MAGIC: &[u8; 4] = b"\x7FELF";
const ELF_CLASS_64: u8 = 2;
const ELF_DATA_LITTLE_ENDIAN: u8 = 1;
const ELF_TYPE_EXEC: u16 = 2;
const ELF_TYPE_DYN: u16 = 3;
const ELF_MACHINE_X86_64: u16 = 62;
const PROGRAM_HEADER_LOAD: u32 = 1;

#[derive(Debug)]
enum ParseError {
    FileTooSmall,
    BadMagic,
    UnsupportedClass,
    UnsupportedEndian,
    UnsupportedType,
    UnsupportedMachine,
    InvalidProgramHeaderSize,
    ProgramHeaderOutOfBounds,
}

fn parse_header(data: &[u8]) -> Result<(u64, u16, u16), ParseError> {
    if data.len() < 64 {
        return Err(ParseError::FileTooSmall);
    }
    if &data[0..4] != ELF_MAGIC {
        return Err(ParseError::BadMagic);
    }
    if data[4] != ELF_CLASS_64 {
        return Err(ParseError::UnsupportedClass);
    }
    if data[5] != ELF_DATA_LITTLE_ENDIAN {
        return Err(ParseError::UnsupportedEndian);
    }
    let e_type = u16::from_le_bytes([data[16], data[17]]);
    if e_type != ELF_TYPE_EXEC && e_type != ELF_TYPE_DYN {
        return Err(ParseError::UnsupportedType);
    }
    let e_machine = u16::from_le_bytes([data[18], data[19]]);
    if e_machine != ELF_MACHINE_X86_64 {
        return Err(ParseError::UnsupportedMachine);
    }
    let e_phoff = u64::from_le_bytes(data[32..40].try_into().unwrap());
    let e_phentsize = u16::from_le_bytes([data[54], data[55]]);
    let e_phnum = u16::from_le_bytes([data[56], data[57]]);
    if e_phentsize < 56 {
        return Err(ParseError::InvalidProgramHeaderSize);
    }
    Ok((e_phoff, e_phentsize, e_phnum))
}

fn parse_program_headers(data: &[u8], phoff: u64, phentsize: u16, phnum: u16) -> Result<(), ParseError> {
    let offset = phoff as usize;
    let entry_size = phentsize as usize;

    for i in 0..phnum {
        let start = offset
            .checked_add((i as usize).checked_mul(entry_size).ok_or(ParseError::ProgramHeaderOutOfBounds)?)
            .ok_or(ParseError::ProgramHeaderOutOfBounds)?;
        let end = start.checked_add(entry_size).ok_or(ParseError::ProgramHeaderOutOfBounds)?;

        if start >= data.len() || end > data.len() {
            return Err(ParseError::ProgramHeaderOutOfBounds);
        }

        let p_type = u32::from_le_bytes(data[start..start + 4].try_into().unwrap());
        if p_type == PROGRAM_HEADER_LOAD {
            let _flags = u32::from_le_bytes(data[start + 4..start + 8].try_into().unwrap());
            let _vaddr = u64::from_le_bytes(data[start + 16..start + 24].try_into().unwrap());
            let _paddr = u64::from_le_bytes(data[start + 24..start + 32].try_into().unwrap());
            let _filesz = u64::from_le_bytes(data[start + 32..start + 40].try_into().unwrap());
            let _memsz = u64::from_le_bytes(data[start + 40..start + 48].try_into().unwrap());
        }
    }
    Ok(())
}

fn main() {
    let mut buf = Vec::new();
    io::stdin().read_to_end(&mut buf).unwrap();

    if let Ok((phoff, phentsize, phnum)) = parse_header(&buf) {
        let _ = parse_program_headers(&buf, phoff, phentsize, phnum);
    }
}
