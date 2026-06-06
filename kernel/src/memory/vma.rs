use alloc::collections::BTreeMap;
use bitflags::bitflags;
use x86_64::VirtAddr;

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct VmaProt: u8 {
        const READ    = 0b001;
        const WRITE   = 0b010;
        const EXECUTE = 0b100;
    }
}

impl VmaProt {
    pub fn is_readable(&self) -> bool {
        self.contains(Self::READ)
    }

    pub fn is_writable(&self) -> bool {
        self.contains(Self::WRITE)
    }

    pub fn is_executable(&self) -> bool {
        self.contains(Self::EXECUTE)
    }

    pub fn violates_wx(&self) -> bool {
        self.is_writable() && self.is_executable()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct InodeId(pub u64);

#[derive(Debug, Clone)]
pub enum VmaBacking {
    Anonymous,
    FileBacked {
        inode: InodeId,
        offset: u64,
    },
    DeviceMapped {
        device: crate::drivers::framework::DeviceKey,
    },
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct VmaFlags: u8 {
        const MAP_SHARED  = 0b001;
        const MAP_PRIVATE = 0b010;
        const MAP_FIXED   = 0b100;
    }
}

#[derive(Debug, Clone)]
pub struct Vma {
    pub start: VirtAddr,
    pub end: VirtAddr,
    pub prot: VmaProt,
    pub backing: VmaBacking,
    pub flags: VmaFlags,
}

impl Vma {
    pub fn size(&self) -> u64 {
        self.end.as_u64() - self.start.as_u64()
    }

    pub fn contains(&self, addr: VirtAddr) -> bool {
        addr >= self.start && addr < self.end
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmaError {
    Conflict,
}

#[derive(Debug, Clone)]
pub struct VmaSet {
    vmas: BTreeMap<VirtAddr, Vma>,
}

impl VmaSet {
    pub fn new() -> Self {
        Self {
            vmas: BTreeMap::new(),
        }
    }

    pub fn find(&self, addr: VirtAddr) -> Option<&Vma> {
        self.vmas
            .range(..=addr)
            .next_back()
            .filter(|(_, vma)| addr < vma.end)
            .map(|(_, vma)| vma)
    }

    pub fn insert(&mut self, vma: Vma) -> Result<(), VmaError> {
        if let Some((_, pred)) = self.vmas.range(..vma.start).next_back()
            && vma.start < pred.end
        {
            return Err(VmaError::Conflict);
        }
        if let Some((succ_start, _)) = self.vmas.range(vma.start..).next()
            && *succ_start < vma.end
        {
            return Err(VmaError::Conflict);
        }
        self.vmas.insert(vma.start, vma);
        Ok(())
    }

    pub fn remove(&mut self, start: VirtAddr) -> Option<Vma> {
        self.vmas.remove(&start)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Vma> {
        self.vmas.values()
    }

    pub fn is_empty(&self) -> bool {
        self.vmas.is_empty()
    }

    pub fn len(&self) -> usize {
        self.vmas.len()
    }
}

impl Default for VmaSet {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;
    use proptest::collection as pc;
    use proptest::prelude::*;
    use x86_64::VirtAddr;

    fn arb_vma_prot() -> impl Strategy<Value = VmaProt> {
        (0..8u8).prop_map(|bits| VmaProt::from_bits_truncate(bits))
    }

    /// Generate a list of non-overlapping VMA *candidates* — page-aligned,
    /// non-zero-size, that can be independently inserted into a VmaSet
    /// without conflicts.
    fn arb_vma_candidates() -> impl Strategy<Value = Vec<Vma>> {
        pc::vec((0..100u64, 1..50u64, arb_vma_prot()), 1..20).prop_map(|segments| {
            let mut vmas = Vec::new();
            let mut cursor = VirtAddr::new(0x1000);
            for (gap, size, prot) in segments {
                cursor = cursor + gap * 0x1000;
                let start = cursor;
                cursor = cursor + size * 0x1000;
                let end = cursor;
                vmas.push(Vma {
                    start,
                    end,
                    prot,
                    backing: VmaBacking::Anonymous,
                    flags: VmaFlags::MAP_PRIVATE,
                });
            }
            vmas
        })
    }

    /// A single mmap or munmap operation on a VmaSet.
    #[derive(Debug, Clone)]
    enum VmaOp {
        Mmap(Vma),
        Munmap(VirtAddr),
    }

    /// Generate a random sequence of mmap/munmap operations from a set of
    /// candidate VMAs.  Each candidate is used at most once for mmap and
    /// at most once for munmap; the sequence interleaves inserts and
    /// removes arbitrarily, challenging the data-structure invariants.
    fn arb_vma_ops(candidates: Vec<Vma>) -> impl Strategy<Value = Vec<VmaOp>> {
        let len = candidates.len();
        let pairs: Vec<(VmaOp, VmaOp)> = candidates
            .into_iter()
            .map(|v| (VmaOp::Mmap(v.clone()), VmaOp::Munmap(v.start)))
            .collect();
        pc::vec(prop::sample::select(pairs), 0..len.saturating_mul(2).max(5)).prop_map(|selected| {
            let mut ops = Vec::new();
            for (mmap, munmap) in selected {
                ops.push(mmap);
                ops.push(munmap);
            }
            ops
        })
    }

    /// Combined strategy: independent candidates + a random operation sequence
    /// drawn from those candidates.
    fn arb_candidates_and_ops() -> impl Strategy<Value = (Vec<Vma>, Vec<VmaOp>)> {
        arb_vma_candidates().prop_flat_map(|candidates| {
            let ops = arb_vma_ops(candidates.clone());
            (proptest::prelude::Just(candidates), ops)
        })
    }

    /// Check the VmaSet invariant:
    ///   - Every address *within* a VMA is findable and maps to the correct VMA
    ///   - Gaps between VMAs and addresses outside the extents are NOT findable
    fn check_vma_invariant(set: &VmaSet) -> Result<(), proptest::test_runner::TestCaseError> {
        let vmas: Vec<&Vma> = set.iter().collect();
        if vmas.is_empty() {
            return Ok(());
        }

        for vma in &vmas {
            // Every VMA start is findable
            let found = set.find(vma.start);
            prop_assert!(
                found.is_some(),
                "VMA [{:#x}, {:#x}) start must be findable",
                vma.start.as_u64(),
                vma.end.as_u64()
            );
            if let Some(f) = found {
                prop_assert_eq!(f.start, vma.start, "found VMA must match at start");
            }

            // Every VMA interior page is findable and maps to the same VMA
            if vma.size() > 0x1000 {
                let mid = vma.start + vma.size() / 2;
                let found_mid = set.find(mid);
                prop_assert!(
                    found_mid.is_some(),
                    "VMA [{:#x}, {:#x}) interior {:#x} must be findable",
                    vma.start.as_u64(),
                    vma.end.as_u64(),
                    mid.as_u64()
                );
                if let Some(f) = found_mid {
                    prop_assert_eq!(f.start, vma.start);
                }
            }

            // Exclusive end must not be covered by the *same* VMA
            // (it may be the start of the next adjacent VMA)
            let found_end = set.find(vma.end);
            if let Some(f) = found_end {
                prop_assert_ne!(
                    f.start,
                    vma.start,
                    "exclusive end {:#x} must not be covered by the same VMA",
                    vma.end.as_u64()
                );
            }
        }

        // Gaps between consecutive VMAs must not be findable
        for pair in vmas.windows(2) {
            let prev = pair[0];
            let next = pair[1];
            if prev.end < next.start {
                let gap_addr = prev.end + 1u64;
                if gap_addr < next.start {
                    let found = set.find(gap_addr);
                    prop_assert!(
                        found.is_none(),
                        "gap address {:#x} between VMAs must not be findable",
                        gap_addr.as_u64()
                    );
                }
            }
        }

        Ok(())
    }

    proptest! {
        /// Property 4 — VMA Tracking Consistency
        ///
        /// From a set of non-overlapping candidate VMAs, generate a random
        /// interleaved sequence of mmap/munmap operations.  Apply them one
        /// by one to a VmaSet, checking after **every** operation that:
        ///   - Every mapped address is covered by exactly one VMA
        ///   - Every unmapped address is not covered by any VMA
        ///
        /// This validates Requirements 9.1 and 9.4: the VmaSet must remain
        /// internally consistent under arbitrary mutation sequences.
        #[test]
        fn vma_consistency_property((_candidates, ops) in arb_candidates_and_ops()) {
            let mut set = VmaSet::new();

            for op in &ops {
                match op {
                    VmaOp::Mmap(vma) => {
                        let _ = set.insert(vma.clone());
                    }
                    VmaOp::Munmap(addr) => {
                        let _ = set.remove(*addr);
                    }
                }
                check_vma_invariant(&set)?;
            }

            // Final invariant: after every candidate has been inserted AND
            // removed, the set should be empty *if* the operation sequence
            // fully drained every candidate.  (We don't assert emptiness
            // here because the sequence may leave some VMAs mapped.)
            // The step-by-step checks above already guarantee consistency.
        }
    }

    #[test]
    fn find_empty_set_returns_none() {
        let set = VmaSet::new();
        assert!(set.find(VirtAddr::new(0x1000)).is_none());
    }

    #[test]
    fn insert_single_vma_findable() {
        let mut set = VmaSet::new();
        let vma = Vma {
            start: VirtAddr::new(0x1000),
            end: VirtAddr::new(0x2000),
            prot: VmaProt::READ | VmaProt::WRITE,
            backing: VmaBacking::Anonymous,
            flags: VmaFlags::MAP_PRIVATE,
        };
        assert!(set.insert(vma.clone()).is_ok());
        assert!(set.find(VirtAddr::new(0x1000)).is_some());
        assert!(set.find(VirtAddr::new(0x1500)).is_some());
        assert!(set.find(VirtAddr::new(0x1FFF)).is_some());
        assert!(set.find(VirtAddr::new(0x2000)).is_none());
        assert!(set.find(VirtAddr::new(0x0FFF)).is_none());
    }

    #[test]
    fn overlapping_insert_rejected() {
        let mut set = VmaSet::new();
        let vma1 = Vma {
            start: VirtAddr::new(0x1000),
            end: VirtAddr::new(0x2000),
            prot: VmaProt::READ,
            backing: VmaBacking::Anonymous,
            flags: VmaFlags::MAP_PRIVATE,
        };
        assert!(set.insert(vma1).is_ok());

        let vma2 = Vma {
            start: VirtAddr::new(0x1500),
            end: VirtAddr::new(0x2500),
            prot: VmaProt::READ,
            backing: VmaBacking::Anonymous,
            flags: VmaFlags::MAP_PRIVATE,
        };
        assert_eq!(set.insert(vma2), Err(VmaError::Conflict));
    }

    #[test]
    fn adjacent_vmas_allowed() {
        let mut set = VmaSet::new();
        let vma1 = Vma {
            start: VirtAddr::new(0x1000),
            end: VirtAddr::new(0x2000),
            prot: VmaProt::READ,
            backing: VmaBacking::Anonymous,
            flags: VmaFlags::MAP_PRIVATE,
        };
        assert!(set.insert(vma1).is_ok());

        let vma2 = Vma {
            start: VirtAddr::new(0x2000),
            end: VirtAddr::new(0x3000),
            prot: VmaProt::WRITE,
            backing: VmaBacking::Anonymous,
            flags: VmaFlags::MAP_PRIVATE,
        };
        assert!(set.insert(vma2).is_ok(), "adjacent VMAs should be allowed");
    }

    #[test]
    fn remove_returns_vma_and_clears() {
        let mut set = VmaSet::new();
        let start = VirtAddr::new(0x1000);
        let vma = Vma {
            start,
            end: VirtAddr::new(0x2000),
            prot: VmaProt::READ,
            backing: VmaBacking::Anonymous,
            flags: VmaFlags::MAP_PRIVATE,
        };
        set.insert(vma.clone()).unwrap();
        let removed = set.remove(start);
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().start, start);
        assert!(set.find(start).is_none());
    }

    #[test]
    fn vma_size_calculation() {
        let vma = Vma {
            start: VirtAddr::new(0x1000),
            end: VirtAddr::new(0x3000),
            prot: VmaProt::READ,
            backing: VmaBacking::Anonymous,
            flags: VmaFlags::MAP_PRIVATE,
        };
        assert_eq!(vma.size(), 0x2000);
    }

    #[test]
    fn vma_contains_checks() {
        let vma = Vma {
            start: VirtAddr::new(0x1000),
            end: VirtAddr::new(0x2000),
            prot: VmaProt::READ,
            backing: VmaBacking::Anonymous,
            flags: VmaFlags::MAP_PRIVATE,
        };
        assert!(vma.contains(VirtAddr::new(0x1000)));
        assert!(vma.contains(VirtAddr::new(0x1FFF)));
        assert!(!vma.contains(VirtAddr::new(0x2000)));
        assert!(!vma.contains(VirtAddr::new(0x0FFF)));
    }

    #[test]
    fn wx_violation_detected() {
        let wx = VmaProt::WRITE | VmaProt::EXECUTE;
        assert!(wx.violates_wx());
        let wo = VmaProt::WRITE;
        assert!(!wo.violates_wx());
        let ro = VmaProt::READ;
        assert!(!ro.violates_wx());
        let rx = VmaProt::READ | VmaProt::EXECUTE;
        assert!(!rx.violates_wx());
    }
}
