use std::{error::Error, fs::File, io::BufWriter, path::PathBuf};

use clap::Parser;
use game_engine::{
    alloc::{Allocator, Backend},
    canvas::Canvas,
    pong::{self, Cpu, CpuConfig, HEIGHT, Input, Pong, PongConfig, Replay, Side, WIDTH},
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
        help = "Write every frame's input here on exit, for `pong --replay`"
    )]
    record: Option<PathBuf>,
}

fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    with_allocator!(cli.allocator, play(&cli))
}

struct PongGame<A: Allocator> {
    game: Pong<A>,
    cpu: Option<Cpu>,
    inputs: Vec<Input>,
}

impl<A: Allocator> Game for PongGame<A> {
    fn update(&mut self, controls: &Controls, _dt: f32) {
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
        self.game.step(input);
        self.inputs.push(input);
    }

    fn draw(&self, canvas: &mut Canvas) {
        pong::draw(&self.game, canvas);
    }

    fn status(&self) -> Option<String> {
        Some(format!("{} particles", self.game.live_particles()))
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
        inputs: Vec::new(),
    };
    window::run(&config, &mut pong)?;

    let [left, right] = pong.game.score();
    eprintln!(
        "final score {left} : {right} after {} frames",
        pong.inputs.len()
    );
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
