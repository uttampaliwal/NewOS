# Phase 3: Physical Memory Bring-Up

## Goal

Use the handed-off boot memory map to build the first kernel-owned physical page-frame allocator.

## Why this matters

Once the kernel can reason about usable physical memory, we can stop treating memory as a passive boot artifact and start building real kernel infrastructure on top of it.

## What this phase adds

- iteration over the boot memory map from shared boot types
- a `MemorySummary` so the kernel can report what memory it received
- a simple bump-style `FrameAllocator` over conventional physical memory
- an early policy to skip low memory below `1 MiB`
- serial verification that the kernel can hand out real page-frame addresses

## Current verified behavior

The freestanding kernel now logs:

- descriptor count
- number of conventional regions
- total conventional pages
- largest conventional region
- a sample of allocated physical frames

## Next step

Use the frame allocator to back:

- kernel page tables
- a higher-half or explicit virtual memory plan
- a heap bootstrap allocator
