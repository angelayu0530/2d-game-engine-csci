use std::{error::Error, fs::File, io::BufWriter, path::PathBuf};

use clap::{Parser, ValueEnum};
use game_engine::{
    alloc::{Allocator, arena::ArenaAllocator, heap::HeapAllocator, pool::PoolAllocator},
    canvas::Canvas,
    pong::{
        BALL_RADIUS, Cpu, CpuConfig, HEIGHT, Input, LEFT_FACE, PADDLE_HALF_H, Pong, PongConfig,
        RIGHT_FACE, Replay, Side, WIDTH,
    },
    window::{self, Controls, Game, Key, WindowConfig},
};

const DT: f32 = 1.0 / 60.0;
const PADDLE_W: f32 = 12.0;
const SCORE_CELL: f32 = 8.0;
const BACKGROUND: u32 = 0x10_14_18;
const FOREGROUND: u32 = 0xE8_E8_E8;
const NET: u32 = 0x40_44_48;
const SPARK: [u32; 3] = [0xFF, 0xB0, 0x40];

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

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Backend {
    Heap,
    Pool,
    Arena,
}

fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    match cli.allocator {
        Backend::Heap => play::<HeapAllocator>(&cli),
        Backend::Pool => play::<PoolAllocator>(&cli),
        Backend::Arena => play::<ArenaAllocator>(&cli),
    }
}

struct PongGame<A: Allocator> {
    game: Pong<A>,
    cfg: PongConfig,
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
        draw(&self.game, &self.cfg, canvas);
    }

    fn status(&self) -> Option<String> {
        Some(format!("{} particles", self.game.live_particles()))
    }
}

fn play<A: Allocator>(cli: &Cli) -> Result<(), Box<dyn Error>> {
    let cfg = PongConfig {
        dt: DT,
        trail: cli.trail,
        burst: cli.burst,
        particle_lifetime: cli.lifetime,
        seed: cli.seed,
    };
    let config = WindowConfig {
        title: format!("Pong [{}]", A::NAME),
        width: WIDTH as usize,
        height: HEIGHT as usize,
        dt: DT,
        ..WindowConfig::default()
    };
    let mut pong = PongGame {
        game: Pong::<A>::new(&cfg),
        cfg,
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

fn draw<A: Allocator>(game: &Pong<A>, cfg: &PongConfig, canvas: &mut Canvas) {
    canvas.clear(BACKGROUND);
    for y in (0..HEIGHT as u32).step_by(32) {
        canvas.fill_rect(WIDTH / 2.0 - 2.0, y as f32 + 8.0, 4.0, 16.0, NET);
    }

    let [left, right] = game.score();
    let left_width = (left.to_string().len() * 4 - 1) as f32 * SCORE_CELL;
    canvas.number(
        left,
        WIDTH / 2.0 - 48.0 - left_width,
        32.0,
        SCORE_CELL,
        FOREGROUND,
    );
    canvas.number(right, WIDTH / 2.0 + 48.0, 32.0, SCORE_CELL, FOREGROUND);

    let max_life = cfg.particle_lifetime.max(1) as f32;
    for particle in game.particles() {
        let color = fade(particle.lifetime as f32 / max_life);
        canvas.fill_rect(
            particle.pos[0] - 1.5,
            particle.pos[1] - 1.5,
            3.0,
            3.0,
            color,
        );
    }

    let paddle_h = PADDLE_HALF_H * 2.0;
    let left_y = game.paddle_y(Side::Left) - PADDLE_HALF_H;
    let right_y = game.paddle_y(Side::Right) - PADDLE_HALF_H;
    canvas.fill_rect(
        LEFT_FACE - BALL_RADIUS - PADDLE_W,
        left_y,
        PADDLE_W,
        paddle_h,
        FOREGROUND,
    );
    canvas.fill_rect(
        RIGHT_FACE + BALL_RADIUS,
        right_y,
        PADDLE_W,
        paddle_h,
        FOREGROUND,
    );

    let ball = game.ball().pos;
    let size = BALL_RADIUS * 2.0;
    canvas.fill_rect(
        ball[0] - BALL_RADIUS,
        ball[1] - BALL_RADIUS,
        size,
        size,
        FOREGROUND,
    );
}

fn fade(t: f32) -> u32 {
    let t = t.clamp(0.0, 1.0);
    let channel = |shift: u32, spark: u32| {
        let bg = (BACKGROUND >> shift & 0xFF) as f32;
        ((bg + (spark as f32 - bg) * t) as u32) << shift
    };
    channel(16, SPARK[0]) | channel(8, SPARK[1]) | channel(0, SPARK[2])
}
