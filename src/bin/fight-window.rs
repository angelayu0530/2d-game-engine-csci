use std::{
    error::Error,
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use clap::Parser;
use game_engine::{
    alloc::{Allocator, Backend},
    canvas::Canvas,
    fight::{Fight, FightConfig, HEIGHT, Input, Replay, WIDTH, render::Renderer},
    window::{self, Controls, Game, Key, Scale, WindowConfig},
    with_allocator,
};

#[derive(Debug, Parser)]
#[command(about = "Fight the boss in a window, allocating effect particles through one allocator")]
struct Cli {
    #[arg(long, value_enum, default_value_t = Backend::Pool)]
    allocator: Backend,
    #[arg(
        long,
        help = "Live particles the room is kept topped up to [default: 10000, or the recording's with --replay]"
    )]
    particles: Option<usize>,
    #[arg(long, default_value_t = 1, conflicts_with = "replay")]
    seed: u64,
    #[arg(
        long,
        help = "Write every step's input here on exit, for `fight --replay`"
    )]
    record: Option<PathBuf>,
    #[arg(
        long,
        conflicts_with = "record",
        help = "Watch a fight recorded with --record instead of playing; combine with --particles to change the load"
    )]
    replay: Option<PathBuf>,
    #[arg(
        long,
        requires = "replay",
        help = "Render the replay to this H.264 video (60 fps, 2x) with ffmpeg and macOS VideoToolbox instead of opening a window"
    )]
    video: Option<PathBuf>,
}

fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    with_allocator!(cli.allocator, play(&cli))
}

/// Where each step's input comes from.
enum Source {
    Keyboard { recorded: Vec<Input> },
    Recording { inputs: Vec<Input>, next: usize },
}

struct Session<A: Allocator> {
    fight: Fight<A>,
    renderer: Renderer,
    source: Source,
}

impl<A: Allocator> Game for Session<A> {
    fn update(&mut self, controls: &Controls, _dt: f32) {
        let input = match &mut self.source {
            Source::Keyboard { recorded } => {
                let input = keyboard(controls);
                recorded.push(input);
                input
            }
            Source::Recording { inputs, next } => {
                // Freeze on the last recorded step rather than inventing input.
                let Some(&input) = inputs.get(*next) else {
                    return;
                };
                *next += 1;
                input
            }
        };
        self.fight.step(input);
    }

    fn draw(&self, canvas: &mut Canvas) {
        self.renderer.draw(&self.fight, canvas);
    }

    fn status(&self) -> Option<String> {
        let particles = format!(
            "{} particles (target {})",
            self.fight.live_particles(),
            self.fight.config().particles
        );
        Some(match &self.source {
            Source::Keyboard { .. } => particles,
            Source::Recording { inputs, next } if *next == inputs.len() => {
                format!("{particles}  replay finished, Esc to close")
            }
            Source::Recording { inputs, next } => {
                format!("{particles}  replay step {next}/{}", inputs.len())
            }
        })
    }
}

fn keyboard(controls: &Controls) -> Input {
    let held = |keys: &[Key]| keys.iter().any(|&key| controls.down(key));
    Input {
        up: held(&[Key::W, Key::Up]),
        down: held(&[Key::S, Key::Down]),
        left: held(&[Key::A, Key::Left]),
        right: held(&[Key::D, Key::Right]),
        attack: controls.down(Key::Space),
        heal: controls.pressed(Key::H),
        spray: controls.down(Key::F),
        restart: controls.pressed(Key::R) || controls.pressed(Key::Enter),
    }
}

fn play<A: Allocator>(cli: &Cli) -> Result<(), Box<dyn Error>> {
    let recording = match &cli.replay {
        Some(path) => Some(Replay::read(BufReader::new(File::open(path)?))?),
        None => None,
    };
    let base = recording.as_ref().map_or(
        FightConfig {
            seed: cli.seed,
            ..FightConfig::default()
        },
        |recording| recording.cfg,
    );
    let cfg = FightConfig {
        particles: cli.particles.unwrap_or(base.particles),
        ..base
    };
    let watching = recording.is_some();
    let config = WindowConfig {
        title: format!(
            "Boss Fight{} [{}]",
            if watching { " replay" } else { "" },
            A::NAME
        ),
        width: WIDTH as usize,
        height: HEIGHT as usize,
        scale: Scale::X2,
        dt: cfg.dt,
        ..WindowConfig::default()
    };
    let expected = recording.as_ref().map(|recording| recording.game_checksum);
    let mut session = Session {
        fight: Fight::<A>::new(&cfg),
        renderer: Renderer::new(),
        source: match recording {
            Some(recording) => Source::Recording {
                inputs: recording.inputs,
                next: 0,
            },
            None => Source::Keyboard {
                recorded: Vec::new(),
            },
        },
    };
    match &cli.video {
        Some(path) => render_video(&mut session, path)?,
        None => window::run(&config, &mut session)?,
    }

    let knight = session.fight.knight();
    let boss = session.fight.boss();
    eprintln!(
        "{} steps at {} particles; knight {} hearts, boss {} hearts",
        session.fight.frame(),
        cfg.particles,
        knight.hp,
        boss.hp
    );
    match session.source {
        Source::Recording { inputs, next } if next < inputs.len() => {
            eprintln!("closed at step {next} of {}", inputs.len());
        }
        Source::Recording { .. } => {
            let matched = Some(session.fight.game_checksum()) == expected;
            eprintln!(
                "replay {} the recorded fight",
                if matched {
                    "reproduced"
                } else {
                    "DIVERGED from"
                }
            );
        }
        Source::Keyboard { recorded } => {
            if let Some(path) = &cli.record {
                Replay {
                    cfg,
                    game_checksum: session.fight.game_checksum(),
                    inputs: recorded,
                }
                .write(BufWriter::new(File::create(path)?))?;
                eprintln!("recorded fight to {}", path.display());
            }
        }
    }
    Ok(())
}

/// Steps through the whole recording, drawing every step exactly as the
/// window would, and pipes the frames to ffmpeg. Frames are produced
/// offline, so the video plays at a steady 60 fps however slow a step is.
fn render_video<A: Allocator>(session: &mut Session<A>, path: &Path) -> Result<(), Box<dyn Error>> {
    let Source::Recording { inputs, next } = &mut session.source else {
        unreachable!("--video requires --replay");
    };
    let (width, height) = (WIDTH as usize, HEIGHT as usize);
    let mut ffmpeg = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-y"])
        .args(["-f", "rawvideo", "-pix_fmt", "bgr0", "-framerate", "60"])
        .args(["-s", &format!("{width}x{height}"), "-i", "-"])
        .args(["-vf", "scale=iw*2:ih*2:flags=neighbor"])
        .args([
            "-c:v",
            "h264_videotoolbox",
            "-b:v",
            "20M",
            "-pix_fmt",
            "yuv420p",
        ])
        .args(["-tag:v", "avc1", "-movflags", "+faststart"])
        .arg(path)
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|err| format!("could not start ffmpeg (brew install ffmpeg): {err}"))?;
    let mut stdin = ffmpeg.stdin.take().expect("ffmpeg stdin is piped");
    let mut canvas = Canvas::new(width, height);
    let mut bytes = Vec::with_capacity(width * height * 4);
    while let Some(&input) = inputs.get(*next) {
        *next += 1;
        session.fight.step(input);
        session.renderer.draw(&session.fight, &mut canvas);
        bytes.clear();
        // 0x00RRGGBB little-endian is B, G, R, 0: ffmpeg's bgr0.
        bytes.extend(canvas.pixels().iter().flat_map(|px| px.to_le_bytes()));
        stdin.write_all(&bytes)?;
        if next.is_multiple_of(60) {
            eprint!("\rrendered {next}/{} frames", inputs.len());
        }
    }
    drop(stdin);
    let status = ffmpeg.wait()?;
    if !status.success() {
        return Err(format!("ffmpeg failed: {status}").into());
    }
    eprintln!("\rwrote {} frames to {}", inputs.len(), path.display());
    Ok(())
}
