//! Kprobes — dynamic kernel probes that fire callbacks at instruction addresses.
//!
//! Kprobes allow placing breakpoint-like probes at arbitrary kernel code
//! addresses. When the probed instruction executes, the probe fires a callback
//! and records an event. This provides a flexible mechanism for tracing
//! arbitrary kernel code paths without modifying the source.
//!
//! Implementation notes (no_std safe subset):
//! - Addresses are stored as raw `usize` pointers; in a real kernel these would
//!   come from symbol resolution or user-space registration.
//! - The actual hardware breakpoint trap (`int3` on x86) is not wired here —
//!   instead we provide the register/enable/disable/record infrastructure that
//!   a trap handler would call.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use spin::Mutex;

use super::TraceEvent;

/// Maximum number of kprobes that can be registered simultaneously.
const MAX_KPROBES: usize = 128;

/// Unique kprobe ID (1-based).
static NEXT_KPROBE_ID: AtomicU32 = AtomicU32::new(1);

/// A registered kprobe.
#[derive(Debug)]
pub struct Kprobe {
    /// Unique identifier.
    pub id: u32,
    /// Symbol name for this probe (for display).
    pub symbol: &'static str,
    /// Module the symbol belongs to.
    pub module: &'static str,
    /// Address being probed (instruction pointer).
    pub address: usize,
    /// Whether the probe is currently armed.
    pub armed: AtomicBool,
    /// User-provided probe label (optional).
    pub label: &'static str,
}

impl Clone for Kprobe {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            symbol: self.symbol,
            module: self.module,
            address: self.address,
            armed: AtomicBool::new(self.armed.load(Ordering::Relaxed)),
            label: self.label,
        }
    }
}

/// A recorded kprobe hit event.
#[derive(Debug, Clone)]
pub struct KprobeEvent {
    pub kprobe_id: u32,
    pub symbol: &'static str,
    pub module: &'static str,
    pub label: &'static str,
    pub address: usize,
    pub timestamp: u64,
    pub cpu: u32,
    /// Register snapshot at the time of the hit (simulated).
    pub registers: KprobeRegisters,
}

/// Register snapshot captured when a kprobe fires.
/// In a real kernel these come from the `pt_regs` passed to the int3 handler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KprobeRegisters {
    pub rip: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub r8: u64,
    pub r9: u64,
}

impl KprobeRegisters {
    /// Create a zero register set (used in tests / initialization).
    pub const fn zero() -> Self {
        Self {
            rip: 0,
            rdi: 0,
            rsi: 0,
            rdx: 0,
            rcx: 0,
            r8: 0,
            r9: 0,
        }
    }
}

/// Bounded event buffer for kprobe hits.
struct KprobeEventBuffer {
    events: Vec<KprobeEvent>,
    max_events: usize,
}

impl KprobeEventBuffer {
    fn new(max_events: usize) -> Self {
        Self {
            events: Vec::new(),
            max_events: max_events.max(1),
        }
    }

    fn push(&mut self, event: KprobeEvent) {
        if self.events.len() >= self.max_events {
            self.events.remove(0);
        }
        self.events.push(event);
    }

    fn snapshot(&self) -> Vec<KprobeEvent> {
        self.events.clone()
    }

    fn clear(&mut self) {
        self.events.clear();
    }

    fn len(&self) -> usize {
        self.events.len()
    }
}

/// Global kprobe registry and event storage.
struct KprobeStore {
    probes: Vec<Kprobe>,
    event_buffer: KprobeEventBuffer,
    enabled: bool,
}

static KPROBE_STORE: Mutex<KprobeStore> = Mutex::new(KprobeStore {
    probes: Vec::new(),
    event_buffer: KprobeEventBuffer { events: Vec::new(), max_events: 512 },
    enabled: false,
});

/// Initialize the kprobe subsystem with the given event buffer capacity.
pub fn init_kprobes(max_events: usize) {
    let mut store = KPROBE_STORE.lock();
    store.event_buffer = KprobeEventBuffer::new(max_events);
    store.enabled = true;
}

/// Enable or disable the kprobe subsystem globally.
pub fn set_kprobes_enabled(enabled: bool) {
    KPROBE_STORE.lock().enabled = enabled;
}

/// Returns whether the kprobe subsystem is enabled.
pub fn is_kprobes_enabled() -> bool {
    KPROBE_STORE.lock().enabled
}

/// Register a kprobe at the given address. Returns the probe ID, or 0 on failure.
pub fn register_kprobe(
    symbol: &'static str,
    module: &'static str,
    address: usize,
    label: &'static str,
) -> u32 {
    let mut store = KPROBE_STORE.lock();
    if store.probes.len() >= MAX_KPROBES {
        return 0;
    }

    // Reject duplicate addresses
    if store.probes.iter().any(|p| p.address == address) {
        return 0;
    }

    let id = NEXT_KPROBE_ID.fetch_add(1, Ordering::Relaxed);
    store.probes.push(Kprobe {
        id,
        symbol,
        module,
        address,
        armed: AtomicBool::new(true),
        label,
    });
    id
}

/// Unregister a kprobe by ID. Returns true if found and removed.
pub fn unregister_kprobe(id: u32) -> bool {
    let mut store = KPROBE_STORE.lock();
    let len_before = store.probes.len();
    store.probes.retain(|p| p.id != id);
    store.probes.len() < len_before
}

/// Arm or disarm a specific kprobe.
pub fn arm_kprobe(id: u32, armed: bool) -> bool {
    let store = KPROBE_STORE.lock();
    if let Some(probe) = store.probes.iter().find(|p| p.id == id) {
        probe.armed.store(armed, Ordering::Relaxed);
        true
    } else {
        false
    }
}

/// Look up a kprobe by ID.
pub fn lookup_kprobe(id: u32) -> Option<Kprobe> {
    let store = KPROBE_STORE.lock();
    store.probes.iter().find(|p| p.id == id).cloned()
}

/// Return all registered kprobe IDs and symbols.
pub fn list_kprobes() -> Vec<(u32, &'static str, &'static str, usize, bool)> {
    let store = KPROBE_STORE.lock();
    store
        .probes
        .iter()
        .map(|p| (p.id, p.symbol, p.module, p.address, p.armed.load(Ordering::Relaxed)))
        .collect()
}

/// Find a kprobe by address.
pub fn find_kprobe_by_address(address: usize) -> Option<Kprobe> {
    let store = KPROBE_STORE.lock();
    store.probes.iter().find(|p| p.address == address).cloned()
}

fn rdtsc() -> u64 {
    // SAFETY: `rdtsc` is a safe, non-privileged hardware instruction.
    unsafe { core::arch::x86_64::_rdtsc() }
}

/// Simulate a kprobe hit (called from trap handler or tests).
/// Records the event with the given register snapshot.
pub fn fire_kprobe(kprobe_id: u32, cpu: u32, regs: KprobeRegisters) {
    let store = KPROBE_STORE.lock();
    if !store.enabled {
        return;
    }

    let probe = match store.probes.iter().find(|p| p.id == kprobe_id) {
        Some(p) if p.armed.load(Ordering::Relaxed) => p.clone(),
        _ => return,
    };

    let timestamp = rdtsc();

    let event = KprobeEvent {
        kprobe_id: probe.id,
        symbol: probe.symbol,
        module: probe.module,
        label: probe.label,
        address: probe.address,
        timestamp,
        cpu,
        registers: regs,
    };

    // Drop the lock before pushing to avoid holding it during buffer operations
    drop(store);

    KPROBE_STORE.lock().event_buffer.push(event);
}

/// Fire a kprobe by address (convenience function).
pub fn fire_kprobe_at_address(address: usize, cpu: u32, regs: KprobeRegisters) {
    let id = {
        let store = KPROBE_STORE.lock();
        match store.probes.iter().find(|p| p.address == address && p.armed.load(Ordering::Relaxed)) {
            Some(p) => p.id,
            None => return,
        }
    };
    fire_kprobe(id, cpu, regs);
}

/// Snapshot all kprobe events, ordered by timestamp.
pub fn snapshot_kprobe_events() -> Vec<KprobeEvent> {
    let store = KPROBE_STORE.lock();
    let mut events = store.event_buffer.snapshot();
    drop(store);
    events.sort_by_key(|e| e.timestamp);
    events
}

/// Clear all kprobe events.
pub fn clear_kprobe_events() {
    KPROBE_STORE.lock().event_buffer.clear();
}

/// Return the current number of recorded kprobe events.
pub fn kprobe_event_count() -> usize {
    KPROBE_STORE.lock().event_buffer.len()
}

/// Convert a kprobe event to a human-readable TraceEvent.
pub fn format_kprobe_event(event: &KprobeEvent) -> TraceEvent {
    TraceEvent {
        category: "kprobe",
        message: alloc::format!(
            "[cpu={}] {}::{} ({}) addr={:#x} t={} rdi={:#x} rsi={:#x}",
            event.cpu,
            event.module,
            event.symbol,
            event.label,
            event.address,
            event.timestamp,
            event.registers.rdi,
            event.registers.rsi,
        ),
    }
}

/// Return all kprobe events formatted as TraceEvents.
pub fn formatted_kprobe_events() -> Vec<TraceEvent> {
    snapshot_kprobe_events()
        .iter()
        .map(format_kprobe_event)
        .collect()
}

/// Drain kprobe events into the global TraceBuffer.
pub fn drain_kprobe_events_to_global() {
    let events = formatted_kprobe_events();
    let mut buffer = super::GLOBAL_TRACE_BUFFER.lock();
    if let Some(buf) = buffer.as_mut() {
        for event in events {
            buf.record(event.category, event.message);
        }
    }
}

/// Reset the kprobe registry (for tests).
pub fn reset_kprobes() {
    let mut store = KPROBE_STORE.lock();
    store.probes.clear();
    store.event_buffer.clear();
    store.enabled = false;
    NEXT_KPROBE_ID.store(1, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_lookup_kprobe() {
        let _s = crate::test_serial::acquire();
        reset_kprobes();
        let id = register_kprobe("do_sys_open", "fs", 0xFFFF_FFFF_8000_1000, "open-trace");
        assert_eq!(id, 1);
        let probe = lookup_kprobe(id).unwrap();
        assert_eq!(probe.symbol, "do_sys_open");
        assert_eq!(probe.module, "fs");
        assert_eq!(probe.address, 0xFFFF_FFFF_8000_1000);
        assert!(probe.armed.load(Ordering::Relaxed));
    }

    #[test]
    fn duplicate_address_rejected() {
        let _s = crate::test_serial::acquire();
        reset_kprobes();
        let id1 = register_kprobe("func1", "m", 0x1000, "label1");
        let id2 = register_kprobe("func2", "m", 0x1000, "label2");
        assert_ne!(id1, 0);
        assert_eq!(id2, 0);
    }

    #[test]
    fn test_unregister_kprobe() {
        let _s = crate::test_serial::acquire();
        reset_kprobes();
        let id = register_kprobe("func", "m", 0x2000, "label");
        assert!(unregister_kprobe(id));
        assert!(lookup_kprobe(id).is_none());
        assert!(!unregister_kprobe(id)); // already removed
    }

    #[test]
    fn arm_disarm_kprobe() {
        let _s = crate::test_serial::acquire();
        reset_kprobes();
        let id = register_kprobe("func", "m", 0x3000, "label");
        assert!(arm_kprobe(id, false));
        assert!(!lookup_kprobe(id).unwrap().armed.load(Ordering::Relaxed));
        assert!(arm_kprobe(id, true));
        assert!(lookup_kprobe(id).unwrap().armed.load(Ordering::Relaxed));
    }

    #[test]
    fn list_kprobes_returns_all() {
        let _s = crate::test_serial::acquire();
        reset_kprobes();
        register_kprobe("a", "m1", 0x1000, "l1");
        register_kprobe("b", "m2", 0x2000, "l2");
        let list = list_kprobes();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].1, "a");
        assert_eq!(list[1].1, "b");
    }

    #[test]
    fn find_by_address() {
        let _s = crate::test_serial::acquire();
        reset_kprobes();
        register_kprobe("func", "m", 0x5000, "label");
        assert!(find_kprobe_by_address(0x5000).is_some());
        assert!(find_kprobe_by_address(0x9999).is_none());
    }

    #[test]
    fn fire_kprobe_records_event() {
        let _s = crate::test_serial::acquire();
        reset_kprobes();
        init_kprobes(64);
        set_kprobes_enabled(true);

        let id = register_kprobe("test_fn", "test_mod", 0x4000, "test-label");
        let regs = KprobeRegisters {
            rip: 0x4000,
            rdi: 0xDEAD,
            rsi: 0xBEEF,
            ..KprobeRegisters::zero()
        };
        fire_kprobe(id, 0, regs);

        let events = snapshot_kprobe_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kprobe_id, id);
        assert_eq!(events[0].symbol, "test_fn");
        assert_eq!(events[0].registers.rdi, 0xDEAD);
        assert_eq!(events[0].registers.rsi, 0xBEEF);

        clear_kprobe_events();
        assert!(snapshot_kprobe_events().is_empty());
    }

    #[test]
    fn fire_disarmed_kprobe_does_not_record() {
        let _s = crate::test_serial::acquire();
        reset_kprobes();
        init_kprobes(64);
        set_kprobes_enabled(true);

        let id = register_kprobe("func", "m", 0x6000, "label");
        arm_kprobe(id, false);
        fire_kprobe(id, 0, KprobeRegisters::zero());

        assert!(snapshot_kprobe_events().is_empty());
        arm_kprobe(id, true);
    }

    #[test]
    fn fire_disabled_kprobe_subsystem_does_not_record() {
        let _s = crate::test_serial::acquire();
        reset_kprobes();
        init_kprobes(64);
        set_kprobes_enabled(false);

        let id = register_kprobe("func", "m", 0x7000, "label");
        fire_kprobe(id, 0, KprobeRegisters::zero());

        assert!(snapshot_kprobe_events().is_empty());
        set_kprobes_enabled(true);
    }

    #[test]
    fn event_buffer_evicts_oldest() {
        let _s = crate::test_serial::acquire();
        reset_kprobes();
        init_kprobes(3);
        set_kprobes_enabled(true);

        let id = register_kprobe("func", "m", 0x8000, "label");
        for _ in 0..5 {
            fire_kprobe(id, 0, KprobeRegisters::zero());
        }

        let events = snapshot_kprobe_events();
        assert_eq!(events.len(), 3);
    }

    #[test]
    fn format_kprobe_event_is_readable() {
        let _s = crate::test_serial::acquire();
        let event = KprobeEvent {
            kprobe_id: 1,
            symbol: "my_func",
            module: "my_mod",
            label: "my-label",
            address: 0xFFFF_FFFF_8000_0000,
            timestamp: 9999,
            cpu: 3,
            registers: KprobeRegisters {
                rip: 0xFFFF_FFFF_8000_0000,
                rdi: 0x1111,
                rsi: 0x2222,
                ..KprobeRegisters::zero()
            },
        };
        let formatted = format_kprobe_event(&event);
        assert_eq!(formatted.category, "kprobe");
        assert!(formatted.message.contains("my_mod::my_func"));
        assert!(formatted.message.contains("my-label"));
        assert!(formatted.message.contains("cpu=3"));
        assert!(formatted.message.contains("t=9999"));
    }

    #[test]
    fn multi_cpu_kprobe_events() {
        let _s = crate::test_serial::acquire();
        reset_kprobes();
        init_kprobes(64);
        set_kprobes_enabled(true);

        let id = register_kprobe("func", "m", 0xA000, "label");
        fire_kprobe(id, 0, KprobeRegisters::zero());
        fire_kprobe(id, 1, KprobeRegisters::zero());
        fire_kprobe(id, 2, KprobeRegisters::zero());

        assert_eq!(kprobe_event_count(), 3);
    }

    #[test]
    fn drain_to_global_populates_buffer() {
        let _s = crate::test_serial::acquire();
        super::super::init_trace_buffer(64);
        reset_kprobes();
        init_kprobes(64);
        set_kprobes_enabled(true);

        let id = register_kprobe("drain_fn", "drain_mod", 0xB000, "drain-label");
        fire_kprobe(id, 0, KprobeRegisters::zero());

        drain_kprobe_events_to_global();
        let global = super::super::snapshot_trace();
        assert!(!global.is_empty());
        assert_eq!(global[0].category, "kprobe");
    }

    #[test]
    fn fire_at_address_convenience() {
        let _s = crate::test_serial::acquire();
        reset_kprobes();
        init_kprobes(64);
        set_kprobes_enabled(true);

        let _id = register_kprobe("func", "m", 0xC000, "label");
        fire_kprobe_at_address(0xC000, 0, KprobeRegisters::zero());
        assert_eq!(kprobe_event_count(), 1);

        // Non-existent address does nothing
        fire_kprobe_at_address(0x9999, 0, KprobeRegisters::zero());
        assert_eq!(kprobe_event_count(), 1);
    }
}
