use crate::{
    alloc::{arena::ArenaAllocator, heap::HeapAllocator, pool::PoolAllocator},
    memory::peak_rss_bytes,
    workload::{WorkloadConfig, run},
};

#[allow(dead_code)]
mod alloc;
#[allow(dead_code)]
mod memory;
#[allow(dead_code)]
mod particle;
#[allow(dead_code)]
mod workload;

fn main() {
    let cfg = WorkloadConfig {
        count: 10_000,
        updates: 100,
        dt: 1.0 / 60.0,
    };

    let _heap = run::<HeapAllocator>(&cfg);
    let _pool = run::<PoolAllocator>(&cfg);
    let _arena = run::<ArenaAllocator>(&cfg);
    let _peak = peak_rss_bytes();
}
