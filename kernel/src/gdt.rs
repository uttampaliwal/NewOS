use lazy_static::lazy_static;
use x86_64::VirtAddr;
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable, SegmentSelector};
use x86_64::structures::tss::TaskStateSegment;

pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;

lazy_static! {
    static ref TSS: TaskStateSegment = {
        let mut tss = TaskStateSegment::new();
        tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = {
            const STACK_SIZE: usize = 4096 * 5;
            static mut STACK: [u8; STACK_SIZE] = [0; STACK_SIZE];

            let stack_start = VirtAddr::from_ptr(unsafe { &raw const STACK });
            let stack_end = stack_start + STACK_SIZE as u64;
            stack_end
        };
        tss
    };
}

lazy_static! {
    pub static ref GDT: (GlobalDescriptorTable, Selectors) = {
        let mut gdt = GlobalDescriptorTable::new();
        let code_selector = gdt.append(Descriptor::kernel_code_segment());
        let data_selector = gdt.append(Descriptor::kernel_data_segment());
        // User mode segments (Ring 3)
        // Order matters for some syscall implementations: User Data then User Code
        let user_data_selector = gdt.append(Descriptor::user_data_segment());
        let user_code_selector = gdt.append(Descriptor::user_code_segment());
        
        let tss_selector = gdt.append(Descriptor::tss_segment(&TSS));
        (
            gdt,
            Selectors {
                code_selector,
                data_selector,
                user_code_selector,
                user_data_selector,
                tss_selector,
            },
        )
    };
}

pub struct Selectors {
    pub code_selector: SegmentSelector,
    pub data_selector: SegmentSelector,
    pub user_code_selector: SegmentSelector,
    pub user_data_selector: SegmentSelector,
    pub tss_selector: SegmentSelector,
}

pub fn init() {
    use x86_64::instructions::segmentation::{CS, DS, ES, SS, Segment};
    use x86_64::instructions::tables::load_tss;

    GDT.0.load();
    unsafe {
        CS::set_reg(GDT.1.code_selector);
        DS::set_reg(SegmentSelector(0));
        ES::set_reg(SegmentSelector(0));
        SS::set_reg(SegmentSelector(0));
        load_tss(GDT.1.tss_selector);
    }
}

/// Set the Ring 0 stack pointer in the TSS.
/// This stack will be loaded by the CPU when an interrupt occurs while in Ring 3.
pub fn set_interrupt_stack(stack_top: VirtAddr) {
    let mut tss = unsafe { &raw mut *( &raw const TSS as *mut TaskStateSegment) };
    unsafe {
        (*tss).privilege_stack_table[0] = stack_top;
    }
}
