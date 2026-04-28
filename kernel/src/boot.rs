use core::fmt::Write;

use newos_abi::boot::{BootInfo, BootOutcome};

use crate::memory::FrameAllocator;
use crate::serial::{self, SerialWriter};
use x86_64::VirtAddr;

pub fn early_boot(boot_info: &BootInfo) -> BootOutcome {
    serial::init();

    if let Err(e) = validate_boot_info(boot_info) {
        crate::serial::println!("BOOT ERROR: Invalid BootInfo: {}", e);
        panic!("Fatal boot error: {}", e);
    }

    let mut writer = SerialWriter;
    let mut frame_allocator = FrameAllocator::new(boot_info);

    let _ = writeln!(writer, "[STG: KERNEL_REACHED]");
    
    let phys_mem_offset = VirtAddr::new(boot_info.physical_memory_offset);

    // 1. Initialize Kernel Paging
    let mut mapper = unsafe { crate::memory::paging::init(phys_mem_offset) };
    let _ = writeln!(writer, "[STG: PAGING_INIT]");

    // 2. Initialize the kernel heap
    crate::memory::heap::init_heap(&mut mapper, &mut frame_allocator)
        .expect("heap initialization failed");
    let _ = writeln!(writer, "[STG: HEAP_INIT]");

    // 3. Initialize Architecture
    crate::gdt::init();
    crate::interrupts::init();
    crate::syscall::init();
    let _ = writeln!(writer, "[STG: ARCH_INIT]");

    // 4. Bring up stable kernel tasks before reintroducing user mode.
    let _ = writeln!(writer, "[STG: TASKS_READY]");
    crate::task::scheduler::add_task(crate::task::Task::new(
        heartbeat_task,
        &mut mapper,
        &mut frame_allocator,
    ));
    crate::task::scheduler::add_task(crate::task::Task::new(
        worker_task,
        &mut mapper,
        &mut frame_allocator,
    ));
    crate::task::scheduler::add_task(crate::task::Task::new(
        idle_task,
        &mut mapper,
        &mut frame_allocator,
    ));

    x86_64::instructions::interrupts::enable();
    let _ = writeln!(writer, "[STG: INTR_ENABLED]");
    let _ = writeln!(writer, "[STG: SCHED_START]");

    crate::task::scheduler::start_scheduling();
}

extern "sysv64" fn heartbeat_task() -> ! {
    let mut counter = 0u64;
    loop {
        counter = counter.wrapping_add(1);
        if counter % 64 == 0 {
            crate::serial::print(format_args!("[task:heartbeat {}]\n", counter));
        }

        for _ in 0..50_000 {
            core::hint::spin_loop();
        }

        crate::task::scheduler::yield_task();
    }
}

extern "sysv64" fn worker_task() -> ! {
    let mut counter = 0u64;
    loop {
        counter = counter.wrapping_add(1);
        if counter % 256 == 0 {
            crate::serial::print(format_args!("w"));
        }

        for _ in 0..200_000 {
            core::hint::spin_loop();
        }
    }
}

extern "sysv64" fn idle_task() -> ! {
    loop {
        x86_64::instructions::hlt();
    }
}

fn validate_boot_info(boot_info: &BootInfo) -> Result<(), &'static str> {
    crate::serial::println!("Validating BootInfo: ABI version = {}, expected = 2", boot_info.abi_version);
    if boot_info.abi_version != 2 {
        return Err("Unsupported BootInfo ABI version");
    }
    if boot_info.memory_map.descriptors.is_null() {
        return Err("Memory map descriptors pointer is null");
    }
    Ok(())
}
