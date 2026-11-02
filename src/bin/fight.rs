use std::{
    fs::File,
    hint::black_box,
    io::{self, BufReader, BufWriter, Write},
    path::PathBuf,
};

use clap::Parser;
use game_engine::{
    alloc::{Allocator, Backend},
    fight::{Fight, FightConfig, Frame, Input, Replay, TRACE_HEADER, run},
    memory::peak_rss_bytes,
    with_allocator,
};

const CSV_HEADER: &str = "allocator,particles,frames,trial,knight_hp,boss_hp,victories,defeats,\
                          game_checksum,created,retired,particle_updates,live,peak_live,checksum,\
                          reserved_bytes,setup_ns,sim_ns,teardown_ns,total_ns,peak_rss_bytes";

#[derive(Debug, Parser)]
#[command(about = "Replay a recorded boss fight through one allocator, headless or in a window")]
struct Cli {
    #[arg(long, value_enum)]
    allocator: Backend,
    #[arg(long, help = "Fight recorded with `fight-window --record`")]
    replay: PathBuf,
    #[arg(
        long,
        help = "Live particle target to replay at; defaults to the recording's. Gameplay is identical at every target"
    )]
    particles: Option<usize>,
    #[arg(long, help = "Replay only the first N recorded steps")]
    frames: Option<usize>,
    #[arg(long, default_value_t = 0)]
    warmups: u32,
    #[arg(long, default_value_t = 1)]
    trials: u32,
    #[arg(long)]
    no_header: bool,
    #[arg(
        long,
        help = "Write one CSV row per step of every measured trial here, for scripts/plot.py"
    )]
    trace: Option<PathBuf>,
    #[arg(
        long,
        help = "Show each measured trial in a window as it runs (needs the default `window` feature). \
                Drawing is excluded from sim_ns, which becomes the sum of per-step times"
    )]
    watch: bool,
}

#[cfg(feature = "window")]
mod view {
    use std::time::{Duration, Instant};

    use game_engine::{
        alloc::Allocator,
        canvas::Canvas,
        fight::{Fight, HEIGHT, WIDTH, render::Renderer},
        window::{Error, Scale, Window, WindowConfig},
    };

    /// Redraw at most this often, so heavy loads are not slowed by drawing
    /// every step.
    const REDRAW: Duration = Duration::from_millis(16);

    /// A window showing the fight being benchmarked. Closing it (or Esc)
    /// stops the drawing; the benchmark carries on headless.
    pub struct View {
        window: Option<Window>,
        canvas: Canvas,
        renderer: Renderer,
        title: String,
        drawn: Option<Instant>,
    }

    impl View {
        pub fn open(title: String) -> Result<Self, Error> {
            let window = Window::open(&WindowConfig {
                title: title.clone(),
                width: WIDTH as usize,
                height: HEIGHT as usize,
                scale: Scale::X2,
                fps: 0,
                ..WindowConfig::default()
            })?;
            Ok(Self {
                window: Some(window),
                canvas: Canvas::new(WIDTH as usize, HEIGHT as usize),
                renderer: Renderer::new(),
                title,
                drawn: None,
            })
        }

        pub fn show<A: Allocator>(
            &mut self,
            fight: &Fight<A>,
            trial: u32,
            step: usize,
            steps: usize,
        ) {
            let Some(window) = &mut self.window else {
                return;
            };
            if !window.is_open() {
                self.window = None;
                return;
            }
            if step < steps && self.drawn.is_some_and(|at| at.elapsed() < REDRAW) {
                return;
            }
            self.renderer.draw(fight, &mut self.canvas);
            window.set_title(&format!(
                "{}  trial {}  step {step}/{steps}  {} particles",
                self.title,
                trial + 1,
                fight.live_particles()
            ));
            if window.present(&self.canvas).is_err() {
                self.window = None;
            }
            self.drawn = Some(Instant::now());
        }
    }
}

fn main() -> io::Result<()> {
    let cli = Cli::parse();
    if cfg!(debug_assertions) {
        eprintln!("warning: debug build; timings are not representative, use --release");
    }
    if cli.watch && !cfg!(feature = "window") {
        return Err(io::Error::other(
            "--watch needs a build with the default `window` feature",
        ));
    }
    let recorded = Replay::read(BufReader::new(File::open(&cli.replay)?))?;
    with_allocator!(cli.allocator, bench(&cli, &recorded))
}

fn bench<A: Allocator>(cli: &Cli, recorded: &Replay) -> io::Result<()> {
    let cfg = FightConfig {
        particles: cli.particles.unwrap_or(recorded.cfg.particles),
        ..recorded.cfg
    };
    let frames = cli
        .frames
        .unwrap_or(recorded.inputs.len())
        .min(recorded.inputs.len());
    let inputs: &[Input] = &recorded.inputs[..frames];
    let complete = frames == recorded.inputs.len();

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
        black_box(run::<A>(&cfg, inputs, None));
    }
    if !cli.no_header {
        println!("{CSV_HEADER}");
    }
    #[cfg(feature = "window")]
    let mut view = match cli.watch {
        true => Some(
            view::View::open(format!(
                "Boss Fight bench [{}] {} particles",
                A::NAME,
                cfg.particles
            ))
            .map_err(io::Error::other)?,
        ),
        false => None,
    };
    let tracing = trace_out.is_some();
    for trial in 0..cli.trials {
        trace_buf.clear();
        let mut observe = |fight: &Fight<A>, frame: Frame| {
            if tracing {
                trace_buf.push(frame);
            }
            #[cfg(feature = "window")]
            if let Some(view) = &mut view {
                view.show(fight, trial, frame.index as usize + 1, frames);
            }
            #[cfg(not(feature = "window"))]
            let _ = fight;
        };
        let result = if tracing || cli.watch {
            run::<A>(&cfg, inputs, Some(&mut observe))
        } else {
            run::<A>(&cfg, inputs, None)
        };
        if complete && result.game_checksum != recorded.game_checksum {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "replay diverged from the recording: gameplay checksum {} != recorded {}",
                    result.game_checksum, recorded.game_checksum
                ),
            ));
        }
        let total = result.setup + result.sim + result.teardown;
        println!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
            A::NAME,
            cfg.particles,
            frames,
            trial,
            result.knight_hp,
            result.boss_hp,
            result.victories,
            result.defeats,
            result.game_checksum,
            result.created,
            result.retired,
            result.particle_updates,
            result.live,
            result.peak_live,
            result.checksum,
            result.peak_reserved_bytes,
            result.setup.as_nanos(),
            result.sim.as_nanos(),
            result.teardown.as_nanos(),
            total.as_nanos(),
            peak_rss_bytes(),
        );
        if let Some(out) = &mut trace_out {
            for frame in &trace_buf {
                frame.write_csv(out, A::NAME, cfg.particles, trial)?;
            }
        }
    }
    if let Some(mut out) = trace_out {
        out.flush()?;
    }
    Ok(())
}
