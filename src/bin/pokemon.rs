use clap::Parser;
use game_engine::{
    alloc::{Allocator, Backend},
    memory::peak_rss_bytes,
    pokemon::{Config, Frame, Replay, replay, run},
    with_allocator,
};
use std::{
    fs::File,
    hint::black_box,
    io::{self, BufWriter, Write},
    path::PathBuf,
};

#[derive(Parser)]
#[command(about = "Headless Pokemon-style expedition and allocator benchmark")]
struct Cli {
    #[arg(long, value_enum)]
    allocator: Backend,
    #[arg(long, default_value_t = 3600)]
    frames: u32,
    #[arg(long, default_value_t = 1)]
    seed: u64,
    #[arg(long, default_value_t = 32)]
    burst: usize,
    #[arg(long, default_value_t = 64)]
    trail: usize,
    #[arg(long, default_value_t = 1.0 / 60.0)]
    dt: f32,
    #[arg(long, default_value_t = 45, value_parser = clap::value_parser!(u32).range(1..))]
    lifetime: u32,
    #[arg(long, default_value_t = 0)]
    warmups: u32,
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
    trials: u32,
    #[arg(long)]
    trace: Option<PathBuf>,
    #[arg(long)]
    no_header: bool,
    #[arg(long, conflicts_with_all = ["frames", "seed", "burst", "lifetime", "trail", "dt"], help = "Benchmark an exact recorded player session")]
    replay: Option<PathBuf>,
}
fn main() -> io::Result<()> {
    let cli = Cli::parse();
    if !cli.dt.is_finite() || cli.dt <= 0.0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "dt must be finite and positive",
        ));
    }
    with_allocator!(cli.allocator, benchmark(&cli))
}
fn benchmark<A: Allocator>(cli: &Cli) -> io::Result<()> {
    let recording = cli
        .replay
        .as_ref()
        .map(|path| Replay::read(File::open(path)?))
        .transpose()?;
    let (cfg, steps) = if let Some(recording) = &recording {
        (recording.config()?, recording.actions.len())
    } else {
        (
            Config {
                trail: cli.trail,
                dt: cli.dt,
                seed: cli.seed,
                burst: cli.burst,
                lifetime: cli.lifetime,
            },
            cli.frames as usize,
        )
    };
    let mode = if recording.is_some() {
        "pokemon-replay"
    } else {
        "pokemon-auto"
    };
    let once = |trace: Option<&mut dyn FnMut(Frame)>| match &recording {
        Some(r) => replay::<A>(r, trace),
        None => Ok(run::<A>(cfg, cli.frames, trace)),
    };
    let mut trace = cli
        .trace
        .as_ref()
        .map(File::create)
        .transpose()?
        .map(BufWriter::new);
    if let Some(out) = &mut trace {
        writeln!(
            out,
            "mode,allocator,trial,frame,step_ns,live,reserved_bytes,wins,catches"
        )?;
    }
    for _ in 0..cli.warmups {
        black_box(once(None)?);
    }
    if !cli.no_header {
        println!(
            "allocator,frames,burst,lifetime,seed,trial,encounters,wins,losses,catches,level,created,retired,particle_updates,live,peak_live,checksum,reserved_bytes,sim_ns,peak_rss_bytes,mode,initial_live,setup_ns,teardown_ns,total_ns,trace_enabled,trail,dt"
        );
    }
    let mut samples = Vec::<Frame>::new();
    for trial in 0..cli.trials {
        samples.clear();
        let result = if trace.is_some() {
            samples.reserve(steps);
            once(Some(&mut |frame| samples.push(frame)))?
        } else {
            once(None)?
        };
        println!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
            A::NAME,
            steps,
            cfg.burst,
            cfg.lifetime,
            cfg.seed,
            trial,
            result.state.encounters,
            result.state.wins,
            result.state.losses,
            result.state.catches,
            result.state.starter.level,
            result.created,
            result.retired,
            result.particle_updates,
            result.live,
            result.peak_live,
            result.checksum,
            result.reserved_bytes,
            result.elapsed.as_nanos(),
            peak_rss_bytes(),
            mode,
            result.initial_live,
            result.setup.as_nanos(),
            result.teardown.as_nanos(),
            (result.setup + result.elapsed + result.teardown).as_nanos(),
            trace.is_some(),
            cfg.trail,
            cfg.dt
        );
        if let Some(out) = &mut trace {
            for f in &samples {
                writeln!(
                    out,
                    "{mode},{},{},{},{},{},{},{},{}",
                    A::NAME,
                    trial,
                    f.index,
                    f.step.as_nanos(),
                    f.live,
                    f.reserved_bytes,
                    f.wins,
                    f.catches
                )?;
            }
        }
    }
    if let Some(out) = &mut trace {
        out.flush()?;
    }
    Ok(())
}
