use super::linked_list::LinkedListAllocator;
use core::alloc::{GlobalAlloc, Layout};
use core::mem::size_of;

struct ListNode {
    next: Option<&'static mut ListNode>,
}

const BLOCK_SIZES: &[usize] = &[8, 16, 32, 64, 128, 256, 512, 1024, 2048];

pub struct FixedSizeBlockAllocator {
    list_heads: [Option<&'static mut ListNode>; BLOCK_SIZES.len()],
    fallback_allocator: LinkedListAllocator,
}

impl Default for FixedSizeBlockAllocator {
    fn default() -> Self {
        Self::new()
    }
}

impl FixedSizeBlockAllocator {
    pub const fn new() -> Self {
        const EMPTY: Option<&'static mut ListNode> = None;
        FixedSizeBlockAllocator {
            list_heads: [EMPTY; BLOCK_SIZES.len()],
            fallback_allocator: LinkedListAllocator::new(),
        }
    }

    /// # Safety
    ///
    /// `heap_start` must point to a valid, unused memory region of at least `heap_size` bytes.
    pub unsafe fn init(&mut self, heap_start: usize, heap_size: usize) {
        // Safety: caller guarantees heap_start points to a valid, unused memory region of at least heap_size bytes.
        unsafe {
            self.fallback_allocator.init(heap_start, heap_size);
        }
    }

    fn fallback_alloc(&mut self, layout: Layout) -> *mut u8 {
        self.fallback_allocator.allocate(layout)
    }
}

fn list_index(layout: &Layout) -> Option<usize> {
    let required_block_size = layout.size().max(layout.align());
    BLOCK_SIZES.iter().position(|&s| s >= required_block_size)
}

unsafe impl GlobalAlloc for super::Locked<FixedSizeBlockAllocator> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if let Some(ptr) = crate::memory::kfence::kfence_alloc(layout.size()) {
            return ptr;
        }

        // Check cgroup memory limit BEFORE taking the allocator lock.
        // cgroup_try_reserve_memory is lock-free (uses atomics + try_lock).
        let cgroup_pid = crate::task::scheduler::get_current_process_id()
            .map(|p| p.0 as u32)
            .unwrap_or(0);
        let reserved = crate::cgroup::cgroup_try_reserve_memory(cgroup_pid, layout.size() as u64);
        if !reserved {
            return core::ptr::null_mut();
        }

        let mut allocator = self.lock();
        let ptr = match list_index(&layout) {
            Some(index) => match allocator.list_heads[index].take() {
                Some(node) => {
                    allocator.list_heads[index] = node.next.take();
                    node as *mut ListNode as *mut u8
                }
                None => {
                    let block_size = BLOCK_SIZES[index];
                    let block_align = block_size;
                    let layout = Layout::from_size_align(block_size, block_align).unwrap();
                    allocator.fallback_alloc(layout)
                }
            },
            None => allocator.fallback_alloc(layout),
        };
        drop(allocator);
        if !ptr.is_null() {
            let alloc_size = layout.size();
            crate::memory::kasan::alloc_poison(ptr as usize, alloc_size);
        } else {
            // Allocation failed — undo the cgroup reservation
            crate::cgroup::cgroup_release_memory(cgroup_pid, layout.size() as u64);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if crate::memory::kfence::kfence_free(ptr) {
            return;
        }

        let alloc_size = layout.size();
        crate::memory::kasan::free_poison(ptr as usize, alloc_size);

        let mut allocator = self.lock();
        match list_index(&layout) {
            Some(index) => {
                let new_node = ListNode {
                    next: allocator.list_heads[index].take(),
                };
                assert!(size_of::<ListNode>() <= BLOCK_SIZES[index]);
                let new_node_ptr = ptr as *mut ListNode;
                // Safety: ptr was allocated with block_size (>= BLOCK_SIZES[index] >= size_of::<ListNode>()), so it has sufficient space.
                unsafe {
                    new_node_ptr.write(new_node);
                    allocator.list_heads[index] = Some(&mut *new_node_ptr);
                }
            }
            None => {
                allocator.fallback_allocator.deallocate(ptr, layout);
            }
        }
        drop(allocator);

        // Release cgroup memory accounting AFTER dropping the allocator lock
        let cgroup_pid = crate::task::scheduler::get_current_process_id()
            .map(|p| p.0 as u32)
            .unwrap_or(0);
        crate::cgroup::cgroup_release_memory(cgroup_pid, alloc_size as u64);
    }
}
