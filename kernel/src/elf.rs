pub const ELF_MAGIC: &[u8; 4] = b"\x7FELF";
pub const ELF_CLASS_64: u8 = 2;
pub const ELF_DATA_LITTLE_ENDIAN: u8 = 1;
pub const ELF_TYPE_EXEC: u16 = 2;
pub const ELF_TYPE_DYN: u16 = 3;
pub const ELF_MACHINE_X86_64: u16 = 62;
pub const PROGRAM_HEADER_LOAD: u32 = 1;
pub const PF_X: u32 = 1;
pub const PF_W: u32 = 2;
pub const PF_R: u32 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ElfHeader {
    pub entry: u64,
    pub program_header_offset: u64,
    pub program_header_entry_size: u16,
    pub program_header_count: u16,
    pub elf_type: u16,
}

#[derive(Debug, Clone, Copy)]
pub struct ProgramHeader {
    pub file_offset: u64,
    pub virtual_address: u64,
    pub physical_address: u64,
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
    let e_phoff = u64::from_le_bytes([
        data[32], data[33], data[34], data[35], data[36], data[37], data[38], data[39],
    ]);
    let e_phentsize = u16::from_le_bytes([data[54], data[55]]);
    let e_phnum = u16::from_le_bytes([data[56], data[57]]);
    if e_phentsize < 56 {
        return Err(ParseError::InvalidProgramHeaderSize);
    }
    let entry = u64::from_le_bytes([
        data[24], data[25], data[26], data[27], data[28], data[29], data[30], data[31],
    ]);
    Ok(ElfHeader {
        entry,
        program_header_offset: e_phoff,
        program_header_entry_size: e_phentsize,
        program_header_count: e_phnum,
        elf_type: e_type,
    })
}

pub fn parse_program_header(
    data: &[u8],
    header: ElfHeader,
    index: u16,
) -> Result<Option<ProgramHeader>, ParseError> {
    let entry_size = header.program_header_entry_size as usize;
    let offset = header.program_header_offset as usize;

    let start = offset
        .checked_add(
            (index as usize)
                .checked_mul(entry_size)
                .ok_or(ParseError::ProgramHeaderOutOfBounds)?,
        )
        .ok_or(ParseError::ProgramHeaderOutOfBounds)?;
    let end = start
        .checked_add(entry_size)
        .ok_or(ParseError::ProgramHeaderOutOfBounds)?;

    if start >= data.len() || end > data.len() {
        return Err(ParseError::ProgramHeaderOutOfBounds);
    }

    let p_type = u32::from_le_bytes([
        data[start],
        data[start + 1],
        data[start + 2],
        data[start + 3],
    ]);
    if p_type != PROGRAM_HEADER_LOAD {
        return Ok(None);
    }

    let flags = u32::from_le_bytes([
        data[start + 4],
        data[start + 5],
        data[start + 6],
        data[start + 7],
    ]);
    let file_offset = u64::from_le_bytes([
        data[start + 8],
        data[start + 9],
        data[start + 10],
        data[start + 11],
        data[start + 12],
        data[start + 13],
        data[start + 14],
        data[start + 15],
    ]);
    let virtual_address = u64::from_le_bytes([
        data[start + 16],
        data[start + 17],
        data[start + 18],
        data[start + 19],
        data[start + 20],
        data[start + 21],
        data[start + 22],
        data[start + 23],
    ]);
    let physical_address = u64::from_le_bytes([
        data[start + 24],
        data[start + 25],
        data[start + 26],
        data[start + 27],
        data[start + 28],
        data[start + 29],
        data[start + 30],
        data[start + 31],
    ]);
    let file_size = u64::from_le_bytes([
        data[start + 32],
        data[start + 33],
        data[start + 34],
        data[start + 35],
        data[start + 36],
        data[start + 37],
        data[start + 38],
        data[start + 39],
    ]);
    let memory_size = u64::from_le_bytes([
        data[start + 40],
        data[start + 41],
        data[start + 42],
        data[start + 43],
        data[start + 44],
        data[start + 45],
        data[start + 46],
        data[start + 47],
    ]);
    let align = u64::from_le_bytes([
        data[start + 48],
        data[start + 49],
        data[start + 50],
        data[start + 51],
        data[start + 52],
        data[start + 53],
        data[start + 54],
        data[start + 55],
    ]);

    if file_size > memory_size {
        // Technically not a ParseError, but a malformed ELF for our purposes
        return Err(ParseError::ProgramHeaderOutOfBounds);
    }

    Ok(Some(ProgramHeader {
        file_offset,
        virtual_address,
        physical_address,
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

    #[test]
    fn test_parse_header_success() {
        let mut data = [0u8; 64];
        data[0..4].copy_from_slice(ELF_MAGIC);
        data[4] = ELF_CLASS_64;
        data[5] = ELF_DATA_LITTLE_ENDIAN;
        data[16..18].copy_from_slice(&ELF_TYPE_EXEC.to_le_bytes());
        data[18..20].copy_from_slice(&ELF_MACHINE_X86_64.to_le_bytes());
        data[24..32].copy_from_slice(&0x1000u64.to_le_bytes()); // entry
        data[32..40].copy_from_slice(&0x40u64.to_le_bytes()); // phoff
        data[54..56].copy_from_slice(&56u16.to_le_bytes()); // phentsize
        data[56..58].copy_from_slice(&1u16.to_le_bytes()); // phnum

        let header = parse_header(&data).unwrap();
        assert_eq!(header.entry, 0x1000);
        assert_eq!(header.program_header_offset, 0x40);
        assert_eq!(header.program_header_entry_size, 56);
        assert_eq!(header.program_header_count, 1);
        assert_eq!(header.elf_type, ELF_TYPE_EXEC);
    }

    #[test]
    fn test_parse_program_header_success() {
        let mut data = [0u8; 120];
        let ph_offset = 64;
        let phentsize = 56;

        // Elf Header
        data[0..4].copy_from_slice(ELF_MAGIC);
        data[4] = ELF_CLASS_64;
        data[5] = ELF_DATA_LITTLE_ENDIAN;
        data[16..18].copy_from_slice(&ELF_TYPE_EXEC.to_le_bytes());
        data[18..20].copy_from_slice(&ELF_MACHINE_X86_64.to_le_bytes());
        data[32..40].copy_from_slice(&(ph_offset as u64).to_le_bytes());
        data[54..56].copy_from_slice(&(phentsize as u16).to_le_bytes());
        data[56..58].copy_from_slice(&1u16.to_le_bytes());

        let header = parse_header(&data).unwrap();

        // Program Header at data[64]
        let start = ph_offset;
        data[start..start + 4].copy_from_slice(&PROGRAM_HEADER_LOAD.to_le_bytes());
        data[start + 4..start + 8].copy_from_slice(&(PF_R | PF_X).to_le_bytes()); // flags
        data[start + 8..start + 16].copy_from_slice(&0x100u64.to_le_bytes()); // file_offset
        data[start + 16..start + 24].copy_from_slice(&0x400000u64.to_le_bytes()); // vaddr
        data[start + 24..start + 32].copy_from_slice(&0x400000u64.to_le_bytes()); // paddr
        data[start + 32..start + 40].copy_from_slice(&0x1000u64.to_le_bytes()); // filesz
        data[start + 40..start + 48].copy_from_slice(&0x1000u64.to_le_bytes()); // memsz
        data[start + 48..start + 56].copy_from_slice(&0x1000u64.to_le_bytes()); // align

        let ph = parse_program_header(&data, header, 0).unwrap().unwrap();
        assert_eq!(ph.flags, PF_R | PF_X);
        assert_eq!(ph.file_offset, 0x100);
        assert_eq!(ph.virtual_address, 0x400000);
        assert_eq!(ph.physical_address, 0x400000);
        assert_eq!(ph.file_size, 0x1000);
        assert_eq!(ph.memory_size, 0x1000);
        assert_eq!(ph.align, 0x1000);
    }
}
