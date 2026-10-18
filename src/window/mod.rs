mod input;

use std::{
    fmt,
    time::{Duration, Instant},
};

use minifb::{MouseMode, ScaleMode, WindowOptions};

pub use input::{Controls, Key, MouseButton};

use crate::canvas::Canvas;

const MAX_STEPS_PER_FRAME: u32 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scale {
    X1,
    X2,
    X4,
    FitScreen,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WindowConfig {
    pub title: String,
    pub width: usize,
    pub height: usize,
    pub scale: Scale,
    pub resizable: bool,
    pub fps: usize,
    pub dt: f32,
    pub quit_key: Option<Key>,
    pub stats_interval: Option<Duration>,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            title: String::from("game"),
            width: 800,
            height: 600,
            scale: Scale::X1,
            resizable: false,
            fps: 60,
            dt: 1.0 / 60.0,
            quit_key: Some(Key::Escape),
            stats_interval: Some(Duration::from_millis(500)),
        }
    }
}

pub trait Game {
    fn update(&mut self, controls: &Controls, dt: f32);

    fn draw(&self, canvas: &mut Canvas);

    fn status(&self) -> Option<String> {
        None
    }

    fn running(&self) -> bool {
        true
    }
}

pub fn run(config: &WindowConfig, game: &mut impl Game) -> Result<(), Error> {
    let mut window = Window::open(config)?;
    let mut canvas = Canvas::new(config.width, config.height);
    let mut controls = Controls::default();
    let mut clock = FixedStep::new(config.dt);
    let mut stats = Stats::new();

    while window.is_open() && game.running() {
        window.poll(&mut controls);
        for _ in 0..clock.due() {
            let start = Instant::now();
            game.update(&controls, config.dt);
            stats.update_time += start.elapsed();
            stats.updates += 1;
            controls.settle();
        }

        game.draw(&mut canvas);
        window.present(&canvas)?;
        stats.frames += 1;

        if let Some(interval) = config.stats_interval {
            let elapsed = stats.since.elapsed();
            if elapsed >= interval {
                let mut title = format!(
                    "{}  {:.0} fps  {:.1} µs/update",
                    config.title,
                    f64::from(stats.frames) / elapsed.as_secs_f64(),
                    stats.update_time.as_secs_f64() * 1e6 / f64::from(stats.updates.max(1)),
                );
                if let Some(status) = game.status() {
                    title.push_str("  ");
                    title.push_str(&status);
                }
                window.set_title(&title);
                stats = Stats::new();
            }
        }
    }
    Ok(())
}

struct Stats {
    since: Instant,
    frames: u32,
    updates: u32,
    update_time: Duration,
}

impl Stats {
    fn new() -> Self {
        Self {
            since: Instant::now(),
            frames: 0,
            updates: 0,
            update_time: Duration::ZERO,
        }
    }
}

pub struct Window {
    inner: minifb::Window,
    quit_key: Option<Key>,
}

impl Window {
    pub fn open(config: &WindowConfig) -> Result<Self, Error> {
        let options = WindowOptions {
            resize: config.resizable,
            scale: match config.scale {
                Scale::X1 => minifb::Scale::X1,
                Scale::X2 => minifb::Scale::X2,
                Scale::X4 => minifb::Scale::X4,
                Scale::FitScreen => minifb::Scale::FitScreen,
            },
            scale_mode: if config.resizable {
                ScaleMode::AspectRatioStretch
            } else {
                ScaleMode::Stretch
            },
            ..WindowOptions::default()
        };
        let mut inner = minifb::Window::new(&config.title, config.width, config.height, options)
            .map_err(Error)?;
        inner.set_target_fps(config.fps);
        Ok(Self {
            inner,
            quit_key: config.quit_key,
        })
    }

    pub fn is_open(&self) -> bool {
        self.inner.is_open()
            && !self
                .quit_key
                .is_some_and(|key| self.inner.is_key_down(key.into()))
    }

    pub fn poll(&self, controls: &mut Controls) {
        controls.poll(
            |key| self.inner.is_key_down(key.into()),
            |button| self.inner.get_mouse_down(button.into()),
            self.inner
                .get_mouse_pos(MouseMode::Discard)
                .map(|(x, y)| [x, y]),
        );
    }

    pub fn set_title(&mut self, title: &str) {
        self.inner.set_title(title);
    }

    pub fn present(&mut self, canvas: &Canvas) -> Result<(), Error> {
        self.inner
            .update_with_buffer(canvas.pixels(), canvas.width(), canvas.height())
            .map_err(Error)
    }
}

pub struct FixedStep {
    step: Duration,
    pending: Duration,
    last: Instant,
}

impl FixedStep {
    pub fn new(dt: f32) -> Self {
        Self {
            step: Duration::from_secs_f32(dt),
            pending: Duration::ZERO,
            last: Instant::now(),
        }
    }

    pub fn due(&mut self) -> u32 {
        let now = Instant::now();
        self.pending += now - self.last;
        self.last = now;
        let owed = self.pending.as_nanos() / self.step.as_nanos();
        let steps = owed.min(u128::from(MAX_STEPS_PER_FRAME)) as u32;
        self.pending = if owed > u128::from(steps) {
            Duration::ZERO
        } else {
            self.pending - self.step * steps
        };
        steps
    }
}

#[derive(Debug)]
pub struct Error(minifb::Error);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "window: {}", self.0)
    }
}

impl std::error::Error for Error {}
