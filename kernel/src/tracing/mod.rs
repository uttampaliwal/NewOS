//! Lightweight in-kernel tracing support.
//!
//! The tracing subsystem provides a bounded ring buffer for trace events that
//! can be inspected from tests and later surfaced through the kernel's
//! observability interfaces.
//!
//! Sub-modules:
//! - `function_trace`: per-function entry/exit tracing with timestamps
//! - `kprobes`: dynamic kernel probes at instruction addresses
//! - `trace_pipe`: unified streaming output with category filtering

pub mod function_trace;
pub mod kprobes;
pub mod trace_pipe;

use alloc::{string::String, vec::Vec};
use spin::Mutex;

/// A single trace event recorded by the kernel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceEvent {
    pub category: &'static str,
    pub message: String,
}

/// A small, bounded in-memory trace buffer.
pub struct TraceBuffer {
    events: Vec<TraceEvent>,
    max_events: usize,
}

impl TraceBuffer {
    pub fn new(max_events: usize) -> Self {
        Self {
            events: Vec::new(),
            max_events: max_events.max(1),
        }
    }

    pub fn record(&mut self, category: &'static str, message: impl Into<String>) {
        let entry = TraceEvent {
            category,
            message: message.into(),
        };
        self.events.push(entry);
        if self.events.len() > self.max_events {
            self.events.remove(0);
        }
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn clear(&mut self) {
        self.events.clear();
    }

    pub fn snapshot(&self) -> Vec<TraceEvent> {
        self.events.clone()
    }
}

pub(crate) static GLOBAL_TRACE_BUFFER: Mutex<Option<TraceBuffer>> = Mutex::new(None);

/// Initialize the global trace buffer.
pub fn init_trace_buffer(max_events: usize) {
    *GLOBAL_TRACE_BUFFER.lock() = Some(TraceBuffer::new(max_events));
}

/// Record a trace event in the global buffer.
pub fn trace(category: &'static str, message: impl Into<String>) {
    let mut guard = GLOBAL_TRACE_BUFFER.lock();
    if let Some(buffer) = guard.as_mut() {
        buffer.record(category, message);
    }
}

/// Return a snapshot of the currently recorded trace events.
pub fn snapshot_trace() -> Vec<TraceEvent> {
    GLOBAL_TRACE_BUFFER
        .lock()
        .as_ref()
        .map(|buffer| buffer.snapshot())
        .unwrap_or_default()
}

/// Clear all recorded trace events.
pub fn clear_trace() {
    let mut guard = GLOBAL_TRACE_BUFFER.lock();
    if let Some(buffer) = guard.as_mut() {
        buffer.clear();
    }
}

#[macro_export]
macro_rules! trace_event {
    ($category:expr, $($arg:tt)*) => {
        $crate::tracing::trace($category, alloc::format!($($arg)*))
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_buffer_records_and_snapshots_events() {
        let _s = crate::test_serial::acquire();
        let mut buffer = TraceBuffer::new(8);
        buffer.record("boot", "kernel entered");
        buffer.record("sched", "context switch");

        let snapshot = buffer.snapshot();
        assert_eq!(snapshot.len(), 2);
        assert_eq!(snapshot[0].category, "boot");
        assert_eq!(snapshot[0].message, "kernel entered");
        assert_eq!(snapshot[1].category, "sched");
        assert_eq!(snapshot[1].message, "context switch");
    }

    #[test]
    fn trace_buffer_drops_oldest_entries_when_full() {
        let _s = crate::test_serial::acquire();
        let mut buffer = TraceBuffer::new(2);
        buffer.record("a", "first");
        buffer.record("b", "second");
        buffer.record("c", "third");

        let snapshot = buffer.snapshot();
        assert_eq!(snapshot.len(), 2);
        assert_eq!(snapshot[0].category, "b");
        assert_eq!(snapshot[0].message, "second");
        assert_eq!(snapshot[1].category, "c");
        assert_eq!(snapshot[1].message, "third");
    }

    #[test]
    fn global_trace_buffer_can_be_initialized_and_cleared() {
        let _s = crate::test_serial::acquire();
        init_trace_buffer(4);
        clear_trace();
        trace("boot", "kernel entered");
        trace("sched", "context switch");

        let snapshot = snapshot_trace();
        assert_eq!(snapshot.len(), 2);
        assert_eq!(snapshot[0].category, "boot");
        assert_eq!(snapshot[0].message, "kernel entered");

        clear_trace();
        assert!(snapshot_trace().is_empty());
    }
}
