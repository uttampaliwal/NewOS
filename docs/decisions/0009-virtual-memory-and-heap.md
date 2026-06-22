# ADR 0003: Virtual Memory and Heap Architecture

## Status

Accepted

## Decision

1.  **Paging Abstraction:** We will use the `x86_64` crate from the Rust OS ecosystem to manage x86_64-specific architectural details, such as page table structures (PML4, PDPT, etc.), control registers (like `Cr3`), and address abstractions.
2.  **Heap Allocator:** We will build our own custom Heap Allocator framework, starting with a simple Bump Allocator protected by spinlocks, but designed to be scalable and sustainable for overall OS development. We will avoid bringing in off-the-shelf allocator crates initially, ensuring we deeply understand our memory footprint and allocation policies.

## Why

### `x86_64` Crate
- While building page tables from scratch teaches the bitwise layout, it is highly error-prone and tedious. The `x86_64` crate provides safe abstractions over these hardware structures without hiding the underlying concepts.
- It is a standard, battle-tested crate in the Rust OS dev ecosystem.
- Using it allows us to focus on the *logic* of memory mapping and layout rather than debugging bitwise operations in PTEs (Page Table Entries).

### Custom Heap Allocator
- Implementing a custom allocator aligns perfectly with the "Learn deeply while building something real" principle.
- By writing our own allocator interface, we can expand and improve it from time to time (e.g., migrating from a bump allocator to a slab allocator or linked-list allocator) as the system's needs grow.
- It gives us absolute control over the kernel's dynamic memory behavior, which is critical for long-term scalability and sustainability.

## Consequences

- We add the `x86_64` and `spin` crates to the kernel's dependencies.
- We must implement the `GlobalAlloc` trait manually for our custom allocator framework.
- Our custom allocator will initially be limited in functionality (e.g., a bump allocator cannot free individual items), but it will unblock the use of the `alloc` crate for early dynamic data structures.
