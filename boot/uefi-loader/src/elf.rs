use core::ptr;

use uefi::Status;
use uefi::boot::{self, AllocateType};
use uefi::mem::memory_map::MemoryType;
use uefi::proto::loaded_image::LoadedImage;

const ELF_MAGIC: &[u8; 4] = b"\x7FELF";
const ELF_CLASS_64: u8 = 2;
const ELF_DATA_LITTLE_ENDIAN: u8 = 1;
const ELF_VERSION_CURRENT: u8 = 1;
const ELF_TYPE_EXEC: u16 = 2;
const ELF_MACHINE_X86_64: u16 = 62;
const PROGRAM_HEADER_LOAD: u32 = 1;

#[derive(Debug, Clone, Copy)]
pub struct LoadedKernel {
    pub entry_point: u64,
    pub image_base: u64,
    pub image_size: u64,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub enum LoadError {
    FileTooSmall,
    BadMagic,
    UnsupportedClass(u8),
    UnsupportedEndian(u8),
    UnsupportedVersion(u8),
    UnsupportedType(u16),
    UnsupportedMachine(u16),
    InvalidProgramHeaderSize(u16),
    ProgramHeaderOutOfBounds,
    SegmentOutOfBounds,
    SegmentAlignment(u64),
    SegmentAddressOverflow,
    NoLoadSegments,
    AllocationFailed(Status),
}

#[derive(Debug, Clone, Copy)]
struct ElfHeader {
    entry_point: u64,
    program_header_offset: usize,
    program_header_entry_size: usize,
    program_header_count: usize,
}

#[derive(Debug, Clone, Copy)]
struct ProgramHeader {
    file_offset: usize,
    physical_address: u64,
    file_size: usize,
    memory_size: usize,
}

pub fn load_kernel(image: &[u8]) -> Result<LoadedKernel, LoadError> {
    let header = parse_header(image)?;

    let mut loadable = 0usize;
    let mut image_base = u64::MAX;
    let mut image_end = 0u64;

    for index in 0..header.program_header_count {
        let program_header = parse_program_header(image, header, index)?;
        if let Some(program_header) = program_header {
            loadable += 1;
            image_base = image_base.min(program_header.physical_address);
            let segment_end = program_header
                .physical_address
                .checked_add(program_header.memory_size as u64)
                .ok_or(LoadError::SegmentAddressOverflow)?;
            image_end = image_end.max(segment_end);
        }
    }

    if loadable == 0 {
        return Err(LoadError::NoLoadSegments);
    }

    if (image_base & 0xFFF) != 0 {
        return Err(LoadError::SegmentAlignment(image_base));
    }

    let image_size = image_end
        .checked_sub(image_base)
        .ok_or(LoadError::SegmentAddressOverflow)?;
    let page_count = image_size.div_ceil(4096) as usize;

    let image_ptr = boot::allocate_pages(
        AllocateType::Address(image_base),
        kernel_memory_type(),
        page_count,
    )
    .map_err(|err| LoadError::AllocationFailed(err.status()))?;

    unsafe {
        ptr::write_bytes(image_ptr.as_ptr(), 0, page_count * 4096);
    }

    for index in 0..header.program_header_count {
        if let Some(program_header) = parse_program_header(image, header, index)? {
            let source_end = program_header
                .file_offset
                .checked_add(program_header.file_size)
                .ok_or(LoadError::SegmentOutOfBounds)?;
            if source_end > image.len() {
                return Err(LoadError::SegmentOutOfBounds);
            }

            unsafe {
                ptr::copy_nonoverlapping(
                    image.as_ptr().add(program_header.file_offset),
                    program_header.physical_address as *mut u8,
                    program_header.file_size,
                );
            }
        }
    }

    Ok(LoadedKernel {
        entry_point: header.entry_point,
        image_base,
        image_size,
    })
}

fn kernel_memory_type() -> MemoryType {
    if let Ok(loaded_image) = boot::open_protocol_exclusive::<LoadedImage>(boot::image_handle()) {
        loaded_image.data_type()
    } else {
        MemoryType::LOADER_DATA
    }
}

fn parse_header(image: &[u8]) -> Result<ElfHeader, LoadError> {
    if image.len() < 64 {
        return Err(LoadError::FileTooSmall);
    }
    if &image[0..4] != ELF_MAGIC {
        return Err(LoadError::BadMagic);
    }
    if image[4] != ELF_CLASS_64 {
        return Err(LoadError::UnsupportedClass(image[4]));
    }
    if image[5] != ELF_DATA_LITTLE_ENDIAN {
        return Err(LoadError::UnsupportedEndian(image[5]));
    }
    if image[6] != ELF_VERSION_CURRENT {
        return Err(LoadError::UnsupportedVersion(image[6]));
    }

    let elf_type = read_u16(image, 16)?;
    if elf_type != ELF_TYPE_EXEC {
        return Err(LoadError::UnsupportedType(elf_type));
    }

    let machine = read_u16(image, 18)?;
    if machine != ELF_MACHINE_X86_64 {
        return Err(LoadError::UnsupportedMachine(machine));
    }

    let program_header_entry_size = read_u16(image, 54)?;
    if usize::from(program_header_entry_size) < 56 {
        return Err(LoadError::InvalidProgramHeaderSize(program_header_entry_size));
    }

    Ok(ElfHeader {
        entry_point: read_u64(image, 24)?,
        program_header_offset: read_u64(image, 32)? as usize,
        program_header_entry_size: usize::from(program_header_entry_size),
        program_header_count: usize::from(read_u16(image, 56)?),
    })
}

fn parse_program_header(
    image: &[u8],
    header: ElfHeader,
    index: usize,
) -> Result<Option<ProgramHeader>, LoadError> {
    let start = header
        .program_header_offset
        .checked_add(index.checked_mul(header.program_header_entry_size).ok_or(LoadError::ProgramHeaderOutOfBounds)?)
        .ok_or(LoadError::ProgramHeaderOutOfBounds)?;
    let end = start
        .checked_add(header.program_header_entry_size)
        .ok_or(LoadError::ProgramHeaderOutOfBounds)?;

    if end > image.len() {
        return Err(LoadError::ProgramHeaderOutOfBounds);
    }

    let program_header = &image[start..end];
    if read_u32(program_header, 0)? != PROGRAM_HEADER_LOAD {
        return Ok(None);
    }

    let physical_address = {
        let paddr = read_u64(program_header, 24)?;
        if paddr != 0 {
            paddr
        } else {
            read_u64(program_header, 16)?
        }
    };
    if (physical_address & 0xFFF) != 0 {
        return Err(LoadError::SegmentAlignment(physical_address));
    }

    let file_size = read_u64(program_header, 32)? as usize;
    let memory_size = read_u64(program_header, 40)? as usize;
    if memory_size < file_size {
        return Err(LoadError::SegmentOutOfBounds);
    }

    Ok(Some(ProgramHeader {
        file_offset: read_u64(program_header, 8)? as usize,
        physical_address,
        file_size,
        memory_size,
    }))
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, LoadError> {
    let end = offset.checked_add(2).ok_or(LoadError::ProgramHeaderOutOfBounds)?;
    let slice = bytes.get(offset..end).ok_or(LoadError::ProgramHeaderOutOfBounds)?;
    Ok(u16::from_le_bytes([slice[0], slice[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, LoadError> {
    let end = offset.checked_add(4).ok_or(LoadError::ProgramHeaderOutOfBounds)?;
    let slice = bytes.get(offset..end).ok_or(LoadError::ProgramHeaderOutOfBounds)?;
    Ok(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, LoadError> {
    let end = offset.checked_add(8).ok_or(LoadError::ProgramHeaderOutOfBounds)?;
    let slice = bytes.get(offset..end).ok_or(LoadError::ProgramHeaderOutOfBounds)?;
    Ok(u64::from_le_bytes([
        slice[0], slice[1], slice[2], slice[3], slice[4], slice[5], slice[6], slice[7],
    ]))
}
