pub const ELF_MAGIC: &[u8; 4] = b"\x7FELF";
pub const ELF_CLASS_64: u8 = 2;
pub const ELF_DATA_LITTLE_ENDIAN: u8 = 1;
pub const ELF_TYPE_EXEC: u16 = 2;
pub const ELF_TYPE_DYN: u16 = 3;
pub const ELF_MACHINE_X86_64: u16 = 62;
pub const PROGRAM_HEADER_LOAD: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ElfHeader {
    pub entry: u64,
    pub program_header_offset: u64,
    pub program_header_entry_size: u16,
    pub program_header_count: u16,
}

#[derive(Debug, Clone, Copy)]
pub struct ProgramHeader {
    pub file_offset: u64,
    pub virtual_address: u64,
    pub file_size: u64,
    pub memory_size: u64,
    pub flags: u32,
    pub align: u64,
}

#[derive(Debug, Clone, Copy)]
pub enum ParseError {
    FileTooSmall,
    BadMagic,
    UnsupportedClass,
    UnsupportedEndian,
    UnsupportedType,
    UnsupportedMachine,
    InvalidProgramHeaderSize,
    ProgramHeaderOutOfBounds,
}

pub fn parse_header(data: &[u8]) -> Result<ElfHeader, ParseError> {
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
    let e_phoff = u64::from_le_bytes([data[32], data[33], data[34], data[35], data[36], data[37], data[38], data[39]]);
    let e_phentsize = u16::from_le_bytes([data[54], data[55]]);
    let e_phnum = u16::from_le_bytes([data[56], data[57]]);
    if e_phentsize < 56 {
        return Err(ParseError::InvalidProgramHeaderSize);
    }
    let entry = u64::from_le_bytes([data[24], data[25], data[26], data[27], data[28], data[29], data[30], data[31]]);
    Ok(ElfHeader {
        entry,
        program_header_offset: e_phoff,
        program_header_entry_size: e_phentsize,
        program_header_count: e_phnum,
    })
}

pub fn parse_program_header(data: &[u8], header: ElfHeader, index: u16) -> Result<Option<ProgramHeader>, ParseError> {
    let start = header.program_header_offset as usize + (index as usize * header.program_header_entry_size as usize);
    let end = start + header.program_header_entry_size as usize;
    if start >= data.len() || end > data.len() {
        return Err(ParseError::ProgramHeaderOutOfBounds);
    }
    let p_type = u32::from_le_bytes([data[start], data[start + 1], data[start + 2], data[start + 3]]);
    if p_type != PROGRAM_HEADER_LOAD {
        return Ok(None);
    }
    let file_offset = u64::from_le_bytes([data[start + 8], data[start + 9], data[start + 10], data[start + 11], data[start + 12], data[start + 13], data[start + 14], data[start + 15]]);
    let virtual_address = u64::from_le_bytes([data[start + 16], data[start + 17], data[start + 18], data[start + 19], data[start + 20], data[start + 21], data[start + 22], data[start + 23]]);
    let file_size = u64::from_le_bytes([data[start + 32], data[start + 33], data[start + 34], data[start + 35], data[start + 36], data[start + 37], data[start + 38], data[start + 39]]);
    let memory_size = u64::from_le_bytes([data[start + 40], data[start + 41], data[start + 42], data[start + 43], data[start + 44], data[start + 45], data[start + 46], data[start + 47]]);
    let flags = u32::from_le_bytes([data[start + 24], data[start + 25], data[start + 26], data[start + 27]]);
    let align = u64::from_le_bytes([data[start + 48], data[start + 49], data[start + 50], data[start + 51], data[start + 52], data[start + 53], data[start + 54], data[start + 55]]);
    Ok(Some(ProgramHeader {
        file_offset,
        virtual_address,
        file_size,
        memory_size,
        flags,
        align,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn magic_is_valid() {
        assert_eq!(ELF_MAGIC, b"\x7FELF");
    }
}