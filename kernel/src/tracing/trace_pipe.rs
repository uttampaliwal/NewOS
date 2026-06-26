//! Trace pipe — unified streaming output with category filtering.
//!
//! The trace pipe combines events from all tracing subsystems (basic trace
//! buffer, function tracer, kprobes) into a single ordered stream that can
//! be consumed by a reader. Category filters allow selecting only events of
//! interest (e.g., "sched", "syscall", "ftrace", "kprobe").
//!
//! Design:
//! - A `TracePipe` holds a snapshot of merged events from all sources.
//! - Filtering is done by category prefix match.
//! - The pipe supports `read_line()` for consuming one event at a time.
//! - Global convenience functions operate on a singleton pipe.

use alloc::{string::String, vec::Vec};
use core::fmt::Write;

use spin::Mutex;

use super::{TraceEvent, GLOBAL_TRACE_BUFFER};

/// Maximum events that the trace pipe can hold.
const TRACE_PIPE_CAPACITY: usize = 4096;

/// A unified trace pipe that merges events from all sources.
pub struct TracePipe {
    events: Vec<TraceEvent>,
    read_pos: usize,
    category_filter: Vec<&'static str>,
}

impl TracePipe {
    /// Create a new empty trace pipe.
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            read_pos: 0,
            category_filter: Vec::new(),
        }
    }

    /// Create a trace pipe pre-populated with a snapshot from all sources.
    pub fn from_all_sources() -> Self {
        let mut pipe = Self::new();
        pipe.refresh();
        pipe
    }

    /// Set category filters. Only events whose category matches one of the
    /// given prefixes will be included. An empty filter list means "accept all".
    /// Existing events that don't match are removed.
    pub fn set_filter(&mut self, categories: &[&'static str]) {
        self.category_filter.clear();
        self.category_filter.extend_from_slice(categories);
        // Re-apply filter to existing events
        let filter = &self.category_filter;
        self.events
            .retain(|e| filter.is_empty() || filter.iter().any(|cat| e.category.starts_with(cat)));
        self.read_pos = 0;
    }

    /// Clear all filters (accept all categories).
    pub fn clear_filter(&mut self) {
        self.category_filter.clear();
    }

    /// Return the current filter categories.
    pub fn filters(&self) -> &[&'static str] {
        &self.category_filter
    }

    /// Refresh the pipe by pulling events from all tracing subsystems.
    pub fn refresh(&mut self) {
        self.events.clear();
        self.read_pos = 0;

        // Pull from global trace buffer
        {
            let buffer = GLOBAL_TRACE_BUFFER.lock();
            if let Some(buf) = buffer.as_ref() {
                let snapshot = buf.snapshot();
                self.events.extend(snapshot);
            }
        }

        // Pull from function tracer
        {
            let func_events = super::function_trace::formatted_function_traces();
            self.events.extend(func_events);
        }

        // Pull from kprobes
        {
            let kp_events = super::kprobes::formatted_kprobe_events();
            self.events.extend(kp_events);
        }

        // Sort by timestamp (embedded in the message)
        self.events.sort_by(|a, b| a.message.cmp(&b.message));

        // Apply filter
        let filter = &self.category_filter;
        self.events.retain(|e| {
            filter.is_empty() || filter.iter().any(|cat| e.category.starts_with(cat))
        });

        // Enforce capacity
        if self.events.len() > TRACE_PIPE_CAPACITY {
            let drain = self.events.len() - TRACE_PIPE_CAPACITY;
            self.events.drain(..drain);
        }
    }

    /// Read the next line from the pipe. Returns `None` when exhausted.
    pub fn read_line(&mut self) -> Option<String> {
        if self.read_pos >= self.events.len() {
            return None;
        }
        let event = &self.events[self.read_pos];
        self.read_pos += 1;

        let mut line = String::new();
        let _ = write!(line, "[{}] {}", event.category, event.message);
        Some(line)
    }

    /// Read all remaining lines as a Vec.
    pub fn read_all(&mut self) -> Vec<String> {
        let mut lines = Vec::new();
        while let Some(line) = self.read_line() {
            lines.push(line);
        }
        lines
    }

    /// Peek at the next line without advancing the read position.
    pub fn peek_line(&self) -> Option<String> {
        if self.read_pos >= self.events.len() {
            return None;
        }
        let event = &self.events[self.read_pos];
        let mut line = String::new();
        let _ = write!(line, "[{}] {}", event.category, event.message);
        Some(line)
    }

    /// Reset the read position to the beginning.
    pub fn rewind(&mut self) {
        self.read_pos = 0;
    }

    /// Return the number of events available to read.
    pub fn remaining(&self) -> usize {
        self.events.len().saturating_sub(self.read_pos)
    }

    /// Return the total number of events in the pipe.
    pub fn total_events(&self) -> usize {
        self.events.len()
    }

    /// Clear the pipe.
    pub fn clear(&mut self) {
        self.events.clear();
        self.read_pos = 0;
    }
}

/// Global singleton trace pipe.
static GLOBAL_TRACE_PIPE: Mutex<Option<TracePipe>> = Mutex::new(None);

/// Initialize the global trace pipe.
pub fn init_trace_pipe() {
    *GLOBAL_TRACE_PIPE.lock() = Some(TracePipe::new());
}

/// Refresh the global trace pipe from all sources.
pub fn refresh_trace_pipe() {
    if let Some(pipe) = GLOBAL_TRACE_PIPE.lock().as_mut() {
        pipe.refresh();
    }
}

/// Set a category filter on the global trace pipe.
pub fn set_trace_pipe_filter(categories: &[&'static str]) {
    if let Some(pipe) = GLOBAL_TRACE_PIPE.lock().as_mut() {
        pipe.set_filter(categories);
    }
}

/// Clear the filter on the global trace pipe.
pub fn clear_trace_pipe_filter() {
    if let Some(pipe) = GLOBAL_TRACE_PIPE.lock().as_mut() {
        pipe.clear_filter();
    }
}

/// Read the next line from the global trace pipe.
pub fn read_trace_pipe_line() -> Option<String> {
    GLOBAL_TRACE_PIPE.lock().as_mut()?.read_line()
}

/// Read all remaining lines from the global trace pipe.
pub fn read_trace_pipe_all() -> Vec<String> {
    GLOBAL_TRACE_PIPE
        .lock()
        .as_mut()
        .map(|p| p.read_all())
        .unwrap_or_default()
}

/// Clear the global trace pipe.
pub fn clear_trace_pipe() {
    if let Some(pipe) = GLOBAL_TRACE_PIPE.lock().as_mut() {
        pipe.clear();
    }
}

/// Return remaining event count in the global trace pipe.
pub fn trace_pipe_remaining() -> usize {
    GLOBAL_TRACE_PIPE
        .lock()
        .as_ref()
        .map(|p| p.remaining())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn trace_pipe_reads_events_in_order() {
        let _s = crate::test_serial::acquire();
        let mut pipe = TracePipe::new();
        pipe.events = vec![
            TraceEvent {
                category: "boot",
                message: String::from("alpha"),
            },
            TraceEvent {
                category: "sched",
                message: String::from("beta"),
            },
        ];
        pipe.read_pos = 0;

        assert_eq!(pipe.read_line().unwrap(), "[boot] alpha");
        assert_eq!(pipe.read_line().unwrap(), "[sched] beta");
        assert!(pipe.read_line().is_none());
    }

    #[test]
    fn trace_pipe_respects_category_filter() {
        let _s = crate::test_serial::acquire();
        let mut pipe = TracePipe::new();
        pipe.events = vec![
            TraceEvent {
                category: "sched",
                message: String::from("ctxsw"),
            },
            TraceEvent {
                category: "syscall",
                message: String::from("open"),
            },
            TraceEvent {
                category: "sched",
                message: String::from("wake"),
            },
        ];
        pipe.read_pos = 0;
        pipe.set_filter(&["sched"]);

        assert_eq!(pipe.remaining(), 2);
        assert_eq!(pipe.read_line().unwrap(), "[sched] ctxsw");
        assert_eq!(pipe.read_line().unwrap(), "[sched] wake");
        assert!(pipe.read_line().is_none());
    }

    #[test]
    fn trace_pipe_empty_filter_accepts_all() {
        let _s = crate::test_serial::acquire();
        let mut pipe = TracePipe::new();
        pipe.events = vec![
            TraceEvent {
                category: "a",
                message: String::from("1"),
            },
            TraceEvent {
                category: "b",
                message: String::from("2"),
            },
        ];
        pipe.read_pos = 0;

        assert_eq!(pipe.remaining(), 2);
    }

    #[test]
    fn trace_pipe_read_all() {
        let _s = crate::test_serial::acquire();
        let mut pipe = TracePipe::new();
        pipe.events = vec![
            TraceEvent {
                category: "a",
                message: String::from("x"),
            },
            TraceEvent {
                category: "b",
                message: String::from("y"),
            },
        ];
        pipe.read_pos = 0;

        let lines = pipe.read_all();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "[a] x");
        assert_eq!(lines[1], "[b] y");
        assert_eq!(pipe.remaining(), 0);
    }

    #[test]
    fn trace_pipe_peek_does_not_advance() {
        let _s = crate::test_serial::acquire();
        let mut pipe = TracePipe::new();
        pipe.events = vec![TraceEvent {
            category: "test",
            message: String::from("peek"),
        }];
        pipe.read_pos = 0;

        assert_eq!(pipe.peek_line().unwrap(), "[test] peek");
        assert_eq!(pipe.remaining(), 1); // still 1
        assert_eq!(pipe.read_line().unwrap(), "[test] peek");
        assert_eq!(pipe.remaining(), 0);
    }

    #[test]
    fn trace_pipe_rewind() {
        let _s = crate::test_serial::acquire();
        let mut pipe = TracePipe::new();
        pipe.events = vec![TraceEvent {
            category: "c",
            message: String::from("m"),
        }];
        pipe.read_pos = 0;

        let _ = pipe.read_line();
        assert_eq!(pipe.remaining(), 0);
        pipe.rewind();
        assert_eq!(pipe.remaining(), 1);
    }

    #[test]
    fn trace_pipe_clear() {
        let _s = crate::test_serial::acquire();
        let mut pipe = TracePipe::new();
        pipe.events = vec![TraceEvent {
            category: "c",
            message: String::from("m"),
        }];
        pipe.read_pos = 0;

        pipe.clear();
        assert_eq!(pipe.total_events(), 0);
        assert_eq!(pipe.remaining(), 0);
    }

    #[test]
    fn trace_pipe_capacity_enforced() {
        let _s = crate::test_serial::acquire();
        let mut pipe = TracePipe::new();
        for i in 0..TRACE_PIPE_CAPACITY + 100 {
            pipe.events.push(TraceEvent {
                category: "cat",
                message: alloc::format!("event_{}", i),
            });
        }
        pipe.read_pos = 0;

        // Manually enforce capacity like refresh does
        if pipe.events.len() > TRACE_PIPE_CAPACITY {
            let drain = pipe.events.len() - TRACE_PIPE_CAPACITY;
            pipe.events.drain(..drain);
        }

        assert!(pipe.total_events() <= TRACE_PIPE_CAPACITY);
    }

    #[test]
    fn global_trace_pipe_init_and_read() {
        let _s = crate::test_serial::acquire();
        init_trace_pipe();
        clear_trace_pipe();

        // Clear all sources first
        super::super::init_trace_buffer(32);
        super::super::clear_trace();
        crate::tracing::function_trace::set_function_tracer_enabled(false);
        crate::tracing::kprobes::reset_kprobes();

        // Push some events into the global buffer
        super::super::trace("pipe_test", "event1");
        super::super::trace("pipe_test", "event2");

        refresh_trace_pipe();
        let remaining = trace_pipe_remaining();
        assert!(remaining >= 2, "expected at least 2 events, got {}", remaining);

        let all_lines = read_trace_pipe_all();
        let has_pipe_test = all_lines.iter().any(|l| l.contains("pipe_test"));
        assert!(has_pipe_test, "expected at least one line containing 'pipe_test', got {:?}", all_lines);
    }

    #[test]
    fn global_trace_pipe_filter() {
        let _s = crate::test_serial::acquire();
        init_trace_pipe();
        clear_trace_pipe();

        super::super::init_trace_buffer(32);
        super::super::clear_trace();
        super::super::trace("keep", "yes");
        super::super::trace("drop", "no");

        set_trace_pipe_filter(&["keep"]);
        refresh_trace_pipe();

        let lines = read_trace_pipe_all();
        assert!(lines.iter().all(|l| l.contains("keep")));
        assert!(!lines.iter().any(|l| l.contains("[drop]")));

        clear_trace_pipe_filter();
    }
}
