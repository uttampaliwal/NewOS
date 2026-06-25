use core::fmt::Write;

use turnix_abi::boot::{BootInfo, BootOutcome};

use crate::memory::FrameAllocator;
use crate::serial::{self, SerialWriter};
use x86_64::VirtAddr;

use alloc::sync::Arc;
use lazy_static::lazy_static;
use spin::Mutex;

lazy_static! {
    pub static ref FRAME_ALLOCATOR: Mutex<Option<FrameAllocator<'static>>> = Mutex::new(None);
    pub static ref PHYS_MEM_OFFSET: Mutex<Option<VirtAddr>> = Mutex::new(None);
}

pub fn get_frame_allocator() -> &'static Mutex<Option<FrameAllocator<'static>>> {
    &FRAME_ALLOCATOR
}

pub fn get_phys_mem_offset() -> VirtAddr {
    *PHYS_MEM_OFFSET
        .lock()
        .as_ref()
        .expect("phys mem offset not set")
}

pub fn early_boot(boot_info: &'static BootInfo) -> BootOutcome {
    serial::init();

    if let Err(e) = validate_boot_info(boot_info) {
        crate::serial::println!("BOOT ERROR: Invalid BootInfo: {}", e);
        panic!("Fatal boot error: {}", e);
    }

    let mut writer = SerialWriter;

    let phys_mem_offset = VirtAddr::new(boot_info.physical_memory_offset);
    *PHYS_MEM_OFFSET.lock() = Some(phys_mem_offset);

    let mut frame_allocator = FrameAllocator::new(boot_info);

    let _ = writeln!(writer, "[STG: KERNEL_REACHED]");
    let _ = writeln!(
        writer,
        "Ramdisk: addr=0x{:016x}, size={} bytes",
        boot_info.ramdisk_addr, boot_info.ramdisk_size
    );

    // 1. Initialize Kernel Paging
    let mut mapper = unsafe { crate::memory::paging::init(phys_mem_offset) };
    let _ = writeln!(writer, "[STG: PAGING_INIT]");

    // 1a. W^X self-check: verify no kernel page is both writable and executable
    {
        let violations = crate::memory::wx::boot_self_check(phys_mem_offset);
        if violations > 0 {
            let _ = writeln!(
                writer,
                "[WARNING] W^X: {} kernel pages are W+X at boot (expected until NX enforcement is applied)",
                violations
            );
        } else {
            let _ = writeln!(writer, "[STG: W^X_OK]");
        }
    }

    // 2. Initialize the kernel heap
    crate::memory::heap::init_heap(&mut mapper, &mut frame_allocator)
        .expect("heap initialization failed");
    let _ = writeln!(writer, "[STG: HEAP_INIT]");

    crate::task::init_kernel_stack_region(&mut mapper, &mut frame_allocator);
    let _ = writeln!(writer, "[STG: KSTACK_REGION_INIT]");

    // Store the frame allocator in the global mutex after heap is ready
    *FRAME_ALLOCATOR.lock() = Some(frame_allocator);

    // 2.5 Initialize swap manager with in-memory backend
    {
        use crate::memory::swap::{InMemorySwapDevice, SwapDevice, SwapManager};
        let swap_device: Arc<dyn SwapDevice> = Arc::new(
            InMemorySwapDevice::new(256, "memswap").expect("failed to allocate swap device memory"),
        );
        SwapManager::init(swap_device, 1024);
    }
    let _ = writeln!(writer, "[STG: SWAP_INIT]");

    // 2.6 Initialize security subsystem (capabilities, LSM, etc.)
    crate::security::init();
    let _ = writeln!(writer, "[STG: SECURITY_INIT]");

    // 3. Initialize Architecture
    crate::gdt::init();
    crate::interrupts::init(phys_mem_offset);
    crate::syscall::init();
    let _ = writeln!(writer, "[STG: ARCH_INIT]");

    // Check kernel page flags for isolation verification
    {
        use x86_64::structures::paging::Translate;
        let kernel_addr = VirtAddr::new(0xffffffff80000000);
        let result = mapper.translate(kernel_addr);
        let _ = writeln!(
            writer,
            "[DEBUG: KERNEL_ADDR={:?}, RESULT={:?}]",
            kernel_addr, result
        );
    }

    // 3.1 Initialize Video Driver
    crate::drivers::video::init(&boot_info.framebuffer);
    let _ = writeln!(writer, "[STG: VIDEO_INIT]");

    // 3.2 Initialize PCI Driver
    crate::drivers::pci::init(phys_mem_offset);
    let _ = writeln!(writer, "[STG: PCI_INIT]");

    // 3.3 Enumerate PCIe devices via ECAM (ACPI MCFG)
    crate::drivers::pcie::enumerate(boot_info.rsdp_addr, phys_mem_offset);
    let _ = writeln!(writer, "[STG: PCIE_ENUM]");

    // 3.4 Probe virtio-net driver and register in DeviceRegistry
    {
        use crate::drivers::framework::{DeviceDriver, DeviceKey};
        let device_infos: alloc::vec::Vec<(DeviceKey, crate::drivers::framework::DeviceInfo)> = {
            let registry = crate::drivers::DEVICE_REGISTRY.lock();
            registry
                .iter_device_infos()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        };
        for (key, info) in &device_infos {
            match crate::drivers::virtio_net::VirtioNetDriver::probe(info) {
                Ok(driver) => {
                    let mut registry = crate::drivers::DEVICE_REGISTRY.lock();
                    registry.register(key.clone(), driver);
                }
                Err(e) => {
                    crate::serial::println!("[VIRTIO] Probe skipped: {:?}", e);
                }
            }
        }
    }
    let _ = writeln!(writer, "[STG: VIRTIO_NET_PROBE]");

    // 3.4a Probe NVMe driver and register in DeviceRegistry
    {
        use crate::drivers::framework::{DeviceDriver, DeviceKey};
        let device_infos: alloc::vec::Vec<(DeviceKey, crate::drivers::framework::DeviceInfo)> = {
            let registry = crate::drivers::DEVICE_REGISTRY.lock();
            registry
                .iter_device_infos()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        };
        for (key, info) in &device_infos {
            match crate::drivers::nvme::NvmeDriver::probe(info) {
                Ok(driver) => {
                    let mut registry = crate::drivers::DEVICE_REGISTRY.lock();
                    registry.register(key.clone(), driver);
                }
                Err(e) => {
                    crate::serial::println!("[NVMe] Probe skipped: {:?}", e);
                }
            }
        }
    }
    let _ = writeln!(writer, "[STG: NVME_PROBE]");

    // 3.4b Probe XHCI USB driver and register in DeviceRegistry
    {
        use crate::drivers::framework::{DeviceDriver, DeviceKey};
        let device_infos: alloc::vec::Vec<(DeviceKey, crate::drivers::framework::DeviceInfo)> = {
            let registry = crate::drivers::DEVICE_REGISTRY.lock();
            registry
                .iter_device_infos()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        };
        for (key, info) in &device_infos {
            match crate::drivers::xhci::XhciDriver::probe(info) {
                Ok(driver) => {
                    let mut registry = crate::drivers::DEVICE_REGISTRY.lock();
                    registry.register(key.clone(), driver);
                }
                Err(e) => {
                    crate::serial::println!("[XHCI] Probe skipped: {:?}", e);
                }
            }
        }
    }
    let _ = writeln!(writer, "[STG: XHCI_PROBE]");

    // 3.4c Probe GPU driver and register in DeviceRegistry
    {
        use crate::drivers::framework::{DeviceDriver, DeviceKey};
        let device_infos: alloc::vec::Vec<(DeviceKey, crate::drivers::framework::DeviceInfo)> = {
            let registry = crate::drivers::DEVICE_REGISTRY.lock();
            registry
                .iter_device_infos()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        };
        for (key, info) in &device_infos {
            match crate::drivers::gpu::GpuDriver::probe(info) {
                Ok(driver) => {
                    let mut registry = crate::drivers::DEVICE_REGISTRY.lock();
                    registry.register(key.clone(), driver);
                }
                Err(e) => {
                    crate::serial::println!("[GPU] Probe skipped: {:?}", e);
                }
            }
        }
    }
    let _ = writeln!(writer, "[STG: GPU_PROBE]");

    // 3.4d Probe TPM 2.0 TIS driver
    {
        const TPM_BASE_ADDR: u64 = 0xFED40000;
        match unsafe { crate::drivers::tpm::TpmDriver::new(TPM_BASE_ADDR) }.probe() {
            Ok(()) => {
                if let Err(e) = crate::drivers::tpm::init(TPM_BASE_ADDR) {
                    crate::serial::println!("[TPM] Init failed: {:?}", e);
                }
            }
            Err(_) => {
                crate::serial::println!("[TPM] Not found");
            }
        }
    }
    let _ = writeln!(writer, "[STG: TPM_PROBE]");

    // Register PID 1 (init) as the Compositor process
    crate::drivers::gpu::DRM_MANAGER
        .lock()
        .set_compositor_pid(1);
    let _ = writeln!(writer, "[STG: COMPOSITOR_PID_SET]");

    // 3.5 Initialize network stack (uses MAC from virtio-net)
    crate::drivers::net::init();
    let _ = writeln!(writer, "[STG: NET_INIT]");

    // 3.6 Initialize ACPI after PCIe enumeration so AML _PRT routing can
    // resolve against the discovered device registry.
    crate::acpi::init(boot_info.rsdp_addr, phys_mem_offset);
    let _ = writeln!(writer, "[STG: ACPI_INIT]");

    // 3.7 Initialize SMP
    crate::smp::init(phys_mem_offset);
    let _ = writeln!(writer, "[STG: SMP_INIT]");

    // 4. Initialize VFS
    crate::vfs::VFS
        .lock()
        .init_from_ramdisk(boot_info.ramdisk_addr, boot_info.ramdisk_size);
    let _ = writeln!(writer, "[STG: VFS_INIT]");

    // 4.2 Mount tmpfs at "/" and ext4 stub at "/mnt"
    {
        let tmpfs: Arc<dyn crate::fs::vfs::FsBackend> =
            Arc::new(crate::fs::tmpfs::TmpfsBackend::new());
        if let Err(e) =
            crate::vfs::VFS
                .lock()
                .mount("/", tmpfs, crate::fs::vfs::MountFlags::default())
        {
            crate::serial::println!("[FS] Failed to mount tmpfs at /: {:?}", e);
        } else {
            let _ = writeln!(writer, "[STG: TMPFS_MOUNTED]");
        }

        let ext4: Arc<dyn crate::fs::vfs::FsBackend> =
            Arc::new(crate::fs::ext4::Ext4Backend::new());
        if let Err(e) =
            crate::vfs::VFS
                .lock()
                .mount("/mnt", ext4, crate::fs::vfs::MountFlags::default())
        {
            crate::serial::println!("[FS] Failed to mount ext4 at /mnt: {:?}", e);
        } else {
            let _ = writeln!(writer, "[STG: EXT4_MOUNTED]");
        }
    }

    // 4.1 Initialize Text Console with PSF font from VFS
    {
        let mut vfs = crate::vfs::VFS.lock();
        if let Some(fd) = vfs.open("font.psf") {
            let stat = vfs.stat("font.psf").unwrap();
            let mut font_data = alloc::vec![0u8; stat.size as usize];
            if let Some(len) = vfs.read(fd, &mut font_data) {
                crate::drivers::video::init_console(font_data[..len].to_vec());
                let _ = writeln!(writer, "[STG: CONSOLE_INIT]");
            }
        }
    }

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
                let _ = writeln!(writer, "[STG: INIT_PROC_BUILD]");

                let init_proc = crate::process::Process::new_from_elf(
                    &elf_data[..len],
                    get_frame_allocator().lock().as_mut().unwrap(),
                    phys_mem_offset,
                )
                .expect("failed to load init process ELF");
                let _ = writeln!(writer, "[STG: INIT_PROC_BUILT]");

                // Add init process to PROCESS_TABLE
                {
                    let mut process_table = crate::process::PROCESS_TABLE.lock();
                    process_table.insert(init_proc.id(), init_proc.inner.clone());
                }
                let _ = writeln!(writer, "[STG: INIT_PROC_REGISTERED]");

                let init_task = crate::task::Task::new_user(
                    init_proc.clone(),
                    &mut mapper,
                    get_frame_allocator().lock().as_mut().unwrap(),
                    phys_mem_offset,
                )
                .expect("failed to create init task");
                let _ = writeln!(writer, "[STG: INIT_TASK_BUILT]");
                crate::acpi::register_init_task(init_task.id);
                crate::task::scheduler::add_task(init_task);
                let _ = writeln!(writer, "[STG: INIT_TASK_ADDED]");

                // 5.1 Load shell process
                if let Some(shell_fd) = vfs.open("shell") {
                    let shell_stat = vfs.stat("shell").unwrap();
                    let mut shell_elf_data = alloc::vec![0u8; shell_stat.size as usize];
                    if let Some(shell_len) = vfs.read(shell_fd, &mut shell_elf_data) {
                        let _ = writeln!(writer, "[STG: SHELL_PROC_BUILD]");
                        let shell_proc = match crate::process::Process::new_from_elf(
                            &shell_elf_data[..shell_len],
                            get_frame_allocator().lock().as_mut().unwrap(),
                            phys_mem_offset,
                        ) {
                            Ok(p) => {
                                let _ = writeln!(writer, "[STG: SHELL_PROC_BUILT]");
                                Some(p)
                            }
                            Err(e) => {
                                let _ = writeln!(writer, "[STG: SHELL_ELF_ERR {:?}]", e);
                                None
                            }
                        };

                        // Add shell process to PROCESS_TABLE
                        if let Some(ref shell_proc) = shell_proc {
                            {
                                let mut process_table = crate::process::PROCESS_TABLE.lock();
                                process_table.insert(shell_proc.id(), shell_proc.inner.clone());
                            }
                            let _ = writeln!(writer, "[STG: SHELL_PROC_REGISTERED]");

                            crate::task::scheduler::add_task(crate::task::Task::new_user(
                                shell_proc.clone(),
                                &mut mapper,
                                get_frame_allocator().lock().as_mut().unwrap(),
                                phys_mem_offset,
                            )
                            .expect("failed to create shell task"));
                            let _ = writeln!(writer, "[STG: SHELL_TASK_ADDED]");
                        }
                        let _ = writeln!(writer, "[STG: SHELL_READY]");
                    }
                }

                // 5.2 Load fault-tester process
                if let Some(fault_fd) = vfs.open("fault-tester") {
                    let fault_stat = vfs.stat("fault-tester").unwrap();
                    let mut fault_elf_data = alloc::vec![0u8; fault_stat.size as usize];
                    if let Some(fault_len) = vfs.read(fault_fd, &mut fault_elf_data) {
                        let _ = writeln!(writer, "[STG: FAULT_PROC_BUILD]");
                        let fault_proc = match crate::process::Process::new_from_elf(
                            &fault_elf_data[..fault_len],
                            get_frame_allocator().lock().as_mut().unwrap(),
                            phys_mem_offset,
                        ) {
                            Ok(p) => {
                                let _ = writeln!(writer, "[STG: FAULT_PROC_BUILT]");
                                Some(p)
                            }
                            Err(e) => {
                                let _ = writeln!(writer, "[STG: FAULT_ELF_ERR {:?}]", e);
                                None
                            }
                        };

                        // Add fault-tester process to PROCESS_TABLE
                        if let Some(ref fault_proc) = fault_proc {
                            {
                                let mut process_table = crate::process::PROCESS_TABLE.lock();
                                process_table.insert(fault_proc.id(), fault_proc.inner.clone());
                            }
                            let _ = writeln!(writer, "[STG: FAULT_PROC_REGISTERED]");

                            crate::task::scheduler::add_task(crate::task::Task::new_user(
                                fault_proc.clone(),
                                &mut mapper,
                                get_frame_allocator().lock().as_mut().unwrap(),
                                phys_mem_offset,
                            )
                            .expect("failed to create fault-tester task"));
                            let _ = writeln!(writer, "[STG: FAULT_TASK_ADDED]");
                        }
                        let _ = writeln!(writer, "[STG: FAULT_TESTER_READY]");
                    }
                }

                // 5.3 Set up stdin (0), stdout (1), stderr (2) as /dev/tty
                vfs.setup_stdio();
                {
                    let table = crate::process::PROCESS_TABLE.lock();
                    if let Some(pcb_arc) = table.get(&init_proc.id()) {
                        let mut pcb = pcb_arc.lock();
                        pcb.fd_table[0] = vfs.get_fd(0);
                        pcb.fd_table[1] = vfs.get_fd(1);
                        pcb.fd_table[2] = vfs.get_fd(2);
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
        get_frame_allocator().lock().as_mut().unwrap(),
    )
    .expect("failed to create heartbeat task"));
    crate::task::scheduler::add_task(crate::task::Task::new(
        worker_task,
        &mut mapper,
        get_frame_allocator().lock().as_mut().unwrap(),
    )
    .expect("failed to create worker task"));
    crate::task::scheduler::add_task(crate::task::Task::new(
        idle_task,
        &mut mapper,
        get_frame_allocator().lock().as_mut().unwrap(),
    )
    .expect("failed to create idle task"));

    x86_64::instructions::interrupts::enable();
    let _ = writeln!(writer, "[STG: INTR_ENABLED]");
    let _ = writeln!(writer, "[STG: SCHED_START]");
    let _ = writeln!(writer, "[BOOT OK]");

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
        if counter.is_multiple_of(64) {
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
        if counter.is_multiple_of(256) {
            crate::serial::print(format_args!("w"));
        }

        for _ in 0..200_000 {
            core::hint::spin_loop();
        }

        crate::task::scheduler::yield_task();
    }
}

extern "sysv64" fn idle_task() -> ! {
    loop {
        x86_64::instructions::hlt();
    }
}

fn validate_boot_info(boot_info: &BootInfo) -> Result<(), &'static str> {
    crate::serial::println!(
        "Validating BootInfo: ABI version = {} (supported: 3, 4)",
        boot_info.abi_version
    );
    // Validate BootInfo ABI version
    if boot_info.abi_version != 3 && boot_info.abi_version != 4 {
        return Err("Unsupported BootInfo ABI version");
    }

    // 1. Memory Map Invariants
    if boot_info.memory_map.descriptors.is_null() {
        return Err("Memory map descriptors pointer is null");
    }
    if !(boot_info.memory_map.descriptors as usize).is_multiple_of(8) {
        return Err("Memory map descriptors must be 8-byte aligned");
    }
    if boot_info.memory_map.map_size == 0 {
        return Err("Memory map is empty");
    }
    if boot_info.memory_map.desc_size
        < core::mem::size_of::<turnix_abi::boot::BootMemoryDescriptor>()
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
