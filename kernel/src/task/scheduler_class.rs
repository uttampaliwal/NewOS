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
