use std::{
    fs::File,
    hint::black_box,
    io::{self, BufReader, BufWriter, Write},
    path::PathBuf,
};

use clap::Parser;
use game_engine::{
    alloc::{Allocator, Backend},
    memory::peak_rss_bytes,
    pong::{CpuConfig, Frame, Outcome, PongConfig, Replay, TRACE_HEADER, TraceMode, replay, run},
    with_allocator,
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
    #[arg(
        long,
        help = "Write one CSV row per frame of every measured trial here, for scripts/plot.py"
    )]
    trace: Option<PathBuf>,
}

fn main() -> io::Result<()> {
    let cli = Cli::parse();
    if cfg!(debug_assertions) {
        eprintln!("warning: debug build; timings are not representative, use --release");
    }
    let recorded = match &cli.replay {
        Some(path) => Some(Replay::read(BufReader::new(File::open(path)?))?),
        None => None,
    };
    let recorded = recorded.as_ref();
    with_allocator!(cli.allocator, play(&cli, recorded))
}

fn play<A: Allocator>(cli: &Cli, recorded: Option<&Replay>) -> io::Result<()> {
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
    let match_once = |trace: Option<&mut dyn FnMut(Frame)>| -> Outcome {
        match recorded {
            Some(recorded) => replay::<A>(recorded, trace),
            None => run::<A>(&cfg, &cpu, cli.frames, trace),
        }
    };
    let mut trace_out = match &cli.trace {
        Some(path) => {
            let mut out = BufWriter::new(File::create(path)?);
            writeln!(out, "{TRACE_HEADER}")?;
            Some(out)
        }
        None => None,
    };
    let mut trace_buf: Vec<Frame> =
        Vec::with_capacity(if trace_out.is_some() { frames } else { 0 });

    for _ in 0..cli.warmups {
        black_box(match_once(None));
    }
    if !cli.no_header {
        println!("{CSV_HEADER}");
    }
    for trial in 0..cli.trials {
        trace_buf.clear();
        let result = match &mut trace_out {
            Some(_) => match_once(Some(&mut |frame| trace_buf.push(frame))),
            None => match_once(None),
        };
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
        if let Some(out) = &mut trace_out {
            let mode = match recorded {
                Some(_) => TraceMode::HumanReplay,
                None => TraceMode::CpuVsCpu,
            };
            for frame in &trace_buf {
                frame.write_csv(out, mode, A::NAME, trial)?;
            }
        }
    }
    if let Some(mut out) = trace_out {
        out.flush()?;
    }
    Ok(())
}
