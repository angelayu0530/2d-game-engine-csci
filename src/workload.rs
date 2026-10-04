use std::{
    hint::black_box,
    time::{Duration, Instant},
};

use crate::{alloc::Allocator, particle::Particle};

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

impl Phases {
    pub fn total(&self) -> Duration {
        self.setup + self.spawn + self.update + self.retire + self.reclaim
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RunResult {
    pub phases: Phases,
    pub checksum: u64,
    pub reserved_bytes: usize,
}

pub fn run<A: Allocator>(cfg: &WorkloadConfig) -> RunResult {
    let mut phases = Phases::default();

    let start = Instant::now();
    let mut alloc = A::with_capacity(cfg.count);
    let mut handles = Vec::with_capacity(cfg.count);
    phases.setup = start.elapsed();

    let start = Instant::now();
    for i in 0..cfg.count {
        handles.push(alloc.create(Particle::seeded(i)));
    }
    phases.spawn = start.elapsed();
    let reserved_bytes = alloc.reserved_bytes();

    let start = Instant::now();
    for _ in 0..cfg.updates {
        for &handle in &handles {
            alloc.get_mut(handle).update(cfg.dt);
        }
    }
    phases.update = start.elapsed();

    let checksum = black_box(checksum(&alloc, &handles));

    let start = Instant::now();
    for handle in handles.drain(..) {
        alloc.retire(handle);
    }
    phases.retire = start.elapsed();

    let start = Instant::now();
    alloc.reclaim();
    drop(alloc);
    phases.reclaim = start.elapsed();

    RunResult {
        phases,
        checksum,
        reserved_bytes,
    }
}

fn checksum<A: Allocator>(alloc: &A, handles: &[A::Handle]) -> u64 {
    handles.iter().fold(0xCBF2_9CE4_8422_2325, |hash, &handle| {
        let p = alloc.get(handle);
        [
            p.pos[0].to_bits(),
            p.pos[1].to_bits(),
            p.vel[0].to_bits(),
            p.vel[1].to_bits(),
            p.lifetime,
        ]
        .into_iter()
        .fold(hash, |hash, word| {
            (hash ^ u64::from(word)).wrapping_mul(0x0100_0000_01B3)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::{WorkloadConfig, run};
    use crate::alloc::{arena::ArenaAllocator, heap::HeapAllocator, pool::PoolAllocator};

    #[test]
    fn backends_reach_identical_final_state() {
        let cfg = WorkloadConfig {
            count: 50_000,
            updates: 20,
            dt: 1.0 / 60.0,
        };
        let heap = run::<HeapAllocator>(&cfg).checksum;
        assert_eq!(run::<PoolAllocator>(&cfg).checksum, heap, "pool");
        assert_eq!(run::<ArenaAllocator>(&cfg).checksum, heap, "arena");
    }
}
