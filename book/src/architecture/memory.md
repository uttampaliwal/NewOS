# Memory Management

Turnix implements a full virtual memory subsystem under `kernel/src/memory/`.

## Virtual Address Space

The kernel uses a Higher-Half Direct Mapping (HHDM) layout. Each process gets its own
page table hierarchy with separate user and kernel address ranges. VMA structures
(`kernel/src/memory/vma.rs`) track per-region permissions (`VmaProt`: read, write,
execute) and flags.

## Demand Paging & Page Cache

Pages are allocated lazily on first access via demand paging (`kernel/src/memory/demand.rs`).
An LRU page cache (`kernel/src/memory/page_cache.rs`) manages file-backed pages and
supports eviction under memory pressure.

## Security Enforcements

- **W^X**: The `VmaProt::violates_wx()` check rejects any mapping that is simultaneously
  writable and executable. This is enforced at `mmap` time and during page faults.
- **ASLR / KASLR**: User-space load and stack base addresses are randomized. The kernel
  itself is relocated at boot via KASLR (`kernel/src/memory/aslr.rs`).

## Swap & OOM

A swap manager (`kernel/src/memory/swap.rs`) tracks 4 KiB swap slots and handles page-out
to a block device. When physical memory is exhausted, the OOM killer
(`kernel/src/memory/oom.rs`) selects a victim process based on a priority scoring algorithm
and terminates it to reclaim frames.

## Heap Allocation

The kernel heap uses a linked-list allocator (`linked_list_allocator` crate) initialized
at boot. User-space processes obtain memory through `brk`, `mmap`, and `munmap` syscalls.

## Slab Allocator

The slab allocator (`kernel/src/memory/slab.rs`) provides efficient object caching for
frequently allocated fixed-size kernel objects. It uses `SlabCache` instances that manage
pages of objects. Currently used for pipe ring buffers (64 KiB objects). The allocator
supports large objects (>= page size) by allocating multiple contiguous pages per slab.
`slab_alloc` and `slab_dealloc` provide the allocation interface, with `alloc_size` tracked
per slab for correct deallocation.
