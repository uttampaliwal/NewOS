use acpi::{
    AcpiHandler, AcpiTables, PhysicalMapping, PlatformInfo,
    fadt::Fadt,
    platform::{
        address::{AddressSpace, GenericAddress},
        interrupt::{InterruptModel, IoApic as AcpiIoApic},
    },
    sdt::Signature,
};
use alloc::{
    collections::{BTreeMap, BTreeSet},
    string::String,
    vec::Vec,
};
use aml::{
    AmlContext, AmlName, DebugVerbosity, Handler, LevelType,
    pci_routing::{PciRoutingTable, Pin},
    value::{AmlValue, Args},
};
use core::ptr::NonNull;
use lazy_static::lazy_static;
use spin::Mutex;
use x86_64::{VirtAddr, instructions::port::Port};

macro_rules! acpi_log {
    ($($arg:tt)*) => {{
        #[cfg(all(target_arch = "x86_64", target_os = "none"))]
        crate::serial::println!($($arg)*);
        #[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
        {
            let _ = format_args!($($arg)*);
        }
    }};
}

pub const AML_METHOD_TIMEOUT_MS: u64 = 100;
const PM1_POWER_BUTTON_BIT: u16 = 1 << 8;
const PM1_SLEEP_ENABLE_BIT: u16 = 1 << 13;
const PM1_SLEEP_TYPE_MASK: u16 = 0x1c00;

static LAPIC_ADDRESS: Mutex<Option<u64>> = Mutex::new(None);
static BOOT_APIC_ID: Mutex<Option<u32>> = Mutex::new(None);
static AP_APIC_IDS: Mutex<Vec<u32>> = Mutex::new(Vec::new());

lazy_static! {
    static ref ACPI_STATE: Mutex<AcpiRuntimeState> = Mutex::new(AcpiRuntimeState::default());
    static ref INIT_SIGNAL_MAILBOX: Mutex<InitSignalMailbox> =
        Mutex::new(InitSignalMailbox::default());
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmlMethodError {
    Evaluation,
    Timeout,
}

pub trait AmlMethodClock {
    fn snapshot(&self) -> Option<u64>;
    fn elapsed_millis(&self, start: u64, end: u64) -> Option<u64>;
}

pub trait AcpiRegisterAccess {
    fn read_u16(&mut self, address: GenericAddress) -> Result<u16, &'static str>;
    fn write_u16(&mut self, address: GenericAddress, value: u16) -> Result<(), &'static str>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcpiPowerState {
    S0,
    S5,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerButtonMode {
    FixedFeature,
    ControlMethod,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SleepTypeValues {
    pub s5_type_a: u16,
    pub s5_type_b: Option<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PowerManagementInfo {
    pub sci_gsi: u32,
    pub power_button_mode: PowerButtonMode,
    pub pm1a_event_block: GenericAddress,
    pub pm1b_event_block: Option<GenericAddress>,
    pub pm1a_control_block: GenericAddress,
    pub pm1b_control_block: Option<GenericAddress>,
    pub sleep_values: Option<SleepTypeValues>,
    pub current_state: AcpiPowerState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PciRoutingEntry {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub pin: u8,
    pub gsi: u32,
    pub vector: u8,
    pub ioapic_address: u32,
    pub ioapic_input: u8,
    pub active_low: bool,
    pub level_triggered: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IoApicDescriptor {
    pub address: u32,
    pub gsi_base: u32,
}

#[derive(Debug, Default)]
struct AcpiRuntimeState {
    io_apics: Vec<IoApicDescriptor>,
    power: Option<PowerManagementInfo>,
    pci_routes: Vec<PciRoutingEntry>,
    platform_devices: Vec<String>,
}

#[derive(Debug, Default, Clone, Copy)]
struct InitSignalMailbox {
    task_id: Option<crate::task::TaskId>,
    shutdown_pending: bool,
    deliveries: u64,
}

#[derive(Clone)]
pub struct TurnixAcpiHandler {
    phys_mem_offset: VirtAddr,
}

impl TurnixAcpiHandler {
    pub fn new(phys_mem_offset: VirtAddr) -> Self {
        Self { phys_mem_offset }
    }
}

impl AcpiHandler for TurnixAcpiHandler {
    unsafe fn map_physical_region<T>(
        &self,
        physical_address: usize,
        size: usize,
    ) -> PhysicalMapping<Self, T> {
        let virtual_address = self.phys_mem_offset + physical_address as u64;
        unsafe {
            PhysicalMapping::new(
                physical_address,
                NonNull::new(virtual_address.as_mut_ptr()).unwrap(),
                size,
                size,
                self.clone(),
            )
        }
    }

    fn unmap_physical_region<T>(_region: &PhysicalMapping<Self, T>) {
        // HHDM mapping; nothing to unmap.
    }
}

struct AmlAcpiHandler {
    phys_mem_offset: VirtAddr,
}

impl AmlAcpiHandler {
    fn new(phys_mem_offset: VirtAddr) -> Self {
        Self { phys_mem_offset }
    }

    fn translate_address(&self, physical_address: usize) -> usize {
        (self.phys_mem_offset + physical_address as u64).as_u64() as usize
    }

    fn pci_config_address(
        segment: u16,
        bus: u8,
        device: u8,
        function: u8,
        offset: u16,
    ) -> Option<u32> {
        if segment != 0 || device >= 32 || function >= 8 {
            return None;
        }

        Some(
            0x8000_0000
                | ((bus as u32) << 16)
                | ((device as u32) << 11)
                | ((function as u32) << 8)
                | ((offset as u32) & 0xfc),
        )
    }

    fn read_pci_config_u32(
        &self,
        segment: u16,
        bus: u8,
        device: u8,
        function: u8,
        offset: u16,
    ) -> u32 {
        let Some(address) = Self::pci_config_address(segment, bus, device, function, offset) else {
            return 0;
        };

        let mut config_addr_port: Port<u32> = Port::new(0xcf8);
        let mut config_data_port: Port<u32> = Port::new(0xcfc);
        unsafe {
            config_addr_port.write(address);
            config_data_port.read()
        }
    }

    fn write_pci_config_u32(
        &self,
        segment: u16,
        bus: u8,
        device: u8,
        function: u8,
        offset: u16,
        value: u32,
    ) {
        let Some(address) = Self::pci_config_address(segment, bus, device, function, offset) else {
            return;
        };

        let mut config_addr_port: Port<u32> = Port::new(0xcf8);
        let mut config_data_port: Port<u32> = Port::new(0xcfc);
        unsafe {
            config_addr_port.write(address);
            config_data_port.write(value);
        }
    }
}

impl Handler for AmlAcpiHandler {
    fn read_u8(&self, address: usize) -> u8 {
        let ptr = self.translate_address(address) as *const u8;
        unsafe { core::ptr::read_volatile(ptr) }
    }

    fn read_u16(&self, address: usize) -> u16 {
        let ptr = self.translate_address(address) as *const u16;
        unsafe { core::ptr::read_volatile(ptr) }
    }

    fn read_u32(&self, address: usize) -> u32 {
        let ptr = self.translate_address(address) as *const u32;
        unsafe { core::ptr::read_volatile(ptr) }
    }

    fn read_u64(&self, address: usize) -> u64 {
        let ptr = self.translate_address(address) as *const u64;
        unsafe { core::ptr::read_volatile(ptr) }
    }

    fn write_u8(&mut self, address: usize, value: u8) {
        let ptr = self.translate_address(address) as *mut u8;
        unsafe { core::ptr::write_volatile(ptr, value) };
    }

    fn write_u16(&mut self, address: usize, value: u16) {
        let ptr = self.translate_address(address) as *mut u16;
        unsafe { core::ptr::write_volatile(ptr, value) };
    }

    fn write_u32(&mut self, address: usize, value: u32) {
        let ptr = self.translate_address(address) as *mut u32;
        unsafe { core::ptr::write_volatile(ptr, value) };
    }

    fn write_u64(&mut self, address: usize, value: u64) {
        let ptr = self.translate_address(address) as *mut u64;
        unsafe { core::ptr::write_volatile(ptr, value) };
    }

    fn read_io_u8(&self, port: u16) -> u8 {
        let mut port = Port::<u8>::new(port);
        unsafe { port.read() }
    }

    fn read_io_u16(&self, port: u16) -> u16 {
        let mut port = Port::<u16>::new(port);
        unsafe { port.read() }
    }

    fn read_io_u32(&self, port: u16) -> u32 {
        let mut port = Port::<u32>::new(port);
        unsafe { port.read() }
    }

    fn write_io_u8(&self, port: u16, value: u8) {
        let mut port = Port::<u8>::new(port);
        unsafe { port.write(value) };
    }

    fn write_io_u16(&self, port: u16, value: u16) {
        let mut port = Port::<u16>::new(port);
        unsafe { port.write(value) };
    }

    fn write_io_u32(&self, port: u16, value: u32) {
        let mut port = Port::<u32>::new(port);
        unsafe { port.write(value) };
    }

    fn read_pci_u8(&self, segment: u16, bus: u8, device: u8, function: u8, offset: u16) -> u8 {
        let value = self.read_pci_config_u32(segment, bus, device, function, offset);
        ((value >> ((offset & 0x3) * 8)) & 0xff) as u8
    }

    fn read_pci_u16(&self, segment: u16, bus: u8, device: u8, function: u8, offset: u16) -> u16 {
        let value = self.read_pci_config_u32(segment, bus, device, function, offset);
        ((value >> ((offset & 0x2) * 8)) & 0xffff) as u16
    }

    fn read_pci_u32(&self, segment: u16, bus: u8, device: u8, function: u8, offset: u16) -> u32 {
        self.read_pci_config_u32(segment, bus, device, function, offset)
    }

    fn write_pci_u8(
        &self,
        segment: u16,
        bus: u8,
        device: u8,
        function: u8,
        offset: u16,
        value: u8,
    ) {
        let aligned = offset & !0x3;
        let shift = (offset & 0x3) * 8;
        let mut current = self.read_pci_config_u32(segment, bus, device, function, aligned);
        current &= !(0xff << shift);
        current |= (value as u32) << shift;
        self.write_pci_config_u32(segment, bus, device, function, aligned, current);
    }

    fn write_pci_u16(
        &self,
        segment: u16,
        bus: u8,
        device: u8,
        function: u8,
        offset: u16,
        value: u16,
    ) {
        let aligned = offset & !0x3;
        let shift = (offset & 0x2) * 8;
        let mut current = self.read_pci_config_u32(segment, bus, device, function, aligned);
        current &= !(0xffff << shift);
        current |= (value as u32) << shift;
        self.write_pci_config_u32(segment, bus, device, function, aligned, current);
    }

    fn write_pci_u32(
        &self,
        segment: u16,
        bus: u8,
        device: u8,
        function: u8,
        offset: u16,
        value: u32,
    ) {
        self.write_pci_config_u32(segment, bus, device, function, offset, value);
    }
}

struct HardwareRegisterAccess;

impl HardwareRegisterAccess {
    fn read_memory_u16(address: u64) -> u16 {
        let virt = crate::boot::get_phys_mem_offset() + address;
        unsafe { core::ptr::read_volatile(virt.as_ptr::<u16>()) }
    }

    fn write_memory_u16(address: u64, value: u16) {
        let virt = crate::boot::get_phys_mem_offset() + address;
        unsafe { core::ptr::write_volatile(virt.as_mut_ptr::<u16>(), value) };
    }

    fn read_memory_timer(address: u64, bit_width: u8) -> u32 {
        let virt = crate::boot::get_phys_mem_offset() + address;
        match bit_width {
            24 | 32 => unsafe { core::ptr::read_volatile(virt.as_ptr::<u32>()) },
            16 => unsafe { core::ptr::read_volatile(virt.as_ptr::<u16>()) as u32 },
            8 => unsafe { core::ptr::read_volatile(virt.as_ptr::<u8>()) as u32 },
            _ => 0,
        }
    }
}

impl AcpiRegisterAccess for HardwareRegisterAccess {
    fn read_u16(&mut self, address: GenericAddress) -> Result<u16, &'static str> {
        match address.address_space {
            AddressSpace::SystemIo => {
                let port = u16::try_from(address.address).map_err(|_| "I/O port out of range")?;
                let mut port = Port::<u16>::new(port);
                Ok(unsafe { port.read() })
            }
            AddressSpace::SystemMemory => Ok(Self::read_memory_u16(address.address)),
            _ => Err("unsupported ACPI register address space"),
        }
    }

    fn write_u16(&mut self, address: GenericAddress, value: u16) -> Result<(), &'static str> {
        match address.address_space {
            AddressSpace::SystemIo => {
                let port = u16::try_from(address.address).map_err(|_| "I/O port out of range")?;
                let mut port = Port::<u16>::new(port);
                unsafe { port.write(value) };
                Ok(())
            }
            AddressSpace::SystemMemory => {
                Self::write_memory_u16(address.address, value);
                Ok(())
            }
            _ => Err("unsupported ACPI register address space"),
        }
    }
}

#[derive(Clone, Copy)]
struct PmTimerClock {
    register: GenericAddress,
    counter_mask: u64,
}

impl PmTimerClock {
    fn new(platform_info: &PlatformInfo) -> Option<Self> {
        platform_info.pm_timer.as_ref().map(|pm_timer| Self {
            register: pm_timer.base,
            counter_mask: if pm_timer.supports_32bit {
                u32::MAX as u64
            } else {
                (1u64 << 24) - 1
            },
        })
    }

    fn snapshot_raw(&self) -> Result<u64, &'static str> {
        let raw = match self.register.address_space {
            AddressSpace::SystemIo => {
                let port = u16::try_from(self.register.address)
                    .map_err(|_| "PM timer port out of range")?;
                match self.register.bit_width {
                    24 | 32 => {
                        let mut port = Port::<u32>::new(port);
                        unsafe { port.read() }
                    }
                    16 => {
                        let mut port = Port::<u16>::new(port);
                        unsafe { port.read() as u32 }
                    }
                    8 => {
                        let mut port = Port::<u8>::new(port);
                        unsafe { port.read() as u32 }
                    }
                    _ => return Err("unsupported PM timer width"),
                }
            }
            AddressSpace::SystemMemory => HardwareRegisterAccess::read_memory_timer(
                self.register.address,
                self.register.bit_width,
            ),
            _ => return Err("unsupported PM timer address space"),
        };

        Ok((raw as u64) & self.counter_mask)
    }
}

impl AmlMethodClock for PmTimerClock {
    fn snapshot(&self) -> Option<u64> {
        self.snapshot_raw().ok()
    }

    fn elapsed_millis(&self, start: u64, end: u64) -> Option<u64> {
        let delta_ticks = if end >= start {
            end - start
        } else {
            (self.counter_mask + 1)
                .checked_sub(start)?
                .checked_add(end)?
        };
        Some(delta_ticks.saturating_mul(1000) / 3_579_545)
    }
}

pub fn get_lapic_address() -> Option<u64> {
    *LAPIC_ADDRESS.lock()
}

pub fn get_boot_apic_id() -> Option<u32> {
    *BOOT_APIC_ID.lock()
}

pub fn get_ap_apic_ids() -> Vec<u32> {
    AP_APIC_IDS.lock().clone()
}

pub fn get_pci_routing_table() -> Vec<PciRoutingEntry> {
    ACPI_STATE.lock().pci_routes.clone()
}

pub fn get_platform_devices() -> Vec<String> {
    ACPI_STATE.lock().platform_devices.clone()
}

pub fn current_power_state() -> Option<AcpiPowerState> {
    ACPI_STATE.lock().power.map(|power| power.current_state)
}

pub fn register_init_task(task_id: crate::task::TaskId) {
    let mut mailbox = INIT_SIGNAL_MAILBOX.lock();
    mailbox.task_id = Some(task_id);
}

pub fn take_init_shutdown_signal() -> bool {
    let mut mailbox = INIT_SIGNAL_MAILBOX.lock();
    let pending = mailbox.shutdown_pending;
    mailbox.shutdown_pending = false;
    pending
}

pub fn init_shutdown_signal_pending() -> bool {
    INIT_SIGNAL_MAILBOX.lock().shutdown_pending
}

pub fn init_shutdown_signal_deliveries() -> u64 {
    INIT_SIGNAL_MAILBOX.lock().deliveries
}

pub fn invoke_method_with_timeout<C: AmlMethodClock>(
    context: &mut AmlContext,
    path: &AmlName,
    clock: &C,
) -> Result<AmlValue, AmlMethodError> {
    let start = clock.snapshot();
    let result = context
        .invoke_method(path, Args::default())
        .map_err(|_| AmlMethodError::Evaluation)?;

    if let Some(start) = start
        && let Some(end) = clock.snapshot()
        && let Some(elapsed) = clock.elapsed_millis(start, end)
        && elapsed > AML_METHOD_TIMEOUT_MS
    {
        acpi_log!(
            "[ACPI] AML method {} exceeded {} ms; ignoring result.",
            path.as_string(),
            AML_METHOD_TIMEOUT_MS
        );
        return Err(AmlMethodError::Timeout);
    }

    Ok(result)
}

pub fn parse_s5_sleep_types(
    context: &AmlContext,
    value: &AmlValue,
) -> Result<SleepTypeValues, &'static str> {
    let AmlValue::Package(values) = value else {
        return Err("invalid _S5 package");
    };

    if values.len() < 2 {
        return Err("_S5 package too short");
    }

    let s5_type_a = values[0]
        .as_integer(context)
        .map_err(|_| "invalid _S5 type A")?;
    let s5_type_b = values[1]
        .as_integer(context)
        .map_err(|_| "invalid _S5 type B")?;

    Ok(SleepTypeValues {
        s5_type_a: s5_type_a as u16,
        s5_type_b: Some(s5_type_b as u16),
    })
}

pub fn resolve_pci_routes_for_registry<C: AmlMethodClock>(
    context: &mut AmlContext,
    registry: &mut crate::drivers::framework::DeviceRegistry,
    io_apics: &[IoApicDescriptor],
    vector_base: u8,
    vector_count: u8,
    clock: &C,
) -> Result<Vec<PciRoutingEntry>, &'static str> {
    let mut routes = Vec::new();
    let mut gsi_to_vector = BTreeMap::<u32, u8>::new();
    let mut next_vector = vector_base;
    let devices = registry
        .iter_device_infos()
        .map(|(key, info)| (key.clone(), info.clone()))
        .collect::<Vec<_>>();

    for bridge in discover_pci_root_bridges(context, clock) {
        let prt_path = AmlName::from_str("_PRT")
            .unwrap()
            .resolve(&bridge.path)
            .map_err(|_| "failed to resolve _PRT path")?;
        let prt = PciRoutingTable::from_prt_path(&prt_path, context)
            .map_err(|_| "failed to parse _PRT")?;

        for (key, info) in devices.iter().filter(|(key, _)| key.bus == bridge.bus) {
            let Some(pin) = info.interrupt_pin.and_then(pin_from_index) else {
                continue;
            };

            let descriptor = match prt.route(key.device as u16, key.function as u16, pin, context) {
                Ok(descriptor) => descriptor,
                Err(_) => continue,
            };

            let Some((ioapic_address, ioapic_input)) =
                resolve_ioapic_for_gsi(io_apics, descriptor.irq)
            else {
                acpi_log!(
                    "[ACPI] No IOAPIC found for GSI {} ({}:{:02x}.{} pin {}).",
                    descriptor.irq,
                    key.bus,
                    key.device,
                    key.function,
                    info.interrupt_pin.unwrap_or_default()
                );
                continue;
            };

            let vector = if let Some(vector) = gsi_to_vector.get(&descriptor.irq).copied() {
                vector
            } else {
                if next_vector >= vector_base.saturating_add(vector_count) {
                    acpi_log!(
                        "[ACPI] Exhausted ACPI PCI vectors while routing GSI {}.",
                        descriptor.irq
                    );
                    continue;
                }
                let vector = next_vector;
                next_vector = next_vector.saturating_add(1);
                gsi_to_vector.insert(descriptor.irq, vector);
                vector
            };

            if let Some(info_mut) = registry.get_device_info_mut(key) {
                info_mut.irq = u8::try_from(descriptor.irq).ok();
            }

            routes.push(PciRoutingEntry {
                bus: key.bus,
                device: key.device,
                function: key.function,
                pin: info.interrupt_pin.unwrap_or_default(),
                gsi: descriptor.irq,
                vector,
                ioapic_address,
                ioapic_input,
                active_low: matches!(
                    descriptor.polarity,
                    aml::resource::InterruptPolarity::ActiveLow
                ),
                level_triggered: matches!(
                    descriptor.trigger,
                    aml::resource::InterruptTrigger::Level
                ),
            });
        }
    }

    Ok(routes)
}

pub fn handle_power_button_event_with_access<A: AcpiRegisterAccess>(
    power: PowerManagementInfo,
    access: &mut A,
) -> Result<bool, &'static str> {
    let triggered = match power.power_button_mode {
        PowerButtonMode::FixedFeature => power_button_status_is_set(power, access)?,
        PowerButtonMode::ControlMethod => true,
    };

    if triggered {
        deliver_shutdown_signal_to_init();
    }

    Ok(triggered)
}

pub fn handle_sci_interrupt() {
    let power = ACPI_STATE.lock().power;
    let Some(power) = power else {
        return;
    };

    let mut access = HardwareRegisterAccess;
    if let Err(error) = handle_power_button_event_with_access(power, &mut access) {
        acpi_log!("[ACPI] Failed to handle SCI interrupt: {}", error);
    }
}

pub fn enter_soft_off() -> Result<(), &'static str> {
    let power = ACPI_STATE
        .lock()
        .power
        .ok_or("ACPI power management unavailable")?;
    let sleep = power.sleep_values.ok_or("ACPI S5 state unavailable")?;
    let mut access = HardwareRegisterAccess;

    transition_pm1_to_sleep(power.pm1a_control_block, sleep.s5_type_a, &mut access)?;
    if let (Some(control_block), Some(s5_type_b)) = (power.pm1b_control_block, sleep.s5_type_b) {
        transition_pm1_to_sleep(control_block, s5_type_b, &mut access)?;
    }

    Ok(())
}

pub fn init(rsdp_addr: u64, phys_mem_offset: VirtAddr) {
    if rsdp_addr == 0 {
        acpi_log!("[ACPI] No RSDP provided by bootloader.");
        return;
    }

    acpi_log!("[ACPI] Initializing... RSDP at {:#x}", rsdp_addr);

    let handler = TurnixAcpiHandler::new(phys_mem_offset);
    let acpi_tables = unsafe { AcpiTables::from_rsdp(handler.clone(), rsdp_addr as usize) };

    let tables = match acpi_tables {
        Ok(tables) => tables,
        Err(error) => {
            acpi_log!("[ACPI] Failed to parse tables: {:?}", error);
            return;
        }
    };

    acpi_log!("[ACPI] Tables parsed successfully.");

    let platform_info = match PlatformInfo::new(&tables) {
        Ok(info) => Some(info),
        Err(error) => {
            acpi_log!("[ACPI] Failed to build platform info: {:?}", error);
            None
        }
    };

    if let Some(ref platform_info) = platform_info {
        update_processor_topology(platform_info);
    }

    let mut context = match parse_acpi_aml_tables(phys_mem_offset, &handler, &tables) {
        Ok(context) => context,
        Err(error) => {
            acpi_log!("[ACPI] Failed to parse AML tables: {}", error);
            return;
        }
    };

    acpi_log!(
        "[ACPI] Parsed DSDT and {} SSDTs into AML namespace.",
        tables.ssdts.len()
    );

    let clock = platform_info.as_ref().and_then(PmTimerClock::new);
    let platform_devices = match &clock {
        Some(clock) => initialize_system_bus_namespace(&mut context, clock),
        None => initialize_system_bus_namespace(&mut context, &NoopClock),
    };

    let io_apics = platform_info
        .as_ref()
        .map(collect_io_apics)
        .unwrap_or_default();

    let power_info = extract_power_management_info(&tables, &context);
    if let Some(power_info) = power_info {
        if let Err(error) = enable_power_button_events(power_info) {
            acpi_log!("[ACPI] Failed to enable power button events: {}", error);
        }
        if let Some((ioapic_address, ioapic_input)) =
            resolve_ioapic_for_gsi(&io_apics, power_info.sci_gsi)
        {
            if let Err(error) = crate::interrupts::configure_ioapic_route(
                ioapic_address,
                ioapic_input,
                crate::interrupts::SCI_INTERRUPT_VECTOR,
                true,
                true,
            ) {
                acpi_log!("[ACPI] Failed to route SCI interrupt: {}", error);
            }
        } else {
            acpi_log!(
                "[ACPI] Could not map SCI GSI {} to an IOAPIC entry.",
                power_info.sci_gsi
            );
        }
    }

    let pci_routes = {
        let mut registry = crate::drivers::DEVICE_REGISTRY.lock();
        match &clock {
            Some(clock) => resolve_pci_routes_for_registry(
                &mut context,
                &mut registry,
                &io_apics,
                crate::interrupts::ACPI_PCI_VECTOR_BASE,
                crate::interrupts::ACPI_PCI_VECTOR_COUNT,
                clock,
            ),
            None => resolve_pci_routes_for_registry(
                &mut context,
                &mut registry,
                &io_apics,
                crate::interrupts::ACPI_PCI_VECTOR_BASE,
                crate::interrupts::ACPI_PCI_VECTOR_COUNT,
                &NoopClock,
            ),
        }
    };

    let pci_routes = match pci_routes {
        Ok(routes) => routes,
        Err(error) => {
            acpi_log!("[ACPI] Failed to resolve PCI routing: {}", error);
            Vec::new()
        }
    };

    if let Err(error) = program_pci_routes(&pci_routes) {
        acpi_log!("[ACPI] Failed to program PCI routes: {}", error);
    }

    {
        let mut state = ACPI_STATE.lock();
        state.io_apics = io_apics;
        state.power = power_info;
        state.pci_routes = pci_routes;
        state.platform_devices = platform_devices;
    }
}

struct NoopClock;

impl AmlMethodClock for NoopClock {
    fn snapshot(&self) -> Option<u64> {
        None
    }

    fn elapsed_millis(&self, _start: u64, _end: u64) -> Option<u64> {
        None
    }
}

fn parse_aml_table<H>(
    handler: &H,
    table: &acpi::AmlTable,
    context: &mut AmlContext,
) -> Result<(), &'static str>
where
    H: AcpiHandler,
{
    let mapping =
        unsafe { handler.map_physical_region::<u8>(table.address, table.length as usize) };
    let data = unsafe {
        core::slice::from_raw_parts(mapping.virtual_start().as_ptr(), table.length as usize)
    };
    let mut aml_bytes = alloc::vec![0u8; data.len()];
    aml_bytes.copy_from_slice(data);

    context
        .parse_table(&aml_bytes)
        .map_err(|_| "failed to parse AML table")?;

    Ok(())
}

fn parse_acpi_aml_tables<H>(
    phys_mem_offset: VirtAddr,
    handler: &H,
    tables: &AcpiTables<H>,
) -> Result<AmlContext, &'static str>
where
    H: AcpiHandler,
{
    let mut context = AmlContext::new(
        alloc::boxed::Box::new(AmlAcpiHandler::new(phys_mem_offset)),
        DebugVerbosity::None,
    );

    if let Some(ref dsdt) = tables.dsdt {
        parse_aml_table(handler, dsdt, &mut context)?;
    }

    for ssdt in tables.ssdts.iter() {
        parse_aml_table(handler, ssdt, &mut context)?;
    }

    Ok(context)
}

fn update_processor_topology(platform_info: &PlatformInfo) {
    if let InterruptModel::Apic(apic_info) = &platform_info.interrupt_model {
        acpi_log!(
            "[ACPI] Local APIC address: {:#x}",
            apic_info.local_apic_address
        );
        *LAPIC_ADDRESS.lock() = Some(apic_info.local_apic_address);
    }

    if let Some(processor_info) = &platform_info.processor_info {
        *BOOT_APIC_ID.lock() = Some(processor_info.boot_processor.local_apic_id);

        let mut ap_ids = AP_APIC_IDS.lock();
        ap_ids.clear();
        for processor in processor_info.application_processors.iter() {
            ap_ids.push(processor.local_apic_id);
        }
    }
}

fn initialize_system_bus_namespace<C: AmlMethodClock>(
    context: &mut AmlContext,
    clock: &C,
) -> Vec<String> {
    let sb_path = AmlName::from_str("\\_SB").unwrap();
    if context.namespace.get_by_path(&sb_path).is_err() {
        acpi_log!("[ACPI] ACPI namespace \\_SB not found.");
        return Vec::new();
    }

    let sb_ini = AmlName::from_str("\\_SB._INI").unwrap();
    invoke_method_if_present(context, &sb_ini, clock);

    let mut devices = Vec::new();
    let mut namespace = context.namespace.clone();
    let _ = namespace.traverse(|path, level| {
        if path.as_string() == "\\" {
            return Ok(true);
        }

        if path.as_string() == "\\_SB" {
            return Ok(true);
        }

        if !path.as_string().starts_with("\\_SB.") {
            return Ok(false);
        }

        if level.typ == LevelType::Device {
            let status = evaluate_device_status(context, path, clock);
            if status.present || status.functional {
                devices.push(path.as_string());
                invoke_method_if_present(context, &resolve_child_path(path, "_INI"), clock);
            }

            return Ok(status.present || status.functional);
        }

        Ok(true)
    });

    acpi_log!(
        "[ACPI] Evaluated \\_SB namespace and discovered {} platform devices.",
        devices.len()
    );
    devices
}

fn evaluate_device_status<C: AmlMethodClock>(
    context: &mut AmlContext,
    path: &AmlName,
    clock: &C,
) -> aml::value::StatusObject {
    let sta_path = resolve_child_path(path, "_STA");
    if context.namespace.get_by_path(&sta_path).is_err() {
        return aml::value::StatusObject::default();
    }

    match invoke_method_with_timeout(context, &sta_path, clock) {
        Ok(value) => value.as_status().unwrap_or_default(),
        Err(error) => {
            acpi_log!(
                "[ACPI] Failed to evaluate {}: {:?}. Continuing with default status.",
                sta_path.as_string(),
                error
            );
            aml::value::StatusObject::default()
        }
    }
}

fn invoke_method_if_present<C: AmlMethodClock>(
    context: &mut AmlContext,
    path: &AmlName,
    clock: &C,
) {
    if context.namespace.get_by_path(path).is_err() {
        return;
    }

    if let Err(error) = invoke_method_with_timeout(context, path, clock) {
        acpi_log!(
            "[ACPI] AML method {} did not complete successfully: {:?}.",
            path.as_string(),
            error
        );
    }
}

fn discover_pci_root_bridges<C: AmlMethodClock>(
    context: &mut AmlContext,
    clock: &C,
) -> Vec<PciRootBridge> {
    let mut bridges = Vec::new();
    let mut namespace = context.namespace.clone();
    let _ = namespace.traverse(|path, level| {
        if path.as_string() == "\\" {
            return Ok(true);
        }

        if !path.as_string().starts_with("\\_SB") {
            return Ok(false);
        }

        if level.typ == LevelType::Device {
            let prt_path = resolve_child_path(path, "_PRT");
            if context.namespace.get_by_path(&prt_path).is_ok() {
                let bus_number =
                    read_optional_integer(context, &resolve_child_path(path, "_BBN"), clock)
                        .unwrap_or(0) as u8;
                bridges.push(PciRootBridge {
                    path: path.clone(),
                    bus: bus_number,
                });
            }
        }

        Ok(true)
    });

    bridges
}

#[derive(Debug, Clone)]
struct PciRootBridge {
    path: AmlName,
    bus: u8,
}

fn read_optional_integer<C: AmlMethodClock>(
    context: &mut AmlContext,
    path: &AmlName,
    clock: &C,
) -> Option<u64> {
    if context.namespace.get_by_path(path).is_err() {
        return None;
    }

    match invoke_method_with_timeout(context, path, clock) {
        Ok(value) => value.as_integer(context).ok(),
        Err(error) => {
            acpi_log!(
                "[ACPI] Failed to evaluate {}: {:?}.",
                path.as_string(),
                error
            );
            None
        }
    }
}

fn collect_io_apics(platform_info: &PlatformInfo) -> Vec<IoApicDescriptor> {
    match &platform_info.interrupt_model {
        InterruptModel::Apic(apic) => {
            let mut io_apics = apic
                .io_apics
                .iter()
                .map(io_apic_descriptor)
                .collect::<Vec<_>>();
            io_apics.sort_by_key(|descriptor| descriptor.gsi_base);
            io_apics
        }
        _ => Vec::new(),
    }
}

fn io_apic_descriptor(io_apic: &AcpiIoApic) -> IoApicDescriptor {
    IoApicDescriptor {
        address: io_apic.address,
        gsi_base: io_apic.global_system_interrupt_base,
    }
}

fn extract_power_management_info(
    tables: &AcpiTables<TurnixAcpiHandler>,
    context: &AmlContext,
) -> Option<PowerManagementInfo> {
    let fadt = unsafe { tables.get_sdt::<Fadt>(Signature::FADT).ok()?? };

    let pm1a_event_block = fadt.pm1a_event_block().ok()?;
    let pm1a_control_block = fadt.pm1a_control_block().ok()?;
    let flags = unsafe { core::ptr::addr_of!(fadt.flags).read_unaligned() };
    let sci_interrupt = unsafe { core::ptr::addr_of!(fadt.sci_interrupt).read_unaligned() };
    let power_button_mode = if flags.power_button_is_control_method() {
        PowerButtonMode::ControlMethod
    } else {
        PowerButtonMode::FixedFeature
    };

    let sleep_values = context
        .namespace
        .get_by_path(&AmlName::from_str("\\_S5").unwrap())
        .ok()
        .and_then(|value| parse_s5_sleep_types(context, value).ok());

    Some(PowerManagementInfo {
        sci_gsi: sci_interrupt as u32,
        power_button_mode,
        pm1a_event_block,
        pm1b_event_block: fadt.pm1b_event_block().ok().flatten(),
        pm1a_control_block,
        pm1b_control_block: fadt.pm1b_control_block().ok().flatten(),
        sleep_values,
        current_state: AcpiPowerState::S0,
    })
}

fn enable_power_button_events(power: PowerManagementInfo) -> Result<(), &'static str> {
    if power.power_button_mode != PowerButtonMode::FixedFeature {
        return Ok(());
    }

    let mut access = HardwareRegisterAccess;
    set_power_button_enable(power.pm1a_event_block, &mut access)?;
    if let Some(pm1b) = power.pm1b_event_block {
        set_power_button_enable(pm1b, &mut access)?;
    }
    Ok(())
}

fn set_power_button_enable<A: AcpiRegisterAccess>(
    event_block: GenericAddress,
    access: &mut A,
) -> Result<(), &'static str> {
    let enable_register = pm1_enable_register(event_block);
    let current = access.read_u16(enable_register)?;
    access.write_u16(enable_register, current | PM1_POWER_BUTTON_BIT)?;
    Ok(())
}

fn power_button_status_is_set<A: AcpiRegisterAccess>(
    power: PowerManagementInfo,
    access: &mut A,
) -> Result<bool, &'static str> {
    let mut triggered = false;

    for event_block in [Some(power.pm1a_event_block), power.pm1b_event_block]
        .into_iter()
        .flatten()
    {
        let status_register = pm1_status_register(event_block);
        let status = access.read_u16(status_register)?;
        if (status & PM1_POWER_BUTTON_BIT) != 0 {
            access.write_u16(status_register, PM1_POWER_BUTTON_BIT)?;
            triggered = true;
        }
    }

    Ok(triggered)
}

fn transition_pm1_to_sleep<A: AcpiRegisterAccess>(
    control_block: GenericAddress,
    sleep_type: u16,
    access: &mut A,
) -> Result<(), &'static str> {
    let current = access.read_u16(control_block)?;
    let next = build_sleep_control_value(current, sleep_type);
    access.write_u16(control_block, next)?;
    Ok(())
}

fn build_sleep_control_value(current: u16, sleep_type: u16) -> u16 {
    (current & !PM1_SLEEP_TYPE_MASK)
        | ((sleep_type << 10) & PM1_SLEEP_TYPE_MASK)
        | PM1_SLEEP_ENABLE_BIT
}

fn resolve_ioapic_for_gsi(io_apics: &[IoApicDescriptor], gsi: u32) -> Option<(u32, u8)> {
    let mut selected = None;

    for descriptor in io_apics.iter() {
        if descriptor.gsi_base <= gsi {
            selected = Some(*descriptor);
        } else {
            break;
        }
    }

    let selected = selected?;
    let input = gsi.checked_sub(selected.gsi_base)?;
    Some((selected.address, u8::try_from(input).ok()?))
}

fn program_pci_routes(routes: &[PciRoutingEntry]) -> Result<(), &'static str> {
    let mut programmed = BTreeSet::new();

    for route in routes {
        if programmed.insert(route.gsi) {
            crate::interrupts::configure_ioapic_route(
                route.ioapic_address,
                route.ioapic_input,
                route.vector,
                route.active_low,
                route.level_triggered,
            )?;
        }
    }

    acpi_log!(
        "[ACPI] Programmed {} PCI interrupt routes.",
        programmed.len()
    );
    Ok(())
}

fn deliver_shutdown_signal_to_init() {
    let mut mailbox = INIT_SIGNAL_MAILBOX.lock();
    mailbox.deliveries = mailbox.deliveries.saturating_add(1);
    mailbox.shutdown_pending = true;

    if let Some(task_id) = mailbox.task_id {
        acpi_log!(
            "[ACPI] Delivered shutdown signal to init task {}.",
            task_id.as_usize()
        );
    } else {
        acpi_log!("[ACPI] Shutdown signal queued before init registration.");
    }
}

fn resolve_child_path(parent: &AmlName, child: &str) -> AmlName {
    AmlName::from_str(child).unwrap().resolve(parent).unwrap()
}

fn pm1_status_register(event_block: GenericAddress) -> GenericAddress {
    GenericAddress {
        bit_width: 16,
        ..event_block
    }
}

fn pm1_enable_register(event_block: GenericAddress) -> GenericAddress {
    let half_bytes = (event_block.bit_width / 16) as u64;
    GenericAddress {
        address: event_block.address + half_bytes,
        bit_width: 16,
        ..event_block
    }
}

fn pin_from_index(index: u8) -> Option<Pin> {
    match index {
        0 => Some(Pin::IntA),
        1 => Some(Pin::IntB),
        2 => Some(Pin::IntC),
        3 => Some(Pin::IntD),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{boxed::Box, sync::Arc, vec, vec::Vec};
    use core::{mem, ptr::NonNull};
    use std::{thread, time::Duration};

    use crate::drivers::framework::{DeviceInfo, DeviceKey, DeviceRegistry};

    struct TestClock {
        start: std::time::Instant,
    }

    impl TestClock {
        fn new() -> Self {
            Self {
                start: std::time::Instant::now(),
            }
        }
    }

    impl AmlMethodClock for TestClock {
        fn snapshot(&self) -> Option<u64> {
            Some(self.start.elapsed().as_millis() as u64)
        }

        fn elapsed_millis(&self, start: u64, end: u64) -> Option<u64> {
            Some(end.saturating_sub(start))
        }
    }

    struct TestRegisterAccess {
        values: BTreeMap<u64, u16>,
    }

    impl TestRegisterAccess {
        fn new() -> Self {
            Self {
                values: BTreeMap::new(),
            }
        }
    }

    impl AcpiRegisterAccess for TestRegisterAccess {
        fn read_u16(&mut self, address: GenericAddress) -> Result<u16, &'static str> {
            Ok(*self.values.get(&address.address).unwrap_or(&0))
        }

        fn write_u16(&mut self, address: GenericAddress, value: u16) -> Result<(), &'static str> {
            self.values.insert(address.address, value);
            Ok(())
        }
    }

    struct NullHandler;

    impl Handler for NullHandler {
        fn read_u8(&self, _address: usize) -> u8 {
            0
        }
        fn read_u16(&self, _address: usize) -> u16 {
            0
        }
        fn read_u32(&self, _address: usize) -> u32 {
            0
        }
        fn read_u64(&self, _address: usize) -> u64 {
            0
        }
        fn write_u8(&mut self, _address: usize, _value: u8) {}
        fn write_u16(&mut self, _address: usize, _value: u16) {}
        fn write_u32(&mut self, _address: usize, _value: u32) {}
        fn write_u64(&mut self, _address: usize, _value: u64) {}
        fn read_io_u8(&self, _port: u16) -> u8 {
            0
        }
        fn read_io_u16(&self, _port: u16) -> u16 {
            0
        }
        fn read_io_u32(&self, _port: u16) -> u32 {
            0
        }
        fn write_io_u8(&self, _port: u16, _value: u8) {}
        fn write_io_u16(&self, _port: u16, _value: u16) {}
        fn write_io_u32(&self, _port: u16, _value: u32) {}
        fn read_pci_u8(
            &self,
            _segment: u16,
            _bus: u8,
            _device: u8,
            _function: u8,
            _offset: u16,
        ) -> u8 {
            0
        }
        fn read_pci_u16(
            &self,
            _segment: u16,
            _bus: u8,
            _device: u8,
            _function: u8,
            _offset: u16,
        ) -> u16 {
            0
        }
        fn read_pci_u32(
            &self,
            _segment: u16,
            _bus: u8,
            _device: u8,
            _function: u8,
            _offset: u16,
        ) -> u32 {
            0
        }
        fn write_pci_u8(
            &self,
            _segment: u16,
            _bus: u8,
            _device: u8,
            _function: u8,
            _offset: u16,
            _value: u8,
        ) {
        }
        fn write_pci_u16(
            &self,
            _segment: u16,
            _bus: u8,
            _device: u8,
            _function: u8,
            _offset: u16,
            _value: u16,
        ) {
        }
        fn write_pci_u32(
            &self,
            _segment: u16,
            _bus: u8,
            _device: u8,
            _function: u8,
            _offset: u16,
            _value: u32,
        ) {
        }
    }

    #[derive(Clone)]
    struct TestAcpiTableHandler {
        image: Arc<[u8]>,
    }

    impl TestAcpiTableHandler {
        fn new(image: Vec<u8>) -> Self {
            Self {
                image: image.into(),
            }
        }
    }

    impl AcpiHandler for TestAcpiTableHandler {
        unsafe fn map_physical_region<T>(
            &self,
            physical_address: usize,
            size: usize,
        ) -> PhysicalMapping<Self, T> {
            let end = physical_address
                .checked_add(size)
                .expect("synthetic ACPI mapping overflow");
            assert!(
                end <= self.image.len(),
                "synthetic ACPI mapping outside test image: {physical_address:#x}..{end:#x}",
            );

            let ptr = unsafe { self.image.as_ptr().add(physical_address) as *mut T };
            unsafe {
                PhysicalMapping::new(
                    physical_address,
                    NonNull::new(ptr).unwrap(),
                    size,
                    size,
                    self.clone(),
                )
            }
        }

        fn unmap_physical_region<T>(_region: &PhysicalMapping<Self, T>) {}
    }

    fn finalize_checksum(bytes: &mut [u8], checksum_index: usize) {
        bytes[checksum_index] = 0;
        let sum = bytes.iter().fold(0u8, |sum, byte| sum.wrapping_add(*byte));
        bytes[checksum_index] = (0u8).wrapping_sub(sum);
    }

    fn write_blob(image: &mut [u8], physical_address: usize, blob: &[u8]) {
        let end = physical_address + blob.len();
        image[physical_address..end].copy_from_slice(blob);
    }

    fn build_rsdp_blob(rsdt_address: u32) -> Vec<u8> {
        let mut rsdp = vec![0u8; 36];
        rsdp[..8].copy_from_slice(b"RSD PTR ");
        rsdp[9..15].copy_from_slice(b"TURNIX");
        rsdp[15] = 0;
        rsdp[16..20].copy_from_slice(&rsdt_address.to_le_bytes());
        finalize_checksum(&mut rsdp[..20], 8);
        rsdp
    }

    fn build_sdt_blob(signature: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let header_len = mem::size_of::<acpi::sdt::SdtHeader>();
        let mut blob = vec![0u8; header_len + body.len()];
        let total_length = blob.len() as u32;
        blob[..4].copy_from_slice(signature);
        blob[4..8].copy_from_slice(&total_length.to_le_bytes());
        blob[8] = 2;
        blob[10..16].copy_from_slice(b"TURNIX");
        blob[16..24].copy_from_slice(b"TESTACPI");
        blob[24..28].copy_from_slice(&1u32.to_le_bytes());
        blob[28..32].copy_from_slice(&0x5452_4e58u32.to_le_bytes());
        blob[32..36].copy_from_slice(&1u32.to_le_bytes());
        blob[header_len..].copy_from_slice(body);
        finalize_checksum(&mut blob, 9);
        blob
    }

    fn build_rsdt_blob(entries: &[u32]) -> Vec<u8> {
        let mut body = Vec::with_capacity(entries.len() * mem::size_of::<u32>());
        for entry in entries {
            body.extend_from_slice(&entry.to_le_bytes());
        }
        build_sdt_blob(b"RSDT", &body)
    }

    fn append_name_seg(bytes: &mut Vec<u8>, name: &str) {
        assert!(!name.is_empty() && name.len() <= 4);
        let mut seg = [b'_'; 4];
        seg[..name.len()].copy_from_slice(name.as_bytes());
        bytes.extend_from_slice(&seg);
    }

    fn encode_pkg_length(payload_len_without_length_field: usize) -> Vec<u8> {
        for total_length_bytes in 1..=4usize {
            let extra_bytes = total_length_bytes - 1;
            let raw_length = payload_len_without_length_field + total_length_bytes;
            let encodable_bits = if extra_bytes == 0 {
                6
            } else {
                4 + (extra_bytes * 8)
            };
            let max_length = (1usize << encodable_bits) - 1;
            if raw_length > max_length {
                continue;
            }

            if extra_bytes == 0 {
                return vec![raw_length as u8];
            }

            let mut bytes = Vec::with_capacity(total_length_bytes);
            bytes.push(((extra_bytes as u8) << 6) | ((raw_length & 0x0f) as u8));

            let mut remaining = raw_length >> 4;
            for _ in 0..extra_bytes {
                bytes.push((remaining & 0xff) as u8);
                remaining >>= 8;
            }

            assert_eq!(remaining, 0);
            return bytes;
        }

        panic!("AML package length too large for test encoding");
    }

    fn aml_zero() -> Vec<u8> {
        vec![0x00]
    }

    fn aml_byte(value: u8) -> Vec<u8> {
        if value == 0 {
            aml_zero()
        } else {
            vec![0x0a, value]
        }
    }

    fn aml_dword(value: u32) -> Vec<u8> {
        let mut bytes = vec![0x0c];
        bytes.extend_from_slice(&value.to_le_bytes());
        bytes
    }

    fn aml_name(name: &str, value: Vec<u8>) -> Vec<u8> {
        let mut bytes = vec![0x08];
        append_name_seg(&mut bytes, name);
        bytes.extend(value);
        bytes
    }

    fn aml_package(elements: Vec<Vec<u8>>) -> Vec<u8> {
        let payload_len = 1 + elements.iter().map(Vec::len).sum::<usize>();
        let mut bytes = vec![0x12];
        bytes.extend(encode_pkg_length(payload_len));
        bytes.push(elements.len() as u8);
        for element in elements {
            bytes.extend(element);
        }
        bytes
    }

    fn aml_scope_sb_pci0(terms: Vec<Vec<u8>>) -> Vec<u8> {
        let mut term_bytes = Vec::new();
        for term in terms {
            term_bytes.extend(term);
        }

        let mut name = vec![b'\\', 0x2e];
        append_name_seg(&mut name, "_SB");
        append_name_seg(&mut name, "PCI0");

        let mut bytes = vec![0x10];
        bytes.extend(encode_pkg_length(name.len() + term_bytes.len()));
        bytes.extend(name);
        bytes.extend(term_bytes);
        bytes
    }

    fn build_prt_ssdt_blob(routes: &[(u16, u16, u8, u32)]) -> Vec<u8> {
        let prt_entries = routes
            .iter()
            .map(|(device, function, pin, gsi)| {
                aml_package(vec![
                    aml_dword((u32::from(*device) << 16) | u32::from(*function)),
                    aml_byte(*pin),
                    aml_zero(),
                    aml_dword(*gsi),
                ])
            })
            .collect::<Vec<_>>();

        let aml = aml_scope_sb_pci0(vec![aml_name("_PRT", aml_package(prt_entries))]);
        build_sdt_blob(b"SSDT", &aml)
    }

    fn parse_test_aml_table_blob(
        context: &mut AmlContext,
        table_blob: &[u8],
    ) -> Result<(), &'static str> {
        let header_len = mem::size_of::<acpi::sdt::SdtHeader>();
        if table_blob.len() < header_len {
            return Err("synthetic SSDT blob shorter than header");
        }

        if &table_blob[..4] != b"SSDT" {
            return Err("synthetic SSDT blob has wrong signature");
        }

        let declared_length = u32::from_le_bytes(table_blob[4..8].try_into().unwrap()) as usize;
        if declared_length != table_blob.len() {
            return Err("synthetic SSDT blob length mismatch");
        }

        let checksum = table_blob
            .iter()
            .fold(0u8, |sum, byte| sum.wrapping_add(*byte));
        if checksum != 0 {
            return Err("synthetic SSDT blob has invalid checksum");
        }

        context
            .parse_table(&table_blob[header_len..])
            .map_err(|_| "failed to parse synthetic AML payload")
    }

    fn add_prt_method(context: &mut AmlContext, path: &str, routes: &[(u16, u16, u8, u32)]) {
        let entries = routes
            .iter()
            .map(|(device, function, pin, gsi)| {
                AmlValue::Package(alloc::vec![
                    AmlValue::Integer(((*device as u64) << 16) | (*function as u64)),
                    AmlValue::Integer(*pin as u64),
                    AmlValue::Integer(0),
                    AmlValue::Integer(*gsi as u64),
                ])
            })
            .collect::<Vec<_>>();

        context
            .namespace
            .add_value(
                AmlName::from_str(path).unwrap(),
                AmlValue::native_method(0, false, 0, move |_| {
                    Ok(AmlValue::Package(entries.clone()))
                }),
            )
            .unwrap();
    }

    fn make_registry_device(
        bus: u8,
        device: u8,
        function: u8,
        interrupt_pin: u8,
    ) -> (DeviceKey, DeviceInfo) {
        (
            DeviceKey::new(bus, device, function, 0x1234, 0x5678),
            DeviceInfo {
                vendor_id: 0x1234,
                device_id: 0x5678,
                class_code: 0x01,
                subclass: 0x06,
                prog_if: 0x01,
                bus,
                device,
                function,
                bars: [None, None, None, None, None, None],
                interrupt_line: None,
                interrupt_pin: Some(interrupt_pin),
                irq: None,
            },
        )
    }

    #[test]
    fn rsdp_checksum_validation_accepts_a_valid_root_pointer() {
        let rsdt_address = 0x100usize;
        let mut image = vec![0u8; 0x200];
        write_blob(&mut image, 0, &build_rsdp_blob(rsdt_address as u32));
        write_blob(&mut image, rsdt_address, &build_rsdt_blob(&[]));

        let tables = unsafe { AcpiTables::from_rsdp(TestAcpiTableHandler::new(image), 0) }.unwrap();
        assert_eq!(tables.revision, 0);
        assert!(tables.sdts.is_empty());
        assert!(tables.dsdt.is_none());
        assert!(tables.ssdts.is_empty());
    }

    #[test]
    fn rsdp_checksum_validation_rejects_corrupt_root_pointer() {
        let mut image = build_rsdp_blob(0x100);
        image[8] = image[8].wrapping_add(1);

        let error = match unsafe { AcpiTables::from_rsdp(TestAcpiTableHandler::new(image), 0) } {
            Ok(_) => panic!("corrupt RSDP checksum unexpectedly validated"),
            Err(error) => error,
        };
        assert!(matches!(
            error,
            acpi::AcpiError::Rsdp(acpi::RsdpError::InvalidChecksum)
        ));
    }

    #[test]
    fn parses_prt_entries_from_a_synthetic_ssdt_blob() {
        let ssdt = build_prt_ssdt_blob(&[(2, 0, 0, 16)]);
        let mut context = AmlContext::new(Box::new(NullHandler), DebugVerbosity::None);
        parse_test_aml_table_blob(&mut context, &ssdt).unwrap();

        let prt_path = AmlName::from_str("\\_SB.PCI0._PRT").unwrap();
        let prt = PciRoutingTable::from_prt_path(&prt_path, &mut context).unwrap();
        let descriptor = prt.route(2, 0, Pin::IntA, &mut context).unwrap();

        assert_eq!(descriptor.irq, 16);
        assert_eq!(descriptor.trigger, aml::resource::InterruptTrigger::Level);
        assert_eq!(
            descriptor.polarity,
            aml::resource::InterruptPolarity::ActiveLow
        );
    }

    #[test]
    fn parse_s5_package_extracts_sleep_types() {
        let context = AmlContext::new(Box::new(NullHandler), DebugVerbosity::None);
        let value = AmlValue::Package(alloc::vec![AmlValue::Integer(5), AmlValue::Integer(0)]);

        let sleep = parse_s5_sleep_types(&context, &value).unwrap();
        assert_eq!(sleep.s5_type_a, 5);
        assert_eq!(sleep.s5_type_b, Some(0));
    }

    #[test]
    fn timed_method_reports_timeout() {
        let mut context = AmlContext::new(Box::new(NullHandler), DebugVerbosity::None);
        let slow_path = AmlName::from_str("\\SLOW").unwrap();
        context
            .namespace
            .add_value(
                slow_path.clone(),
                AmlValue::native_method(0, false, 0, |_| {
                    thread::sleep(Duration::from_millis(150));
                    Ok(AmlValue::Integer(1))
                }),
            )
            .unwrap();

        let clock = TestClock::new();
        assert!(matches!(
            invoke_method_with_timeout(&mut context, &slow_path, &clock),
            Err(AmlMethodError::Timeout)
        ));
    }

    #[test]
    fn fixed_feature_power_button_clears_status_and_queues_shutdown() {
        let power = PowerManagementInfo {
            sci_gsi: 9,
            power_button_mode: PowerButtonMode::FixedFeature,
            pm1a_event_block: GenericAddress {
                address_space: AddressSpace::SystemIo,
                bit_width: 32,
                bit_offset: 0,
                access_size: acpi::platform::address::AccessSize::WordAccess,
                address: 0x1000,
            },
            pm1b_event_block: None,
            pm1a_control_block: GenericAddress {
                address_space: AddressSpace::SystemIo,
                bit_width: 16,
                bit_offset: 0,
                access_size: acpi::platform::address::AccessSize::WordAccess,
                address: 0x2000,
            },
            pm1b_control_block: None,
            sleep_values: Some(SleepTypeValues {
                s5_type_a: 5,
                s5_type_b: None,
            }),
            current_state: AcpiPowerState::S0,
        };

        let mut access = TestRegisterAccess::new();
        access.values.insert(0x1000, PM1_POWER_BUTTON_BIT);

        register_init_task(crate::task::TaskId(1));
        assert!(handle_power_button_event_with_access(power, &mut access).unwrap());
        assert!(init_shutdown_signal_pending());
        assert_eq!(access.values.get(&0x1000), Some(&PM1_POWER_BUTTON_BIT));
        assert!(take_init_shutdown_signal());
    }

    #[test]
    fn sleep_control_value_sets_typ_and_enable() {
        let value = build_sleep_control_value(0x0003, 5);
        assert_eq!(value & PM1_SLEEP_ENABLE_BIT, PM1_SLEEP_ENABLE_BIT);
        assert_eq!(value & PM1_SLEEP_TYPE_MASK, 5 << 10);
    }

    #[test]
    fn resolve_ioapic_maps_gsi_to_correct_input() {
        let io_apics = alloc::vec![
            IoApicDescriptor {
                address: 0xfec0_0000,
                gsi_base: 0
            },
            IoApicDescriptor {
                address: 0xfec0_1000,
                gsi_base: 24
            },
        ];

        assert_eq!(resolve_ioapic_for_gsi(&io_apics, 7), Some((0xfec0_0000, 7)));
        assert_eq!(
            resolve_ioapic_for_gsi(&io_apics, 28),
            Some((0xfec0_1000, 4))
        );
    }

    #[test]
    fn resolves_prt_routes_and_updates_registry_irq() {
        let mut context = AmlContext::new(Box::new(NullHandler), DebugVerbosity::None);
        context
            .namespace
            .add_level(AmlName::from_str("\\_SB").unwrap(), LevelType::Scope)
            .unwrap();
        context
            .namespace
            .add_level(AmlName::from_str("\\_SB.PCI0").unwrap(), LevelType::Device)
            .unwrap();
        context
            .namespace
            .add_value(
                AmlName::from_str("\\_SB.PCI0._BBN").unwrap(),
                AmlValue::Integer(0),
            )
            .unwrap();
        add_prt_method(&mut context, "\\_SB.PCI0._PRT", &[(2, 0, 0, 16)]);

        let mut registry = DeviceRegistry::new();
        let (key, info) = make_registry_device(0, 2, 0, 0);
        registry.register_device_info(key.clone(), info);

        let routes = resolve_pci_routes_for_registry(
            &mut context,
            &mut registry,
            &[IoApicDescriptor {
                address: 0xfec0_0000,
                gsi_base: 0,
            }],
            0x50,
            32,
            &TestClock::new(),
        )
        .unwrap();

        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].device, 2);
        assert_eq!(routes[0].pin, 0);
        assert_eq!(routes[0].gsi, 16);
        assert_eq!(routes[0].vector, 0x50);
        assert_eq!(routes[0].ioapic_input, 16);
        assert_eq!(registry.get_device_info(&key).unwrap().irq, Some(16));
    }

    #[test]
    fn shared_gsi_reuses_the_same_vector() {
        let mut context = AmlContext::new(Box::new(NullHandler), DebugVerbosity::None);
        context
            .namespace
            .add_level(AmlName::from_str("\\_SB").unwrap(), LevelType::Scope)
            .unwrap();
        context
            .namespace
            .add_level(AmlName::from_str("\\_SB.PCI0").unwrap(), LevelType::Device)
            .unwrap();
        context
            .namespace
            .add_value(
                AmlName::from_str("\\_SB.PCI0._BBN").unwrap(),
                AmlValue::Integer(0),
            )
            .unwrap();
        add_prt_method(
            &mut context,
            "\\_SB.PCI0._PRT",
            &[(2, 0, 0, 18), (3, 0, 1, 18)],
        );

        let mut registry = DeviceRegistry::new();
        let (key_a, info_a) = make_registry_device(0, 2, 0, 0);
        let (key_b, info_b) = make_registry_device(0, 3, 0, 1);
        registry.register_device_info(key_a, info_a);
        registry.register_device_info(key_b, info_b);

        let routes = resolve_pci_routes_for_registry(
            &mut context,
            &mut registry,
            &[IoApicDescriptor {
                address: 0xfec0_0000,
                gsi_base: 0,
            }],
            0x50,
            32,
            &TestClock::new(),
        )
        .unwrap();

        assert_eq!(routes.len(), 2);
        assert_eq!(routes[0].gsi, 18);
        assert_eq!(routes[1].gsi, 18);
        assert_eq!(routes[0].vector, routes[1].vector);
    }
}
