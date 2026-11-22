use std::{
    fs::File,
    hint::black_box,
    io::{self, BufWriter, Write},
    path::PathBuf,
};

use clap::Parser;
use game_engine::{
    alloc::{Allocator, Backend},
    memory::peak_rss_bytes,
    with_allocator,
    workload::{Frame, TRACE_HEADER, WorkloadConfig, run},
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
    #[arg(
        long,
        help = "Write one CSV row per step (spawn, each update, teardown) of every measured trial here"
    )]
    trace: Option<PathBuf>,
}

fn main() -> io::Result<()> {
    let cli = Cli::parse();
    if cfg!(debug_assertions) {
        eprintln!("warning: debug build; timings are not representative, use --release");
    }
    with_allocator!(cli.allocator, bench(&cli))
}

fn bench<A: Allocator>(cli: &Cli) -> io::Result<()> {
    let cfg = WorkloadConfig {
        count: cli.count,
        updates: cli.updates,
        dt: cli.dt,
    };
    let mut trace_out = match &cli.trace {
        Some(path) => {
            let mut out = BufWriter::new(File::create(path)?);
            writeln!(out, "{TRACE_HEADER}")?;
            Some(out)
        }
        None => None,
    };
    let mut trace_buf: Vec<Frame> = Vec::with_capacity(match trace_out {
        Some(_) => cfg.updates as usize + 2,
        None => 0,
    });

    for _ in 0..cli.warmups {
        black_box(run::<A>(&cfg, None));
    }
    if !cli.no_header {
        println!("{CSV_HEADER}");
    }
    for trial in 0..cli.trials {
        trace_buf.clear();
        let result = match trace_out {
            Some(_) => run::<A>(&cfg, Some(&mut |frame| trace_buf.push(frame))),
            None => run::<A>(&cfg, None),
        };
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
        if let Some(out) = &mut trace_out {
            for frame in &trace_buf {
                frame.write_csv(out, A::NAME, cfg.count, trial)?;
            }
        }
    }
    if let Some(mut out) = trace_out {
        out.flush()?;
    }
    Ok(())
}
