//! Tasklet support built on top of the softirq framework.
//!
//! Tasklets are deferred callbacks that run from softirq context.  This
//! implementation keeps the model intentionally small: a tasklet is a single
//! function pointer that can be scheduled and later drained by the tasklet
//! softirq handler.

use core::sync::atomic::{AtomicBool, Ordering};

use spin::Mutex;

use crate::softirq::{Softirq, softirq_process, softirq_register};

/// A deferred callback executed from tasklet softirq context.
pub struct Tasklet {
    handler: fn(),
    scheduled: AtomicBool,
}

// SAFETY: the tasklet stores a plain function pointer and an atomic flag.
unsafe impl Send for Tasklet {}
unsafe impl Sync for Tasklet {}

impl Tasklet {
    /// Create a new tasklet for the provided callback.
    pub const fn new(handler: fn()) -> Self {
        Self {
            handler,
            scheduled: AtomicBool::new(false),
        }
    }

    /// Queue this tasklet for execution.
    pub fn schedule(&self) {
        self.scheduled.store(true, Ordering::Release);
        Softirq::Tasklet.raise();
    }

    fn run(&self) {
        if self.scheduled.swap(false, Ordering::AcqRel) {
            (self.handler)();
        }
    }
}

static TASKLETS: Mutex<[Option<&'static Tasklet>; 16]> = Mutex::new([None; 16]);
static INIT: AtomicBool = AtomicBool::new(false);

fn dispatch_tasklets() {
    let tasklets = TASKLETS.lock();
    for tasklet in tasklets.iter().flatten() {
        tasklet.run();
    }
}

/// Register a tasklet with the global dispatcher.
///
/// The dispatcher is installed lazily the first time a tasklet is registered.
pub fn register_tasklet(tasklet: &'static Tasklet) -> bool {
    if !INIT.swap(true, Ordering::AcqRel) {
        softirq_register(Softirq::Tasklet, dispatch_tasklets);
    }

    let mut tasklets = TASKLETS.lock();
    for slot in tasklets.iter_mut() {
        if slot.is_none() {
            *slot = Some(tasklet);
            return true;
        }
    }
    false
}

/// Run pending softirqs, including tasklets.
pub fn drain() {
    softirq_process();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_serial;
    use core::sync::atomic::AtomicU32;

    static COUNTER: AtomicU32 = AtomicU32::new(0);
    static TASKLET: Tasklet = Tasklet::new(|| {
        COUNTER.fetch_add(1, Ordering::Relaxed);
    });

    #[test]
    fn tasklet_runs_when_scheduled() {
        let _s = test_serial::acquire();
        COUNTER.store(0, Ordering::Relaxed);
        INIT.store(false, Ordering::Relaxed);
        TASKLETS.lock().fill(None);

        assert!(register_tasklet(&TASKLET));
        TASKLET.schedule();
        drain();

        assert_eq!(COUNTER.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn tasklet_does_not_run_without_schedule() {
        let _s = test_serial::acquire();
        COUNTER.store(0, Ordering::Relaxed);
        INIT.store(false, Ordering::Relaxed);
        TASKLETS.lock().fill(None);

        assert!(register_tasklet(&TASKLET));
        drain();

        assert_eq!(COUNTER.load(Ordering::Relaxed), 0);
    }
}
