use std::hint::black_box;

use clap::{Parser, ValueEnum};
use game_engine::{
    alloc::{Allocator, arena::ArenaAllocator, heap::HeapAllocator, pool::PoolAllocator},
    memory::peak_rss_bytes,
    workload::{WorkloadConfig, run},
};

const CSV_HEADER: &str = "allocator,count,updates,trial,setup_ns,spawn_ns,update_ns,retire_ns,\
                          reclaim_ns,total_ns,checksum,reserved_bytes,peak_rss_bytes";

#[derive(Debug, Parser)]
#[command(version)]
struct Cli {
    #[arg(long, value_enum)]
    allocator: Backend,
    #[arg(long, default_value_t = 100_000)]
    count: usize,
    #[arg(long, default_value_t = 100)]
    updates: u32,
    #[arg(long, default_value_t = 1.0 / 60.0)]
    dt: f32,
    #[arg(long, default_value_t = 2)]
    warmups: u32,
    #[arg(long, default_value_t = 10)]
    trials: u32,
    #[arg(long)]
    no_header: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Backend {
    Heap,
    Pool,
    Arena,
}

fn main() {
    let cli = Cli::parse();
    if cfg!(debug_assertions) {
        eprintln!("warning: debug build; timings are not representative, use --release");
    }
    match cli.allocator {
        Backend::Heap => bench::<HeapAllocator>(&cli),
        Backend::Pool => bench::<PoolAllocator>(&cli),
        Backend::Arena => bench::<ArenaAllocator>(&cli),
    }
}

fn bench<A: Allocator>(cli: &Cli) {
    let cfg = WorkloadConfig {
        count: cli.count,
        updates: cli.updates,
        dt: cli.dt,
    };
    for _ in 0..cli.warmups {
        black_box(run::<A>(&cfg));
    }
    if !cli.no_header {
        println!("{CSV_HEADER}");
    }
    for trial in 0..cli.trials {
        let result = run::<A>(&cfg);
        let p = result.phases;
        println!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{}",
            A::NAME,
            cfg.count,
            cfg.updates,
            trial,
            p.setup.as_nanos(),
            p.spawn.as_nanos(),
            p.update.as_nanos(),
            p.retire.as_nanos(),
            p.reclaim.as_nanos(),
            p.total().as_nanos(),
            result.checksum,
            result.reserved_bytes,
            peak_rss_bytes(),
        );
    }
}
