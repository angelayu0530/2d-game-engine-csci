use clap::Parser;
use game_engine::{
    alloc::{Allocator, Backend},
    memory::peak_rss_bytes,
    pokemon::{Config, Frame, Observer, Replay, benchmark_session, replay},
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
    #[arg(
        long,
        help = "Show measured trials in a window; drawing is excluded from sim_ns"
    )]
    watch: bool,
    #[arg(
        long,
        help = "Maintain this exact live particle target independently of gameplay"
    )]
    particles: Option<usize>,
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
#[cfg(feature = "window")]
mod view {
    use std::time::{Duration, Instant};

    use game_engine::{
        alloc::Allocator,
        canvas::Canvas,
        pokemon::{
            Pokemon,
            render::{HEIGHT, Hud, Renderer, WIDTH},
        },
        window::{Error, Scale, Window, WindowConfig},
    };

    /// Redraw at most this often, so heavy loads are not slowed by drawing
    /// every step.
    const REDRAW: Duration = Duration::from_millis(16);

    /// A window showing the Pokémon session being benchmarked. Closing it (or Esc)
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
                width: WIDTH,
                height: HEIGHT,
                scale: Scale::X2,
                fps: 0,
                ..WindowConfig::default()
            })?;
            Ok(Self {
                window: Some(window),
                canvas: Canvas::new(WIDTH, HEIGHT),
                renderer: Renderer::new(),
                title,
                drawn: None,
            })
        }

        pub fn show<A: Allocator>(
            &mut self,
            fight: &Pokemon<A>,
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
            self.renderer.draw(fight, &Hud::default(), &mut self.canvas);
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
    if cli.watch && !cfg!(feature = "window") {
        return Err(io::Error::other("--watch needs the default window feature"));
    }
    if cli.particles == Some(0) {
        return Err(io::Error::other("--particles must be positive"));
    }
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
    // Validate the original complete recording before changing its visual workload.
    let expected_game = recording
        .as_ref()
        .map(|r| replay::<A>(r, None).map(|o| o.game_checksum))
        .transpose()?;
    let once = |observe: Observer<'_, A>| {
        benchmark_session::<A>(cfg, cli.frames, recording.as_ref(), cli.particles, observe)
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
            "allocator,frames,burst,lifetime,seed,trial,encounters,wins,losses,catches,level,created,retired,particle_updates,live,peak_live,checksum,reserved_bytes,sim_ns,peak_rss_bytes,mode,initial_live,setup_ns,teardown_ns,total_ns,trace_enabled,trail,dt,game_checksum,particles,watch"
        );
    }
    #[cfg(feature = "window")]
    let mut view = if cli.watch {
        Some(view::View::open(format!("Pokemon bench [{}]", A::NAME)).map_err(io::Error::other)?)
    } else {
        None
    };
    let mut samples = Vec::<Frame>::new();
    for trial in 0..cli.trials {
        samples.clear();
        let tracing = trace.is_some();
        if tracing {
            samples.reserve(steps);
        }
        let mut observe = |game: &game_engine::pokemon::Pokemon<A>, frame: Frame| {
            if tracing {
                samples.push(frame);
            }
            #[cfg(feature = "window")]
            if let Some(view) = &mut view {
                view.show(game, trial, frame.index as usize + 1, steps);
            }
            #[cfg(not(feature = "window"))]
            let _ = game;
        };
        let result = once(if tracing || cli.watch {
            Some(&mut observe)
        } else {
            None
        })?;
        if expected_game.is_some_and(|expected| expected != result.game_checksum) {
            return Err(io::Error::other(
                "Replay gameplay diverged from the recording",
            ));
        }
        if cli.particles.is_none()
            && recording
                .as_ref()
                .is_some_and(|r| r.checksum != result.checksum)
        {
            return Err(io::Error::other("Replay full checksum mismatch"));
        }
        println!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
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
            cfg.dt,
            result.game_checksum,
            cli.particles.map_or(String::new(), |n| n.to_string()),
            cli.watch
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
