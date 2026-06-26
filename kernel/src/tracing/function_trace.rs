//! Function tracer — records entry/exit of kernel functions with timestamps.
//!
//! Each traced function gets a numeric ID. The tracer records events of the
//! form `(function_id, phase)` where phase is `Entry` or `Exit`, along with a
//! timestamp obtained from the TSC (or a monotonic counter).

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

use spin::Mutex;

use super::TraceEvent;

/// Maximum number of functions that can be registered for tracing.
const MAX_TRACED_FUNCTIONS: usize = 256;

/// Phase of a function trace event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FunctionPhase {
    Entry,
    Exit,
}

/// A single function-trace event.
#[derive(Debug, Clone)]
pub struct FunctionTraceEvent {
    pub function_id: u32,
    pub phase: FunctionPhase,
    pub timestamp: u64,
    pub cpu: u32,
}

/// Metadata for a registered traced function.
#[derive(Debug, Clone)]
pub struct FunctionRecord {
    pub name: &'static str,
    pub module: &'static str,
}

/// Global registry of traced functions.
struct FunctionRegistry {
    records: Vec<FunctionRecord>,
}

static REGISTRY: Mutex<FunctionRegistry> = Mutex::new(FunctionRegistry { records: Vec::new() });

/// Counter for generating unique function IDs.
static NEXT_ID: AtomicU32 = AtomicU32::new(1);

/// Register a function for tracing. Returns a unique ID (1-based).
pub fn register_function(name: &'static str, module: &'static str) -> u32 {
    let mut reg = REGISTRY.lock();
    if reg.records.len() >= MAX_TRACED_FUNCTIONS {
        return 0;
    }
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    reg.records.push(FunctionRecord { name, module });
    id
}

/// Look up a function record by its 1-based ID.
pub fn lookup_function(id: u32) -> Option<FunctionRecord> {
    let reg = REGISTRY.lock();
    reg.records.get((id as usize).checked_sub(1)?).cloned()
}

/// Return all registered function records.
pub fn list_functions() -> Vec<(u32, FunctionRecord)> {
    let reg = REGISTRY.lock();
    reg.records
        .iter()
        .enumerate()
        .map(|(i, r)| ((i as u32) + 1, r.clone()))
        .collect()
}

/// Per-CPU function trace buffer.
struct PerCpuBuffer {
    events: Vec<FunctionTraceEvent>,
    max_events: usize,
}

impl PerCpuBuffer {
    fn new(max_events: usize) -> Self {
        Self {
            events: Vec::new(),
            max_events: max_events.max(1),
        }
    }

    fn push(&mut self, event: FunctionTraceEvent) {
        if self.events.len() >= self.max_events {
            self.events.remove(0);
        }
        self.events.push(event);
    }

    fn snapshot(&self) -> Vec<FunctionTraceEvent> {
        self.events.clone()
    }

    fn clear(&mut self) {
        self.events.clear();
    }

    fn len(&self) -> usize {
        self.events.len()
    }
}

/// Maximum number of CPUs supported.
const MAX_CPUS: usize = 64;

/// Global per-CPU function-trace storage.
struct PerCpuTraceStore {
    buffers: Vec<PerCpuBuffer>,
    enabled: bool,
}

static PER_CPU_STORE: Mutex<PerCpuTraceStore> = Mutex::new(PerCpuTraceStore {
    buffers: Vec::new(),
    enabled: false,
});

/// Initialize per-CPU trace buffers with the given capacity per CPU.
pub fn init_function_tracer(max_events_per_cpu: usize) {
    let mut store = PER_CPU_STORE.lock();
    store.buffers.clear();
    for _ in 0..MAX_CPUS {
        store.buffers.push(PerCpuBuffer::new(max_events_per_cpu));
    }
    store.enabled = true;
}

/// Enable or disable the function tracer globally.
pub fn set_function_tracer_enabled(enabled: bool) {
    PER_CPU_STORE.lock().enabled = enabled;
}

/// Returns whether the function tracer is enabled.
pub fn is_function_tracer_enabled() -> bool {
    PER_CPU_STORE.lock().enabled
}

/// Record a function entry event.
pub fn trace_function_entry(function_id: u32, cpu: u32) {
    let store = PER_CPU_STORE.lock();
    if !store.enabled {
        return;
    }
    drop(store);
    trace_function_event(function_id, FunctionPhase::Entry, cpu);
}

/// Record a function exit event.
pub fn trace_function_exit(function_id: u32, cpu: u32) {
    let store = PER_CPU_STORE.lock();
    if !store.enabled {
        return;
    }
    drop(store);
    trace_function_event(function_id, FunctionPhase::Exit, cpu);
}

fn rdtsc() -> u64 {
    // SAFETY: `rdtsc` is a safe, non-privileged hardware instruction.
    unsafe { core::arch::x86_64::_rdtsc() }
}

fn trace_function_event(function_id: u32, phase: FunctionPhase, cpu: u32) {
    let cpu_idx = (cpu as usize).min(MAX_CPUS - 1);
    let timestamp = rdtsc();

    let event = FunctionTraceEvent {
        function_id,
        phase,
        timestamp,
        cpu,
    };

    let mut store = PER_CPU_STORE.lock();
    if store.enabled {
        store.buffers[cpu_idx].push(event);
    }
}

/// Snapshot all per-CPU function trace events, ordered by timestamp.
pub fn snapshot_function_traces() -> Vec<FunctionTraceEvent> {
    let store = PER_CPU_STORE.lock();
    let mut all: Vec<FunctionTraceEvent> = Vec::new();
    for buf in store.buffers.iter() {
        all.extend(buf.snapshot());
    }
    drop(store);
    all.sort_by_key(|e| e.timestamp);
    all
}

/// Clear all per-CPU function trace buffers.
pub fn clear_function_traces() {
    let mut store = PER_CPU_STORE.lock();
    for buf in store.buffers.iter_mut() {
        buf.clear();
    }
}

/// Return the number of events per CPU.
pub fn function_traces_per_cpu() -> Vec<(u32, usize)> {
    let store = PER_CPU_STORE.lock();
    store
        .buffers
        .iter()
        .enumerate()
        .map(|(i, buf)| (i as u32, buf.len()))
        .collect()
}

/// Convert a function trace event to a human-readable TraceEvent.
pub fn format_function_event(event: &FunctionTraceEvent) -> Option<TraceEvent> {
    let record = lookup_function(event.function_id)?;
    let phase_str = match event.phase {
        FunctionPhase::Entry => "entry",
        FunctionPhase::Exit => "exit",
    };
    Some(TraceEvent {
        category: "ftrace",
        message: alloc::format!(
            "[cpu={}] {}::{} {} t={}",
            event.cpu,
            record.module,
            record.name,
            phase_str,
            event.timestamp
        ),
    })
}

/// Return all function trace events formatted as TraceEvents.
pub fn formatted_function_traces() -> Vec<TraceEvent> {
    snapshot_function_traces()
        .iter()
        .filter_map(format_function_event)
        .collect()
}

/// Reset the function registry and ID counter (for tests).
pub fn reset_registry() {
    REGISTRY.lock().records.clear();
    NEXT_ID.store(1, Ordering::Relaxed);
}

/// Read and convert all per-CPU function traces into the global TraceBuffer.
pub fn drain_function_traces_to_global() {
    let events = formatted_function_traces();
    let mut buffer = super::GLOBAL_TRACE_BUFFER.lock();
    if let Some(buf) = buffer.as_mut() {
        for event in events {
            buf.record(event.category, event.message);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_lookup_function() {
        let _s = crate::test_serial::acquire();
        reset_registry();
        let id = register_function("do_fork", "process");
        assert_eq!(id, 1);
        let rec = lookup_function(id).unwrap();
        assert_eq!(rec.name, "do_fork");
        assert_eq!(rec.module, "process");
    }

    #[test]
    fn multiple_registrations_get_unique_ids() {
        let _s = crate::test_serial::acquire();
        reset_registry();
        let id1 = register_function("func_a", "mod1");
        let id2 = register_function("func_b", "mod2");
        let id3 = register_function("func_c", "mod3");
        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
        assert_ne!(id1, id3);
    }

    #[test]
    fn lookup_nonexistent_returns_none() {
        let _s = crate::test_serial::acquire();
        reset_registry();
        assert!(lookup_function(999).is_none());
    }

    #[test]
    fn list_functions_returns_all() {
        let _s = crate::test_serial::acquire();
        reset_registry();
        register_function("a", "m1");
        register_function("b", "m2");
        let list = list_functions();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].1.name, "a");
        assert_eq!(list[1].1.name, "b");
    }

    #[test]
    fn per_cpu_buffer_respects_capacity() {
        let _s = crate::test_serial::acquire();
        let mut buf = PerCpuBuffer::new(3);
        for i in 0..5 {
            buf.push(FunctionTraceEvent {
                function_id: i,
                phase: FunctionPhase::Entry,
                timestamp: i as u64,
                cpu: 0,
            });
        }
        assert_eq!(buf.len(), 3);
        let snap = buf.snapshot();
        assert_eq!(snap[0].function_id, 2);
        assert_eq!(snap[2].function_id, 4);
    }

    #[test]
    fn function_tracer_records_and_snapshots() {
        let _s = crate::test_serial::acquire();
        init_function_tracer(16);
        set_function_tracer_enabled(true);
        reset_registry();

        let id = register_function("test_fn", "test_mod");
        trace_function_entry(id, 0);
        trace_function_exit(id, 0);

        let events = snapshot_function_traces();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].phase, FunctionPhase::Entry);
        assert_eq!(events[1].phase, FunctionPhase::Exit);

        clear_function_traces();
        assert!(snapshot_function_traces().is_empty());
    }

    #[test]
    fn function_tracer_disabled_does_not_record() {
        let _s = crate::test_serial::acquire();
        init_function_tracer(16);
        set_function_tracer_enabled(false);
        reset_registry();

        let id = register_function("disabled_fn", "test");
        trace_function_entry(id, 0);

        assert!(snapshot_function_traces().is_empty());
        set_function_tracer_enabled(true);
    }

    #[test]
    fn multi_cpu_traces_are_merged_sorted_by_timestamp() {
        let _s = crate::test_serial::acquire();
        init_function_tracer(16);
        set_function_tracer_enabled(true);
        reset_registry();

        let id = register_function("fn", "mod");
        trace_function_entry(id, 0);
        trace_function_entry(id, 1);
        trace_function_exit(id, 0);
        trace_function_exit(id, 1);

        let events = snapshot_function_traces();
        assert_eq!(events.len(), 4);
        assert!(events[0].timestamp <= events[1].timestamp);
    }

    #[test]
    fn format_function_event_produces_readable_output() {
        let _s = crate::test_serial::acquire();
        reset_registry();
        let id = register_function("handle_syscall", "syscall");
        let event = FunctionTraceEvent {
            function_id: id,
            phase: FunctionPhase::Entry,
            timestamp: 12345,
            cpu: 2,
        };
        let formatted = format_function_event(&event).unwrap();
        assert_eq!(formatted.category, "ftrace");
        assert!(formatted.message.contains("syscall::handle_syscall"));
        assert!(formatted.message.contains("entry"));
        assert!(formatted.message.contains("t=12345"));
        assert!(formatted.message.contains("cpu=2"));
    }

    #[test]
    fn format_nonexistent_function_returns_none() {
        let _s = crate::test_serial::acquire();
        let event = FunctionTraceEvent {
            function_id: 999,
            phase: FunctionPhase::Exit,
            timestamp: 0,
            cpu: 0,
        };
        assert!(format_function_event(&event).is_none());
    }

    #[test]
    fn per_cpu_counts_are_accurate() {
        let _s = crate::test_serial::acquire();
        init_function_tracer(16);
        set_function_tracer_enabled(true);
        reset_registry();

        let id = register_function("fn", "m");
        trace_function_entry(id, 0);
        trace_function_entry(id, 0);
        trace_function_entry(id, 1);

        let counts = function_traces_per_cpu();
        assert_eq!(counts[0].1, 2);
        assert_eq!(counts[1].1, 1);
    }

    #[test]
    fn drain_to_global_populates_buffer() {
        let _s = crate::test_serial::acquire();
        super::super::init_trace_buffer(64);
        init_function_tracer(16);
        set_function_tracer_enabled(true);
        reset_registry();

        let id = register_function("drain_fn", "drain_mod");
        trace_function_entry(id, 0);

        drain_function_traces_to_global();
        let global = super::super::snapshot_trace();
        assert!(!global.is_empty());
        assert_eq!(global[0].category, "ftrace");
    }
}
