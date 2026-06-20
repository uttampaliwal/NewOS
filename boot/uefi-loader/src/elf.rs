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
const ELF_TYPE_DYN: u16 = 3;
const ELF_MACHINE_X86_64: u16 = 62;
const PROGRAM_HEADER_LOAD: u32 = 1;
const SECTION_TYPE_RELA: u32 = 4;
const R_X86_64_RELATIVE: u32 = 8;

#[derive(Debug, Clone, Copy)]
pub struct LoadedKernel {
    pub entry_point: u64,
    pub physical_base: u64,
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
    virtual_address: u64,
    _physical_address: u64,
    file_size: usize,
    memory_size: usize,
}

pub fn load_kernel(image: &[u8], kaslr_offset: u64) -> Result<LoadedKernel, LoadError> {
    let header = parse_header(image)?;

    let mut loadable = 0usize;
    let mut virtual_base = u64::MAX;
    let mut virtual_end = 0u64;

    for index in 0..header.program_header_count {
        let program_header = parse_program_header(image, header, index)?;
        if let Some(program_header) = program_header {
            loadable += 1;
            virtual_base = virtual_base.min(program_header.virtual_address);
            let segment_end = program_header
                .virtual_address
                .checked_add(program_header.memory_size as u64)
                .ok_or(LoadError::SegmentAddressOverflow)?;
            virtual_end = virtual_end.max(segment_end);
        }
    }

    if loadable == 0 {
        return Err(LoadError::NoLoadSegments);
    }

    if (virtual_base & 0xFFF) != 0 {
        return Err(LoadError::SegmentAlignment(virtual_base));
    }

    let image_size = virtual_end
        .checked_sub(virtual_base)
        .ok_or(LoadError::SegmentAddressOverflow)?;
    let page_count = image_size.div_ceil(4096) as usize;

    // Allocate physical memory anywhere (AnyPages) for the kernel
    let physical_ptr =
        boot::allocate_pages(AllocateType::AnyPages, kernel_memory_type(), page_count)
            .map_err(|err| LoadError::AllocationFailed(err.status()))?;

    let physical_base = physical_ptr.as_ptr() as u64;

    unsafe {
        ptr::write_bytes(physical_ptr.as_ptr(), 0, page_count * 4096);
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

            // Calculate the physical offset for this segment
            let offset = program_header.virtual_address - virtual_base;
            let dest_physical = physical_base + offset;

            unsafe {
                ptr::copy_nonoverlapping(
                    image.as_ptr().add(program_header.file_offset),
                    dest_physical as *mut u8,
                    program_header.file_size,
                );
            }
        }
    }

    // Apply RELA relocations for PIE/DYN binaries to populate the .got and
    // other absolute-address fixups.  Even with kaslr_offset = 0 the linker
    // may leave GOT entries zeroed (relying on the loader to fill them from
    // the RELA table), so always process them.
    apply_relocations(image, physical_base, virtual_base, kaslr_offset)?;

    Ok(LoadedKernel {
        entry_point: header.entry_point,
        physical_base,
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

/// Apply `R_X86_64_RELATIVE` relocations from all `SHT_RELA` sections,
/// adding `kaslr_offset` to each absolute address target.
///
/// This makes the kernel position-independent by adjusting all embedded
/// absolute addresses to account for the load offset from the preferred
/// base (0xffff_ffff_8000_0000).
fn apply_relocations(
    image: &[u8],
    physical_base: u64,
    virtual_base: u64,
    kaslr_offset: u64,
) -> Result<(), LoadError> {
    if image.len() < 64 {
        return Err(LoadError::FileTooSmall);
    }

    let shoff = read_u64(image, 40)? as usize;
    let shentsize = read_u16(image, 58)? as usize;
    let shnum = read_u16(image, 60)? as usize;
    let shstrndx = read_u16(image, 62)? as usize;

    if shoff == 0 || shentsize < 64 || shnum == 0 {
        return Ok(()); // No section headers, nothing to relocate
    }

    // Read the section name string table (.shstrtab)
    let strtab_off = shoff
        .checked_add(
            shstrndx
                .checked_mul(shentsize)
                .ok_or(LoadError::ProgramHeaderOutOfBounds)?,
        )
        .ok_or(LoadError::ProgramHeaderOutOfBounds)?;
    let strtab_end = strtab_off
        .checked_add(shentsize)
        .ok_or(LoadError::ProgramHeaderOutOfBounds)?;
    if strtab_end > image.len() {
        return Err(LoadError::ProgramHeaderOutOfBounds);
    }
    let strtab_sh = &image[strtab_off..strtab_end];
    let _strtab_offset = read_u64(strtab_sh, 24)? as usize;
    let _strtab_size = read_u64(strtab_sh, 32)? as usize;

    // Iterate through all section headers looking for SHT_RELA
    for i in 0..shnum {
        let sh_off = shoff
            .checked_add(
                i.checked_mul(shentsize)
                    .ok_or(LoadError::ProgramHeaderOutOfBounds)?,
            )
            .ok_or(LoadError::ProgramHeaderOutOfBounds)?;
        let sh_end = sh_off
            .checked_add(shentsize)
            .ok_or(LoadError::ProgramHeaderOutOfBounds)?;
        if sh_end > image.len() {
            return Err(LoadError::ProgramHeaderOutOfBounds);
        }

        let sh_data = &image[sh_off..sh_end];
        let sh_type = read_u32(sh_data, 4)?;

        if sh_type != SECTION_TYPE_RELA {
            continue;
        }

        let sh_offset = read_u64(sh_data, 24)? as usize;
        let sh_size = read_u64(sh_data, 32)? as usize;
        let sh_entsize = read_u64(sh_data, 56)? as usize;

        // Each RELA entry is 24 bytes
        if sh_entsize < 24 {
            continue;
        }

        let num_entries = sh_size / sh_entsize;

        for j in 0..num_entries {
            let entry_off = sh_offset
                .checked_add(
                    j.checked_mul(sh_entsize)
                        .ok_or(LoadError::ProgramHeaderOutOfBounds)?,
                )
                .ok_or(LoadError::ProgramHeaderOutOfBounds)?;
            let entry_end = entry_off
                .checked_add(24)
                .ok_or(LoadError::ProgramHeaderOutOfBounds)?;
            if entry_end > image.len() {
                return Err(LoadError::ProgramHeaderOutOfBounds);
            }

            let r_offset = read_u64(image, entry_off)?; // location to fix
            let r_info = read_u64(image, entry_off + 8)?; // symbol index + type
            let r_addend = read_u64(image, entry_off + 16)?; // addend

            let r_type = (r_info & 0xffff_ffff) as u32;

            if r_type != R_X86_64_RELATIVE {
                continue;
            }

            // The target address (absolute virtual address in the ELF)
            let target_va = r_offset;
            // Convert to physical offset from where the kernel was loaded
            if target_va < virtual_base {
                continue;
            }
            let physical_target = physical_base + (target_va - virtual_base);

            // R_X86_64_RELATIVE: *(r_offset) = B + A
            //   B = load delta = actual_base - linked_virtual_base = kaslr_offset
            //   A = addend (the prelinked full virtual address from the RELA entry)
            // Result: the addend (full VA) shifted by the KASLR offset.
            let new_val = r_addend.wrapping_add(kaslr_offset);
            let target_ptr = physical_target as *mut u64;
            unsafe {
                core::ptr::write_volatile(target_ptr, new_val);
            }
        }
    }

    Ok(())
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
    if elf_type != ELF_TYPE_EXEC && elf_type != ELF_TYPE_DYN {
        return Err(LoadError::UnsupportedType(elf_type));
    }

    let machine = read_u16(image, 18)?;
    if machine != ELF_MACHINE_X86_64 {
        return Err(LoadError::UnsupportedMachine(machine));
    }

    let program_header_entry_size = read_u16(image, 54)?;
    if usize::from(program_header_entry_size) < 56 {
        return Err(LoadError::InvalidProgramHeaderSize(
            program_header_entry_size,
        ));
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
        .checked_add(
            index
                .checked_mul(header.program_header_entry_size)
                .ok_or(LoadError::ProgramHeaderOutOfBounds)?,
        )
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

    let virtual_address = read_u64(program_header, 16)?;
    let physical_address = read_u64(program_header, 24)?;

    if (virtual_address & 0xFFF) != 0 {
        return Err(LoadError::SegmentAlignment(virtual_address));
    }

    let file_size = read_u64(program_header, 32)? as usize;
    let memory_size = read_u64(program_header, 40)? as usize;
    if memory_size < file_size {
        return Err(LoadError::SegmentOutOfBounds);
    }

    Ok(Some(ProgramHeader {
        file_offset: read_u64(program_header, 8)? as usize,
        virtual_address,
        _physical_address: physical_address,
        file_size,
        memory_size,
    }))
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, LoadError> {
    let end = offset
        .checked_add(2)
        .ok_or(LoadError::ProgramHeaderOutOfBounds)?;
    let slice = bytes
        .get(offset..end)
        .ok_or(LoadError::ProgramHeaderOutOfBounds)?;
    Ok(u16::from_le_bytes([slice[0], slice[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, LoadError> {
    let end = offset
        .checked_add(4)
        .ok_or(LoadError::ProgramHeaderOutOfBounds)?;
    let slice = bytes
        .get(offset..end)
        .ok_or(LoadError::ProgramHeaderOutOfBounds)?;
    Ok(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, LoadError> {
    let end = offset
        .checked_add(8)
        .ok_or(LoadError::ProgramHeaderOutOfBounds)?;
    let slice = bytes
        .get(offset..end)
        .ok_or(LoadError::ProgramHeaderOutOfBounds)?;
    Ok(u64::from_le_bytes([
        slice[0], slice[1], slice[2], slice[3], slice[4], slice[5], slice[6], slice[7],
    ]))
}
