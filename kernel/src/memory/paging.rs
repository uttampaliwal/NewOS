use x86_64::registers::control::{Cr3, Cr3Flags};
use x86_64::structures::paging::{OffsetPageTable, PageTable};
use x86_64::VirtAddr;
use crate::memory::FrameAllocator;
use x86_64::structures::paging::FrameAllocator as X86FrameAllocator;

/// Initialize a new OffsetPageTable by cloning the UEFI PML4.
///
/// # Safety
/// This function is unsafe because the caller must guarantee that the
/// complete physical memory is mapped to virtual memory at the passed
/// `physical_memory_offset`.
pub unsafe fn init(
    physical_memory_offset: VirtAddr,
    frame_allocator: &mut FrameAllocator,
) -> OffsetPageTable<'static> {
    let (uefi_pml4_frame, _) = Cr3::read();
    
    // Allocate a new frame for our kernel PML4
    let new_pml4_frame = frame_allocator.allocate_frame().expect("Failed to allocate frame for new PML4");
    
    let uefi_pml4_ptr = (physical_memory_offset + uefi_pml4_frame.start_address().as_u64()).as_ptr::<PageTable>();
    let new_pml4_ptr = (physical_memory_offset + new_pml4_frame.start_address().as_u64()).as_mut_ptr::<PageTable>();
    
    // Copy the UEFI PML4 to our new PML4
    unsafe { core::ptr::copy_nonoverlapping(uefi_pml4_ptr, new_pml4_ptr, 1) };
    
    // Switch CR3 to our new PML4
    unsafe { Cr3::write(new_pml4_frame, Cr3Flags::empty()) };
    
    unsafe { OffsetPageTable::new(&mut *new_pml4_ptr, physical_memory_offset) }
}
