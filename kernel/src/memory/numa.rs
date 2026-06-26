use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};
use spin::Mutex;

const MAX_NUMA_NODES: usize = 8;
pub const NUMA_NODE_INVALID: i32 = -1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryTier {
    Dram = 0,
    Pmem = 1,
    Cxl = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumaPolicy {
    Local,
    Bind(i32),
    Interleave,
    Preferred(i32),
    Default,
}

impl Default for NumaPolicy {
    fn default() -> Self {
        NumaPolicy::Local
    }
}

#[derive(Debug, Clone)]
pub struct NodeStats {
    pub total_memory: u64,
    pub free_memory: u64,
    pub pages_allocated: u64,
    pub pages_freed: u64,
    pub distances: [u16; MAX_NUMA_NODES],
}

impl NodeStats {
    fn new() -> Self {
        Self {
            total_memory: 0,
            free_memory: 0,
            pages_allocated: 0,
            pages_freed: 0,
            distances: [0; MAX_NUMA_NODES],
        }
    }
}

#[derive(Debug, Clone)]
pub struct NumaNode {
    pub id: i32,
    pub tier: MemoryTier,
    pub stats: NodeStats,
    pub online: bool,
    pub cpu_mask: u64,
}

#[derive(Debug)]
pub struct NumaManager {
    nodes: BTreeMap<i32, NumaNode>,
    online_count: u32,
    policy: NumaPolicy,
    rr_counter: AtomicU32,
    preferred_node: i32,
    node_ranges: BTreeMap<u64, (u64, i32)>,
    total_system_memory: u64,
}

impl Default for NumaManager {
    fn default() -> Self {
        Self::new()
    }
}

impl NumaManager {
    pub fn new() -> Self {
        Self {
            nodes: BTreeMap::new(),
            online_count: 0,
            policy: NumaPolicy::Default,
            rr_counter: AtomicU32::new(0),
            preferred_node: NUMA_NODE_INVALID,
            node_ranges: BTreeMap::new(),
            total_system_memory: 0,
        }
    }

    pub fn init_single_node(&mut self, node_id: i32, memory: u64) {
        let mut stats = NodeStats::new();
        stats.total_memory = memory;
        stats.free_memory = memory;
        for i in 0..MAX_NUMA_NODES {
            stats.distances[i] = if i == node_id as usize { 10 } else { 20 };
        }

        let node = NumaNode {
            id: node_id,
            tier: MemoryTier::Dram,
            stats,
            online: true,
            cpu_mask: 0,
        };

        self.nodes.insert(node_id, node);
        self.online_count = 1;
        self.total_system_memory = memory;
    }

    pub fn add_node(&mut self, node_id: i32, memory: u64) {
        let mut stats = NodeStats::new();
        stats.total_memory = memory;
        stats.free_memory = memory;
        for i in 0..MAX_NUMA_NODES {
            stats.distances[i] = if i == node_id as usize { 10 } else { 20 };
        }

        let node = NumaNode {
            id: node_id,
            tier: MemoryTier::Dram,
            stats,
            online: true,
            cpu_mask: 0,
        };

        self.nodes.insert(node_id, node);
        self.online_count += 1;
        self.total_system_memory += memory;
    }

    pub fn set_node_tier(&mut self, node_id: i32, tier: MemoryTier) {
        if let Some(node) = self.nodes.get_mut(&node_id) {
            node.tier = tier;
        }
    }

    pub fn set_node_cpu_mask(&mut self, node_id: i32, mask: u64) {
        if let Some(node) = self.nodes.get_mut(&node_id) {
            node.cpu_mask = mask;
        }
    }

    pub fn add_node_range(&mut self, start: u64, end: u64, node_id: i32) {
        self.node_ranges.insert(start, (end, node_id));
    }

    pub fn alloc_page(&mut self, policy: NumaPolicy) -> Option<(i32, u64)> {
        match policy {
            NumaPolicy::Local | NumaPolicy::Default => {
                if self.preferred_node != NUMA_NODE_INVALID {
                    if let Some(result) = self.alloc_from_node(self.preferred_node) {
                        return Some(result);
                    }
                }
                self.alloc_from_any()
            }
            NumaPolicy::Bind(node_id) => self.alloc_from_node(node_id),
            NumaPolicy::Interleave => {
                let count = self.online_count;
                if count == 0 {
                    return None;
                }
                let idx = self.rr_counter.fetch_add(1, Ordering::Relaxed) % count;
                let target = self
                    .nodes
                    .values()
                    .filter(|n| n.online)
                    .nth(idx as usize)
                    .map(|n| n.id)?;
                self.alloc_from_node(target)
            }
            NumaPolicy::Preferred(node_id) => {
                if let Some(result) = self.alloc_from_node(node_id) {
                    return Some(result);
                }
                self.alloc_from_any()
            }
        }
    }

    pub fn alloc_from_node(&mut self, node_id: i32) -> Option<(i32, u64)> {
        let node = self.nodes.get_mut(&node_id)?;
        if !node.online || node.stats.free_memory < 4096 {
            return None;
        }
        let base = node.stats.total_memory - node.stats.free_memory;
        node.stats.free_memory -= 4096;
        node.stats.pages_allocated += 1;
        Some((node_id, base))
    }

    pub fn alloc_from_any(&mut self) -> Option<(i32, u64)> {
        let candidates: Vec<i32> = self
            .nodes
            .values()
            .filter(|n| n.online && n.stats.free_memory >= 4096)
            .map(|n| n.id)
            .collect();

        if candidates.is_empty() {
            return None;
        }

        for &node_id in &candidates {
            if let Some(result) = self.alloc_from_node(node_id) {
                return Some(result);
            }
        }
        None
    }

    pub fn free_page(&mut self, node_id: i32, _phys_addr: u64) -> bool {
        if let Some(node) = self.nodes.get_mut(&node_id) {
            node.stats.free_memory += 4096;
            node.stats.pages_freed += 1;
            true
        } else {
            false
        }
    }

    pub fn set_policy(&mut self, policy: NumaPolicy) {
        self.policy = policy;
    }

    pub fn policy(&self) -> NumaPolicy {
        self.policy
    }

    pub fn distance(&self, from: i32, to: i32) -> u16 {
        if from == to {
            return 10;
        }
        if let Some(node) = self.nodes.get(&from) {
            if (to as usize) < MAX_NUMA_NODES {
                return node.stats.distances[to as usize];
            }
        }
        20
    }

    pub fn nearest_available_node(&self, _from: i32) -> Option<i32> {
        self.nodes
            .values()
            .filter(|n| n.online && n.stats.free_memory >= 4096)
            .min_by_key(|n| n.stats.distances[n.id as usize])
            .map(|n| n.id)
    }

    pub fn all_node_stats(&self) -> Vec<(i32, NodeStats)> {
        self.nodes
            .iter()
            .map(|(&id, node)| (id, node.stats.clone()))
            .collect()
    }

    pub fn online_node_count(&self) -> u32 {
        self.online_count
    }

    pub fn total_memory(&self) -> u64 {
        self.total_system_memory
    }

    pub fn preferred_node(&self) -> i32 {
        self.preferred_node
    }

    pub fn phys_addr_to_node(&self, phys_addr: u64) -> i32 {
        for (&start, &(end, node_id)) in &self.node_ranges {
            if phys_addr >= start && phys_addr < end {
                return node_id;
            }
        }
        NUMA_NODE_INVALID
    }
}

static NUMA: Mutex<NumaManager> = Mutex::new(NumaManager {
    nodes: BTreeMap::new(),
    online_count: 0,
    policy: NumaPolicy::Default,
    rr_counter: AtomicU32::new(0),
    preferred_node: NUMA_NODE_INVALID,
    node_ranges: BTreeMap::new(),
    total_system_memory: 0,
});

pub fn numa_init_single_node(node_id: i32, memory: u64) {
    NUMA.lock().init_single_node(node_id, memory);
}

pub fn numa_add_node(node_id: i32, memory: u64) {
    NUMA.lock().add_node(node_id, memory);
}

pub fn numa_alloc_page(policy: NumaPolicy) -> Option<(i32, u64)> {
    NUMA.lock().alloc_page(policy)
}

pub fn numa_free_page(node_id: i32, phys_addr: u64) -> bool {
    NUMA.lock().free_page(node_id, phys_addr)
}

pub fn numa_set_policy(policy: NumaPolicy) {
    NUMA.lock().set_policy(policy);
}

pub fn numa_policy() -> NumaPolicy {
    NUMA.lock().policy()
}

pub fn numa_distance(from: i32, to: i32) -> u16 {
    NUMA.lock().distance(from, to)
}

pub fn numa_online_count() -> u32 {
    NUMA.lock().online_node_count()
}

pub fn numa_total_memory() -> u64 {
    NUMA.lock().total_memory()
}

pub fn numa_phys_addr_to_node(phys_addr: u64) -> i32 {
    NUMA.lock().phys_addr_to_node(phys_addr)
}

pub fn numa_set_node_tier(node_id: i32, tier: MemoryTier) {
    NUMA.lock().set_node_tier(node_id, tier);
}

pub fn numa_set_node_cpu_mask(node_id: i32, mask: u64) {
    NUMA.lock().set_node_cpu_mask(node_id, mask);
}

pub fn numa_add_node_range(start: u64, end: u64, node_id: i32) {
    NUMA.lock().add_node_range(start, end, node_id);
}

pub fn numa_nearest_available_node(from: i32) -> Option<i32> {
    NUMA.lock().nearest_available_node(from)
}

pub fn numa_all_node_stats() -> Vec<(i32, NodeStats)> {
    NUMA.lock().all_node_stats()
}

pub fn numa_preferred_node() -> i32 {
    NUMA.lock().preferred_node()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_serial;

    fn setup_single_node() {
        let mut mgr = NumaManager::new();
        mgr.init_single_node(0, 1024 * 4096);
        *NUMA.lock() = mgr;
    }

    fn setup_two_nodes() {
        let mut mgr = NumaManager::new();
        mgr.init_single_node(0, 512 * 4096);
        mgr.add_node(1, 512 * 4096);
        *NUMA.lock() = mgr;
    }

    #[test]
    fn test_single_node_init() {
        let _s = test_serial::acquire();
        setup_single_node();
        let mgr = NUMA.lock();
        assert_eq!(mgr.online_node_count(), 1);
        assert_eq!(mgr.total_memory(), 1024 * 4096);
        assert!(mgr.nodes.contains_key(&0));
    }

    #[test]
    fn test_add_node() {
        let _s = test_serial::acquire();
        setup_two_nodes();
        let mgr = NUMA.lock();
        assert_eq!(mgr.online_node_count(), 2);
        assert_eq!(mgr.total_memory(), 1024 * 4096);
    }

    #[test]
    fn test_local_alloc() {
        let _s = test_serial::acquire();
        setup_single_node();
        let result = NUMA.lock().alloc_page(NumaPolicy::Local);
        assert!(result.is_some());
        let (node_id, _addr) = result.unwrap();
        assert_eq!(node_id, 0);
    }

    #[test]
    fn test_bind_alloc() {
        let _s = test_serial::acquire();
        setup_two_nodes();
        let result = NUMA.lock().alloc_page(NumaPolicy::Bind(1));
        assert!(result.is_some());
        let (node_id, _addr) = result.unwrap();
        assert_eq!(node_id, 1);
    }

    #[test]
    fn test_bind_exhausted() {
        let _s = test_serial::acquire();
        let mut mgr = NumaManager::new();
        mgr.init_single_node(0, 4096);
        mgr.add_node(1, 4096);
        mgr.alloc_page(NumaPolicy::Bind(1));
        let result = mgr.alloc_page(NumaPolicy::Bind(1));
        assert!(result.is_none());
    }

    #[test]
    fn test_interleave() {
        let _s = test_serial::acquire();
        setup_two_nodes();
        let mut mgr = NUMA.lock();
        let r1 = mgr.alloc_page(NumaPolicy::Interleave);
        let r2 = mgr.alloc_page(NumaPolicy::Interleave);
        assert!(r1.is_some() && r2.is_some());
        let n1 = r1.unwrap().0;
        let n2 = r2.unwrap().0;
        assert_ne!(n1, n2);
    }

    #[test]
    fn test_free_page() {
        let _s = test_serial::acquire();
        setup_single_node();
        let mut mgr = NUMA.lock();
        let free_before = mgr.nodes.get(&0).unwrap().stats.free_memory;
        let (node_id, addr) = mgr.alloc_page(NumaPolicy::Local).unwrap();
        let free_after_alloc = mgr.nodes.get(&0).unwrap().stats.free_memory;
        assert!(free_after_alloc < free_before);
        assert!(mgr.free_page(node_id, addr));
        let free_after_free = mgr.nodes.get(&0).unwrap().stats.free_memory;
        assert_eq!(free_after_free, free_before);
    }

    #[test]
    fn test_distance_local() {
        let _s = test_serial::acquire();
        setup_two_nodes();
        let d = NUMA.lock().distance(0, 0);
        assert_eq!(d, 10);
    }

    #[test]
    fn test_distance_remote() {
        let _s = test_serial::acquire();
        setup_two_nodes();
        let d = NUMA.lock().distance(0, 1);
        assert_eq!(d, 20);
    }

    #[test]
    fn test_nearest_available_node() {
        let _s = test_serial::acquire();
        setup_two_nodes();
        let nearest = NUMA.lock().nearest_available_node(0);
        assert!(nearest.is_some());
    }

    #[test]
    fn test_policy_set_get() {
        let _s = test_serial::acquire();
        setup_single_node();
        let mut mgr = NUMA.lock();
        mgr.set_policy(NumaPolicy::Interleave);
        assert_eq!(mgr.policy(), NumaPolicy::Interleave);
    }

    #[test]
    fn test_set_node_tier() {
        let _s = test_serial::acquire();
        setup_single_node();
        NUMA.lock().set_node_tier(0, MemoryTier::Pmem);
        let tier = NUMA.lock().nodes.get(&0).unwrap().tier;
        assert_eq!(tier, MemoryTier::Pmem);
    }

    #[test]
    fn test_set_node_cpu_mask() {
        let _s = test_serial::acquire();
        setup_single_node();
        NUMA.lock().set_node_cpu_mask(0, 0xFF);
        let mask = NUMA.lock().nodes.get(&0).unwrap().cpu_mask;
        assert_eq!(mask, 0xFF);
    }

    #[test]
    fn test_node_range() {
        let _s = test_serial::acquire();
        setup_single_node();
        NUMA.lock().add_node_range(0x1000, 0x2000, 0);
        let node = NUMA.lock().phys_addr_to_node(0x1500);
        assert_eq!(node, 0);
        let node = NUMA.lock().phys_addr_to_node(0x3000);
        assert_eq!(node, NUMA_NODE_INVALID);
    }

    #[test]
    fn test_all_node_stats() {
        let _s = test_serial::acquire();
        setup_two_nodes();
        let stats = NUMA.lock().all_node_stats();
        assert_eq!(stats.len(), 2);
    }

    #[test]
    fn test_preferred_fallback() {
        let _s = test_serial::acquire();
        let mut mgr = NumaManager::new();
        mgr.init_single_node(0, 4096);
        mgr.preferred_node = 99;
        let result = mgr.alloc_page(NumaPolicy::Local);
        assert!(result.is_some());
        let (node_id, _) = result.unwrap();
        assert_eq!(node_id, 0);
    }

    #[test]
    fn test_numa_global_free_page() {
        let _s = test_serial::acquire();
        setup_single_node();
        let (node_id, addr) = numa_alloc_page(NumaPolicy::Local).unwrap();
        assert!(numa_free_page(node_id, addr));
    }

    #[test]
    fn test_numa_global_api() {
        let _s = test_serial::acquire();
        setup_two_nodes();
        assert_eq!(numa_online_count(), 2);
        assert_eq!(numa_total_memory(), 1024 * 4096);
        assert_eq!(numa_distance(0, 0), 10);
        assert_eq!(numa_distance(0, 1), 20);
    }
}
