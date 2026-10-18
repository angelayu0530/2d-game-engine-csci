use std::{fs::File, hint::black_box, io::BufReader, path::PathBuf};

use clap::{Parser, ValueEnum};
use game_engine::{
    alloc::{Allocator, arena::ArenaAllocator, heap::HeapAllocator, pool::PoolAllocator},
    memory::peak_rss_bytes,
    pong::{CpuConfig, Outcome, PongConfig, Replay, replay, run},
};

const CSV_HEADER: &str = "allocator,frames,trail,burst,lifetime,trial,score_left,score_right,\
                          created,retired,particle_updates,live,peak_live,checksum,\
                          reserved_bytes,sim_ns,peak_rss_bytes";

#[derive(Debug, Parser)]
#[command(about = "Run a headless Pong match through one allocator")]
struct Cli {
    #[arg(long, value_enum)]
    allocator: Backend,
    #[arg(long, default_value_t = 3_600)]
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
    #[arg(
        long,
        conflicts_with_all = ["frames", "dt", "trail", "burst", "lifetime", "seed", "cpu_variance"],
        help = "Replay a match recorded by `play --record`; config and frame count come from the file"
    )]
    replay: Option<PathBuf>,
    #[arg(
        long,
        default_value_t = CpuConfig::default().variance,
        help = "Computer aim error per approach, as a fraction of paddle reach; 0 for none"
    )]
    cpu_variance: f32,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Backend {
    Heap,
    Pool,
    Arena,
}

fn main() -> std::io::Result<()> {
    let cli = Cli::parse();
    if cfg!(debug_assertions) {
        eprintln!("warning: debug build; timings are not representative, use --release");
    }
    let recorded = match &cli.replay {
        Some(path) => Some(Replay::read(BufReader::new(File::open(path)?))?),
        None => None,
    };
    let recorded = recorded.as_ref();
    match cli.allocator {
        Backend::Heap => play::<HeapAllocator>(&cli, recorded),
        Backend::Pool => play::<PoolAllocator>(&cli, recorded),
        Backend::Arena => play::<ArenaAllocator>(&cli, recorded),
    }
    Ok(())
}

fn play<A: Allocator>(cli: &Cli, recorded: Option<&Replay>) {
    let (cfg, frames) = match recorded {
        Some(recorded) => (recorded.cfg, recorded.inputs.len()),
        None => (
            PongConfig {
                dt: cli.dt,
                trail: cli.trail,
                burst: cli.burst,
                particle_lifetime: cli.lifetime,
                seed: cli.seed,
            },
            cli.frames as usize,
        ),
    };
    let cpu = CpuConfig {
        variance: cli.cpu_variance,
        seed: cli.seed,
        ..CpuConfig::default()
    };
    let match_once = || -> Outcome {
        match recorded {
            Some(recorded) => replay::<A>(recorded),
            None => run::<A>(&cfg, &cpu, cli.frames),
        }
    };
    for _ in 0..cli.warmups {
        black_box(match_once());
    }
    if !cli.no_header {
        println!("{CSV_HEADER}");
    }
    for trial in 0..cli.trials {
        let result = match_once();
        println!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
            A::NAME,
            frames,
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
