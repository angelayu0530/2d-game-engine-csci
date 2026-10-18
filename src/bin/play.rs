use std::{
    error::Error,
    fs::File,
    io::{BufWriter, Write},
    path::PathBuf,
};

use clap::Parser;
use game_engine::{
    alloc::{Allocator, Backend},
    canvas::Canvas,
    pong::{
        self, Cpu, CpuConfig, Frame, HEIGHT, Input, Pong, PongConfig, Replay, Side, TRACE_HEADER,
        TraceMode, WIDTH,
    },
    window::{self, Controls, Game, Key, WindowConfig},
    with_allocator,
};

#[derive(Debug, Parser)]
#[command(about = "Play Pong in a window, allocating particles through one allocator")]
struct Cli {
    #[arg(long, value_enum, default_value_t = Backend::Pool)]
    allocator: Backend,
    #[arg(long, help = "Right paddle on Up/Down instead of the computer")]
    two_player: bool,
    #[arg(long, default_value_t = 64)]
    trail: usize,
    #[arg(long, default_value_t = 32)]
    burst: usize,
    #[arg(long, default_value_t = 45)]
    lifetime: u32,
    #[arg(long, default_value_t = 1)]
    seed: u64,
    #[arg(
        long,
        default_value_t = CpuConfig::default().variance,
        help = "Computer aim error per approach, as a fraction of paddle reach; 0 for none"
    )]
    cpu_variance: f32,
    #[arg(
        long,
        default_value_t = 11,
        help = "End the match when either side reaches this score; 0 to play until Esc"
    )]
    to: u32,
    #[arg(
        long,
        help = "Write every frame's input here on exit, for `pong --replay`"
    )]
    record: Option<PathBuf>,
    #[arg(
        long,
        help = "Write one CSV row per played frame here on exit, for scripts/plot.py"
    )]
    trace: Option<PathBuf>,
}

fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    with_allocator!(cli.allocator, play(&cli))
}

struct PongGame<A: Allocator> {
    game: Pong<A>,
    cpu: Option<Cpu>,
    to: u32,
    inputs: Vec<Input>,
    frames: Vec<Frame>,
}

impl<A: Allocator> Game for PongGame<A> {
    fn update(&mut self, controls: &Controls, _dt: f32) {
        if !self.running() {
            return;
        }
        let input = match &mut self.cpu {
            None => Input {
                left: controls.axis(&[Key::W], &[Key::S]),
                right: controls.axis(&[Key::Up], &[Key::Down]),
            },
            Some(cpu) => Input {
                left: controls.axis(&[Key::W, Key::Up], &[Key::S, Key::Down]),
                right: cpu.input(self.game.paddle_y(Side::Right), self.game.ball()),
            },
        };
        self.frames.push(self.game.timed_step(input));
        self.inputs.push(input);
    }

    fn draw(&self, canvas: &mut Canvas) {
        pong::draw(&self.game, canvas);
    }

    fn status(&self) -> Option<String> {
        Some(format!("{} particles", self.game.live_particles()))
    }

    fn running(&self) -> bool {
        let [left, right] = self.game.score();
        self.to == 0 || left.max(right) < self.to
    }
}

fn play<A: Allocator>(cli: &Cli) -> Result<(), Box<dyn Error>> {
    let cfg = PongConfig {
        trail: cli.trail,
        burst: cli.burst,
        particle_lifetime: cli.lifetime,
        seed: cli.seed,
        ..PongConfig::default()
    };
    let config = WindowConfig {
        title: format!("Pong [{}]", A::NAME),
        width: WIDTH as usize,
        height: HEIGHT as usize,
        dt: cfg.dt,
        ..WindowConfig::default()
    };
    let mut pong = PongGame {
        game: Pong::<A>::new(&cfg),
        cpu: (!cli.two_player).then(|| {
            Cpu::new(
                Side::Right,
                CpuConfig {
                    variance: cli.cpu_variance,
                    seed: cli.seed,
                    ..CpuConfig::default()
                },
            )
        }),
        to: cli.to,
        inputs: Vec::new(),
        frames: Vec::new(),
    };
    window::run(&config, &mut pong)?;

    let [left, right] = pong.game.score();
    eprintln!(
        "final score {left} : {right} after {} frames",
        pong.inputs.len()
    );
    if let Some(path) = &cli.trace {
        let mut out = BufWriter::new(File::create(path)?);
        writeln!(out, "{TRACE_HEADER}")?;
        for frame in &pong.frames {
            frame.write_csv(&mut out, TraceMode::HumanLive, A::NAME, 0)?;
        }
        out.flush()?;
        eprintln!(
            "wrote {} frame rows to {}",
            pong.frames.len(),
            path.display()
        );
    }
    if let Some(path) = &cli.record {
        Replay {
            cfg,
            inputs: pong.inputs,
        }
        .write(BufWriter::new(File::create(path)?))?;
        eprintln!("recorded match to {}", path.display());
    }
    Ok(())
}
