use super::linked_list::LinkedListAllocator;
use core::alloc::{GlobalAlloc, Layout};
use core::ptr;

struct ListNode {
    next: Option<&'static mut ListNode>,
}

const BLOCK_SIZES: &[usize] = &[8, 16, 32, 64, 128, 256, 512, 1024, 2048];

pub struct FixedSizeBlockAllocator {
    list_heads: [Option<&'static mut ListNode>; BLOCK_SIZES.len()],
    fallback_allocator: LinkedListAllocator,
}

impl FixedSizeBlockAllocator {
    pub const fn new() -> Self {
        const EMPTY: Option<&'static mut ListNode> = None;
        FixedSizeBlockAllocator {
            list_heads: [EMPTY; BLOCK_SIZES.len()],
            fallback_allocator: LinkedListAllocator::new(),
        }
    }

    pub unsafe fn init(&mut self, heap_start: usize, heap_size: usize) {
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
        let mut allocator = self.lock();
        match list_index(&layout) {
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
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let mut allocator = self.lock();
        match list_index(&layout) {
            Some(index) => {
                let new_node = ListNode {
                    next: allocator.list_heads[index].take(),
                };
                assert!(core::mem::size_of::<ListNode>() <= BLOCK_SIZES[index]);
                let new_node_ptr = ptr as *mut ListNode;
                unsafe {
                    new_node_ptr.write(new_node);
                    allocator.list_heads[index] = Some(&mut *new_node_ptr);
                }
            }
            None => {
                allocator.fallback_allocator.deallocate(ptr, layout);
            }
        }
    }
}
