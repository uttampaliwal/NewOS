//! Synchronization primitives for the kernel.
//!
//! This module provides scalable concurrency primitives beyond the basic
//! `spin::Mutex`:
//!
//! - [`seqlock`] — optimistic reader / exclusive writer lock
//! - [`rwlock`] — multiple-reader / single-writer lock
//! - [`rcu`] — read-copy-update for lock-free read-side access
//! - [`workqueue`] — deferred work execution
//! - [`percpu`] — per-CPU data framework
//! - [`completion`] — one-shot event signalling
//! - [`lockdep`] — lock dependency tracker / deadlock detector

pub mod completion;
pub mod lockdep;
pub mod percpu;
pub mod rcu;
pub mod rwlock;
pub mod seqlock;
pub mod workqueue;
