use std::{
    hint::black_box,
    io::{self, Write},
    time::{Duration, Instant},
};

use crate::{alloc::Allocator, memory, particle::Particle};

pub const TRACE_HEADER: &str = "mode,allocator,count,trial,frame,phase,step_ns,live,\
                                reserved_bytes,created,retired,minor_faults,rss_bytes";

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Setup plus creating every particle.
    Spawn,
    /// One pass updating every particle.
    Update,
    /// Retiring every particle, reclaiming, and dropping the allocator.
    Teardown,
}

impl Phase {
    pub fn label(self) -> &'static str {
        match self {
            Phase::Spawn => "spawn",
            Phase::Update => "update",
            Phase::Teardown => "teardown",
        }
    }
}

/// One traced step: frame 0 spawns, frames `1..=updates` update, and the last
/// frame tears down. `created`, `retired`, and `minor_faults` count this step
/// only; `rss_bytes` is read after it, outside its timing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub index: u32,
    pub phase: Phase,
    pub step: Duration,
    pub live: usize,
    pub reserved_bytes: usize,
    pub created: u64,
    pub retired: u64,
    pub minor_faults: u64,
    pub rss_bytes: usize,
}

impl Frame {
    pub fn write_csv(
        &self,
        out: &mut impl Write,
        allocator: &str,
        count: usize,
        trial: u32,
    ) -> io::Result<()> {
        writeln!(
            out,
            "basic,{allocator},{count},{trial},{},{},{},{},{},{},{},{},{}",
            self.index,
            self.phase.label(),
            self.step.as_nanos(),
            self.live,
            self.reserved_bytes,
            self.created,
            self.retired,
            self.minor_faults,
            self.rss_bytes,
        )
    }
}

/// Runs the workload once. With `observe`, every update pass is timed on its
/// own and `phases.update` is the sum of those times, so the observer is not
/// counted.
pub fn run<A: Allocator>(
    cfg: &WorkloadConfig,
    mut observe: Option<&mut dyn FnMut(Frame)>,
) -> RunResult {
    let mut phases = Phases::default();
    let count = cfg.count as u64;
    let mut faults = observe.as_ref().map_or(0, |_| memory::minor_faults());

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
    if let Some(observe) = observe.as_deref_mut() {
        let now = memory::minor_faults();
        observe(Frame {
            index: 0,
            phase: Phase::Spawn,
            step: phases.setup + phases.spawn,
            live: alloc.live_count(),
            reserved_bytes,
            created: count,
            retired: 0,
            minor_faults: now - faults,
            rss_bytes: memory::current_rss_bytes(),
        });
        faults = memory::minor_faults();
    }

    match observe.as_deref_mut() {
        None => {
            let start = Instant::now();
            for _ in 0..cfg.updates {
                update(&mut alloc, &handles, cfg.dt);
            }
            phases.update = start.elapsed();
        }
        Some(observe) => {
            for index in 1..=cfg.updates {
                let start = Instant::now();
                update(&mut alloc, &handles, cfg.dt);
                let step = start.elapsed();
                phases.update += step;
                let now = memory::minor_faults();
                observe(Frame {
                    index,
                    phase: Phase::Update,
                    step,
                    live: alloc.live_count(),
                    reserved_bytes: alloc.reserved_bytes(),
                    created: 0,
                    retired: 0,
                    minor_faults: now - faults,
                    rss_bytes: memory::current_rss_bytes(),
                });
                faults = memory::minor_faults();
            }
        }
    }

    let checksum = black_box(checksum(&alloc, &handles));
    if observe.is_some() {
        faults = memory::minor_faults();
    }

    let start = Instant::now();
    for handle in handles.drain(..) {
        alloc.retire(handle);
    }
    phases.retire = start.elapsed();

    let start = Instant::now();
    alloc.reclaim();
    drop(alloc);
    phases.reclaim = start.elapsed();
    if let Some(observe) = observe {
        observe(Frame {
            index: cfg.updates + 1,
            phase: Phase::Teardown,
            step: phases.retire + phases.reclaim,
            live: 0,
            reserved_bytes: 0,
            created: 0,
            retired: count,
            minor_faults: memory::minor_faults() - faults,
            rss_bytes: memory::current_rss_bytes(),
        });
    }

    RunResult {
        phases,
        checksum,
        reserved_bytes,
    }
}

fn update<A: Allocator>(alloc: &mut A, handles: &[A::Handle], dt: f32) {
    for &handle in handles {
        alloc.get_mut(handle).update(dt);
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
        let heap = run::<HeapAllocator>(&cfg, None).checksum;
        assert_eq!(run::<PoolAllocator>(&cfg, None).checksum, heap, "pool");
        assert_eq!(run::<ArenaAllocator>(&cfg, None).checksum, heap, "arena");
    }
}
