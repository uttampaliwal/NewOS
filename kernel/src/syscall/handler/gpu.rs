use super::SyscallResult;
use crate::drivers::gpu;
use turnix_abi::input::InputEvent;
use turnix_abi::syscall::SyscallArgs;

pub fn handle_input_read(args: SyscallArgs) -> SyscallResult {
    // arg0: destination buffer pointer
    // arg1: number of InputEvent slots (sizeof(InputEvent) = 8 bytes each)
    let buf_ptr = args.arg0 as *mut InputEvent;
    let slots = args.arg1 as usize;

    if buf_ptr.is_null() || slots == 0 {
        return SyscallResult::Success(0);
    }

    let mut count: u64 = 0;
    for i in 0..slots {
        if let Some(ev) = crate::input::read_event() {
            // Safety: buf_ptr is non-null (checked above) and i < slots, so the write is within the caller-provided buffer.
            unsafe { buf_ptr.add(i).write_unaligned(ev) };
            count += 1;
        } else {
            break;
        }
    }

    SyscallResult::Success(count)
}

pub fn handle_gbm_create(args: SyscallArgs) -> SyscallResult {
    let caller_pid = gpu::current_pid();
    {
        let mgr = gpu::DRM_MANAGER.lock();
        if !mgr.is_compositor(caller_pid) {
            return SyscallResult::Error(1);
        }
    }

    let width = args.arg0 as u32;
    let height = args.arg1 as u32;
    let format = args.arg2 as u32;

    if width == 0 || height == 0 {
        return SyscallResult::Error(22);
    }

    match gpu::gbm::gbm_create(width, height, format) {
        Some(id) => SyscallResult::Success(id),
        None => SyscallResult::Error(12),
    }
}

pub fn handle_gbm_map(args: SyscallArgs) -> SyscallResult {
    let caller_pid = gpu::current_pid();
    {
        let mgr = gpu::DRM_MANAGER.lock();
        if !mgr.is_compositor(caller_pid) {
            return SyscallResult::Error(1);
        }
    }

    let id = args.arg0;
    match gpu::gbm::gbm_map(id) {
        Some(addr) => SyscallResult::Success(addr),
        None => SyscallResult::Error(2),
    }
}

pub fn handle_gbm_destroy(args: SyscallArgs) -> SyscallResult {
    let caller_pid = gpu::current_pid();
    {
        let mgr = gpu::DRM_MANAGER.lock();
        if !mgr.is_compositor(caller_pid) {
            return SyscallResult::Error(1);
        }
    }

    let id = args.arg0;
    gpu::gbm::gbm_destroy(id);
    SyscallResult::Success(0)
}

pub fn handle_drm_page_flip(args: SyscallArgs) -> SyscallResult {
    let caller_pid = gpu::current_pid();
    let crtc_id = args.arg1 as u32;
    let gbm_id = args.arg0;

    // Gather framebuffer info under DRM_MANAGER lock, then drop it before
    // touching GBM to maintain consistent lock ordering (DRM → GBM).
    let fb_addr;
    let fb_size;
    {
        let mgr = gpu::DRM_MANAGER.lock();
        if !mgr.is_compositor(caller_pid) {
            return SyscallResult::Error(1);
        }
        fb_addr = mgr.framebuffer_addr();
        fb_size = mgr.framebuffer_size();
    }
    if fb_addr == 0 || fb_size == 0 {
        return SyscallResult::Error(2);
    }

    let gbm_phys = match gpu::gbm::gbm_map(gbm_id) {
        Some(addr) => addr,
        None => return SyscallResult::Error(2),
    };
    let buf_size = gpu::gbm::gbm_buffer_size(gbm_id).unwrap_or(fb_size);

    let phys_mem_offset = crate::boot::get_phys_mem_offset();
    let src = (phys_mem_offset + gbm_phys).as_ptr::<u8>();
    let dst = (phys_mem_offset + fb_addr).as_mut_ptr::<u8>();
    let copy_size = core::cmp::min(fb_size, buf_size);

    // Safety: src and dst point to valid physical memory regions (GBM buffer and framebuffer) with sufficient size.
    unsafe {
        core::ptr::copy_nonoverlapping(src, dst, copy_size as usize);
    }

    let mut mgr = gpu::DRM_MANAGER.lock();
    match mgr.page_flip(crtc_id, 0) {
        Ok(()) => SyscallResult::Success(0),
        Err(_) => SyscallResult::Error(5),
    }
}
