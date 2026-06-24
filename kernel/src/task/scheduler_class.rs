#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
#[allow(non_camel_case_types)]
pub enum SchedulingPolicy {
    SCHED_NORMAL = 0,
    SCHED_FIFO = 1,
    SCHED_RR = 2,
    SCHED_BATCH = 3,
    SCHED_IDLE = 5,
}

impl SchedulingPolicy {
    pub fn from_u8(val: u8) -> Option<Self> {
        match val {
            0 => Some(Self::SCHED_NORMAL),
            1 => Some(Self::SCHED_FIFO),
            2 => Some(Self::SCHED_RR),
            3 => Some(Self::SCHED_BATCH),
            5 => Some(Self::SCHED_IDLE),
            _ => None,
        }
    }
}

pub const DEFAULT_TIMESLICE: u32 = 10;
pub const RT_TIMESLICE: u32 = 20;

pub fn base_priority(policy: SchedulingPolicy) -> u8 {
    match policy {
        SchedulingPolicy::SCHED_FIFO => 100,
        SchedulingPolicy::SCHED_RR => 100,
        SchedulingPolicy::SCHED_NORMAL => 120,
        SchedulingPolicy::SCHED_BATCH => 130,
        SchedulingPolicy::SCHED_IDLE => 139,
    }
}

pub fn default_timeslice(policy: SchedulingPolicy) -> u32 {
    match policy {
        SchedulingPolicy::SCHED_RR => RT_TIMESLICE,
        SchedulingPolicy::SCHED_FIFO => u32::MAX,
        SchedulingPolicy::SCHED_NORMAL => DEFAULT_TIMESLICE,
        SchedulingPolicy::SCHED_BATCH => DEFAULT_TIMESLICE,
        SchedulingPolicy::SCHED_IDLE => 1,
    }
}

/// Linux nice-to-weight mapping (nice -20..+19).
/// Weight determines the proportional share of CPU time.
/// Higher weight = more CPU time.
pub const NICE_WEIGHTS: [u32; 40] = [
    88761, 71755, 56483, 46273, 36291, // nice -20..-16
    29154, 23254, 18705, 14949, 11916, // nice -15..-11
    9548, 7620, 6100, 4904, 3906, // nice -10..-6
    3121, 2501, 1991, 1586, 1277, // nice  -5..-1
    1024, 820, 655, 526, 423, // nice   0..+4
    335, 272, 215, 172, 137, // nice  +5..+9
    110, 87, 70, 56, 45, // nice +10..+14
    36, 29, 23, 18, 15, // nice +15..+19
];

/// Convert a nice value (-20..+19) to a scheduling weight.
pub fn nice_to_weight(nice: i32) -> u32 {
    let idx = (nice + 20) as usize;
    if idx < 40 { NICE_WEIGHTS[idx] } else { 1024 }
}

/// Convert a nice value to a base priority (lower = higher prio).
pub fn nice_to_priority(nice: i32) -> u8 {
    (120 + nice).clamp(100, 139) as u8
}
