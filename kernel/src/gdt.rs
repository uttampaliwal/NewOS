use lazy_static::lazy_static;
use x86_64::VirtAddr;
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable, SegmentSelector};
use x86_64::structures::tss::TaskStateSegment;

pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;

#[repr(align(4096))]
pub struct TssWrapper(TaskStateSegment);
pub static mut TSS: TssWrapper = TssWrapper(TaskStateSegment::new());

lazy_static! {
    pub static ref GDT: (GlobalDescriptorTable, Selectors) = {
        let mut gdt = GlobalDescriptorTable::new();
<<<<<<< HEAD

        // Use raw descriptors to ensure absolute architectural correctness.
        // Bit 47: Present
        // Bit 44: Descriptor Type (1 for code/data)
        // Bit 43: Executable
        // Bit 42: Conforming/Direction
        // Bit 41: Readable/Writable
        // Bit 53: 64-bit (Long Mode)
        // Bits 45-46: Privilege Level (DPL)
=======
>>>>>>> 0f397571af62411c068ff843edb220926589e735

        // 1. Kernel Segments
        let kernel_code = gdt.append(Descriptor::kernel_code_segment());
<<<<<<< HEAD

        // 0x10: Kernel Data (DPL 0)
        let kernel_data = gdt.append(Descriptor::kernel_data_segment());

        // 0x18: User Code 32-bit (Compatibility, required as base for SYSRET)
        // Flags: Present | DescriptorType | Executable | Readable | DPL 3
        let user_code_32 = gdt.append(Descriptor::user_code_segment());

        // 0x20: User Data (DPL 3, 64-bit)
        // Flags: Present | DescriptorType | Writable | DPL 3
        let user_data = gdt.append(Descriptor::user_data_segment());

        // 0x28: User Code 64-bit (DPL 3, Long Mode)
        // Flags: Present | DescriptorType | Executable | Readable | LongMode | DPL 3
        let user_code_64 = gdt.append(Descriptor::user_code_segment());

        // 0x30: TSS
        let tss = gdt.append(Descriptor::tss_segment(&TSS));
=======
        let kernel_data = gdt.append(Descriptor::kernel_data_segment());

        // 2. User Segments
        let user_code_32 = gdt.append(Descriptor::user_code_segment());
        let user_data = gdt.append(Descriptor::user_data_segment());
        let user_code_64 = gdt.append(Descriptor::user_code_segment());

        // 3. TSS
        let tss = unsafe {
            #[allow(static_mut_refs)]
            gdt.append(Descriptor::tss_segment(&TSS.0))
        };
>>>>>>> 0f397571af62411c068ff843edb220926589e735

        (
            gdt,
            Selectors {
                kernel_code,
                kernel_data,
                user_code_32,
                user_code_64,
                user_data,
                tss,
            },
        )
    };
}

pub struct Selectors {
    pub kernel_code: SegmentSelector,
    pub kernel_data: SegmentSelector,
    pub user_code_32: SegmentSelector,
    pub user_code_64: SegmentSelector,
    pub user_data: SegmentSelector,
    pub tss: SegmentSelector,
}

#[repr(C)]
pub struct PerCpu {
    pub kernel_stack_ptr: u64,
    pub user_rsp_temp: u64,
}

pub static mut PER_CPU: PerCpu = PerCpu {
    kernel_stack_ptr: 0,
    user_rsp_temp: 0,
};

pub fn init() {
    use x86_64::instructions::segmentation::{CS, DS, ES, SS, Segment};
    use x86_64::instructions::tables::load_tss;

    unsafe {
        // Initialize TSS Double Fault Stack
        TSS.0.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = {
            const STACK_SIZE: usize = 4096 * 2;
            static mut STACK: [u8; STACK_SIZE] = [0; STACK_SIZE];
            let stack_start = VirtAddr::from_ptr(&raw const STACK);
            stack_start + STACK_SIZE as u64
        };

        GDT.0.load();
        CS::set_reg(GDT.1.kernel_code);
        DS::set_reg(SegmentSelector(0));
        ES::set_reg(SegmentSelector(0));
        SS::set_reg(SegmentSelector(0));
        load_tss(GDT.1.tss);

        // Kernel mode runs with GS pointing at PER_CPU. On every transition
        // to user mode, swapgs restores the user GS base (currently zero) and
        // leaves KernelGsBase ready for the next syscall/interrupt entry.
        use x86_64::registers::model_specific::{GsBase, KernelGsBase};
        let per_cpu_ptr = VirtAddr::from_ptr(&raw const PER_CPU);
        GsBase::write(per_cpu_ptr);
        KernelGsBase::write(VirtAddr::zero());
    }
}

pub fn reload_gdt() {
    GDT.0.load();
}

pub fn set_interrupt_stack(stack_top: VirtAddr) {
    unsafe {
<<<<<<< HEAD
        let tss_ptr = &raw const TSS as *mut TaskStateSegment;
        (*tss_ptr).privilege_stack_table[0] = stack_top;

        // Also update our PER_CPU structure for swapgs-based syscalls
=======
        TSS.0.privilege_stack_table[0] = stack_top;
>>>>>>> 0f397571af62411c068ff843edb220926589e735
        PER_CPU.kernel_stack_ptr = stack_top.as_u64();
    }
}
