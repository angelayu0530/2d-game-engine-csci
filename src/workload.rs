use std::time::Duration;

use crate::alloc::Allocator;

#[derive(Debug, Clone, Copy)]
pub struct WorkloadConfig {
    pub count: usize,
    pub updates: u32,
    pub dt: f32,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Phases {
    pub setup: Duration,
    pub spawn: Duration,
    pub update: Duration,
    pub retire: Duration,
    pub reclaim: Duration,
}

#[derive(Debug, Clone, Copy)]
pub struct RunResult {
    pub phases: Phases,
    pub checksum: u64,
    pub reserved_bytes: usize,
}

pub fn run<A: Allocator>(_cfg: &WorkloadConfig) -> RunResult {
    todo!()
}
