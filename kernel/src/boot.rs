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
    let _ = writeln!(
        writer,
        "Ramdisk: addr=0x{:016x}, size={} bytes",
        boot_info.ramdisk_addr, boot_info.ramdisk_size
    );

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
    crate::interrupts::init(phys_mem_offset);
    crate::syscall::init();
    let _ = writeln!(writer, "[STG: ARCH_INIT]");

    // 3.1 Initialize Video Driver
    crate::drivers::video::init(&boot_info.framebuffer);
    let _ = writeln!(writer, "[STG: VIDEO_INIT]");

    // 4. Initialize VFS
    crate::vfs::VFS
        .lock()
        .init_from_ramdisk(boot_info.ramdisk_addr, boot_info.ramdisk_size);
    let _ = writeln!(writer, "[STG: VFS_INIT]");

    // 5. Load and start the init process from ELF
    let _ = writeln!(writer, "[STG: INIT_LOAD]");
    {
        let mut vfs = crate::vfs::VFS.lock();
        if let Some(fd) = vfs.open("init") {
            let stat = vfs.stat("init").unwrap();
            let _ = writeln!(writer, "[STG: INIT_SIZE={}]", stat.size);
            let mut elf_data = alloc::vec![0u8; stat.size as usize];
            if let Some(len) = vfs.read(fd, &mut elf_data) {
                let _ = writeln!(writer, "[STG: INIT_READ_DONE]");
                let init_proc = crate::process::Process::new_from_elf(
                    &elf_data[..len],
                    &mut frame_allocator,
                    phys_mem_offset,
                )
                .expect("failed to load init process ELF");

                crate::task::scheduler::add_task(crate::task::Task::new_user(
                    init_proc,
                    &mut mapper,
                    &mut frame_allocator,
                    phys_mem_offset,
                ));

                // 5.1 Load shell process
                if let Some(shell_fd) = vfs.open("shell") {
                    let shell_stat = vfs.stat("shell").unwrap();
                    let mut shell_elf_data = alloc::vec![0u8; shell_stat.size as usize];
                    if let Some(shell_len) = vfs.read(shell_fd, &mut shell_elf_data) {
                        let shell_proc = crate::process::Process::new_from_elf(
                            &shell_elf_data[..shell_len],
                            &mut frame_allocator,
                            phys_mem_offset,
                        ).expect("failed to load shell process ELF");

                        crate::task::scheduler::add_task(crate::task::Task::new_user(
                            shell_proc,
                            &mut mapper,
                            &mut frame_allocator,
                            phys_mem_offset,
                        ));
                        let _ = writeln!(writer, "[STG: SHELL_READY]");
                    }
                }

                let _ = writeln!(writer, "[STG: INIT_READY]");
            }
        } else {
            let _ = writeln!(writer, "[STG: INIT_NOT_FOUND]");
        }
    }

    // 6. Bring up stable kernel tasks as well
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

    // Proof of concept: Read from VFS
    {
        let mut vfs = crate::vfs::VFS.lock();
        if let Some(fd) = vfs.open("initramfs.txt") {
            let mut buf = [0u8; 64];
            if let Some(len) = vfs.read(fd, &mut buf) {
                let content = core::str::from_utf8(&buf[..len]).unwrap_or("INVALID UTF-8");
                crate::serial::print(format_args!("[vfs:initramfs.txt] {}\n", content));
            }
        } else {
            crate::serial::print(format_args!("[vfs] initramfs.txt not found\n"));
        }
    }

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
    crate::serial::println!(
        "Validating BootInfo: ABI version = {}, expected = 3",
        boot_info.abi_version
    );
    if boot_info.abi_version != 3 {
        return Err("Unsupported BootInfo ABI version");
    }

    // 1. Memory Map Invariants
    if boot_info.memory_map.descriptors.is_null() {
        return Err("Memory map descriptors pointer is null");
    }
    if (boot_info.memory_map.descriptors as usize) % 8 != 0 {
        return Err("Memory map descriptors must be 8-byte aligned");
    }
    if boot_info.memory_map.map_size == 0 {
        return Err("Memory map is empty");
    }
    if boot_info.memory_map.desc_size
        < core::mem::size_of::<newos_abi::boot::BootMemoryDescriptor>()
    {
        return Err("Memory map descriptor size is too small");
    }

    // 2. Memory Layout Invariants
    if boot_info.physical_memory_offset < 0xffff_8000_0000_0000 {
        return Err("Physical memory offset must be in higher-half");
    }

    // 3. Kernel Image Invariants
    if boot_info.kernel_image_base < 0xffff_ffff_8000_0000 {
        return Err("Kernel image base must be in higher-half kernel region");
    }
    if boot_info.kernel_image_size == 0 {
        return Err("Kernel image size cannot be zero");
    }

    // 4. Ramdisk Invariants
    if boot_info.ramdisk_size > 0 && boot_info.ramdisk_addr == 0 {
        return Err("Ramdisk size is non-zero but address is null");
    }
    if boot_info.ramdisk_addr != 0 && boot_info.ramdisk_size == 0 {
        return Err("Ramdisk address is non-zero but size is null");
    }

    Ok(())
}
