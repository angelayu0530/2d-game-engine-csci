use std::hint::black_box;

use clap::{Parser, ValueEnum};
use game_engine::{
    alloc::{Allocator, arena::ArenaAllocator, heap::HeapAllocator, pool::PoolAllocator},
    memory::peak_rss_bytes,
    pong::{PongConfig, run},
};

const CSV_HEADER: &str = "allocator,frames,trail,burst,lifetime,trial,score_left,score_right,\
                          created,retired,particle_updates,live,peak_live,checksum,\
                          reserved_bytes,sim_ns,peak_rss_bytes";

#[derive(Debug, Parser)]
#[command(about = "Run a headless Pong match through one allocator")]
struct Cli {
    #[arg(long, value_enum)]
    allocator: Backend,
    #[arg(long, default_value_t = 600)]
    frames: u32,
    #[arg(long, default_value_t = 1.0 / 60.0)]
    dt: f32,
    #[arg(long, default_value_t = 64)]
    trail: usize,
    #[arg(long, default_value_t = 32)]
    burst: usize,
    #[arg(long, default_value_t = 45)]
    lifetime: u32,
    #[arg(long, default_value_t = 1)]
    seed: u64,
    #[arg(long, default_value_t = 0)]
    warmups: u32,
    #[arg(long, default_value_t = 1)]
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
        Backend::Heap => play::<HeapAllocator>(&cli),
        Backend::Pool => play::<PoolAllocator>(&cli),
        Backend::Arena => play::<ArenaAllocator>(&cli),
    }
}

fn play<A: Allocator>(cli: &Cli) {
    let cfg = PongConfig {
        dt: cli.dt,
        trail: cli.trail,
        burst: cli.burst,
        particle_lifetime: cli.lifetime,
        seed: cli.seed,
    };
    for _ in 0..cli.warmups {
        black_box(run::<A>(&cfg, cli.frames));
    }
    if !cli.no_header {
        println!("{CSV_HEADER}");
    }
    for trial in 0..cli.trials {
        let result = run::<A>(&cfg, cli.frames);
        println!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
            A::NAME,
            cli.frames,
            cfg.trail,
            cfg.burst,
            cfg.particle_lifetime,
            trial,
            result.score[0],
            result.score[1],
            result.created,
            result.retired,
            result.particle_updates,
            result.live_particles,
            result.peak_live,
            result.checksum,
            result.peak_reserved_bytes,
            result.elapsed.as_nanos(),
            peak_rss_bytes(),
        );
    }
}
