use core::alloc::{GlobalAlloc, Layout};

const HEAP_SIZE: usize = 1024 * 1024;

#[repr(C, align(16))]
struct Heap([u8; HEAP_SIZE]);

static HEAP: Heap = Heap([0; HEAP_SIZE]);
static mut OFFSET: usize = 0;

pub struct BumpAllocator;

unsafe impl GlobalAlloc for BumpAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let align = layout.align();
        let size = layout.size();
        let current = unsafe { OFFSET };
        let aligned = (current + align - 1) & !(align - 1);
        let new = aligned + size;
        if new > HEAP_SIZE {
            return core::ptr::null_mut();
        }
        unsafe {
            OFFSET = new;
        }
        unsafe { HEAP.0.as_ptr().add(aligned) as *mut u8 }
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}
