use alloc::collections::BTreeMap;
use spin::Mutex;

pub const FUTEX_WAIT: u32 = 0;
pub const FUTEX_WAKE: u32 = 1;

struct FutexWaitQueue {
    waiters: alloc::vec::Vec<crate::task::TaskId>,
}

static FUTEX_TABLE: Mutex<BTreeMap<u64, FutexWaitQueue>> = Mutex::new(BTreeMap::new());

pub fn futex_wait(uaddr: u64, expected_val: u32) -> Result<(), i32> {
    let current_val = unsafe { core::ptr::read_volatile(uaddr as *const u32) };
    if current_val != expected_val {
        return Err(11); // EAGAIN
    }

    if let Some(tid) = crate::task::scheduler::get_current_task_id() {
        let mut table = FUTEX_TABLE.lock();
        let q = table.entry(uaddr).or_insert_with(|| FutexWaitQueue {
            waiters: alloc::vec::Vec::new(),
        });
        q.waiters.push(tid);
        drop(table);

        crate::task::scheduler::block_current();
        crate::task::scheduler::yield_task();
        Ok(())
    } else {
        Err(1) // EPERM
    }
}

pub fn futex_wake(uaddr: u64, count: usize) -> Result<usize, i32> {
    let mut table = FUTEX_TABLE.lock();
    let mut woken = 0;
    if let Some(q) = table.get_mut(&uaddr) {
        while woken < count && !q.waiters.is_empty() {
            let tid = q.waiters.remove(0);
            crate::task::scheduler::wake_task_by_id(tid);
            woken += 1;
        }
        if q.waiters.is_empty() {
            table.remove(&uaddr);
        }
    }
    Ok(woken)
}
