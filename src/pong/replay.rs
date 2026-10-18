use std::{
    io::{self, BufRead, Write},
    time::Instant,
};

use super::{Input, Outcome, Pong, PongConfig};
use crate::alloc::Allocator;

const MAGIC: &str = "pong-replay v1";

#[derive(Debug, Clone, PartialEq)]
pub struct Replay {
    pub cfg: PongConfig,
    pub inputs: Vec<Input>,
}

impl Replay {
    pub fn write(&self, mut out: impl Write) -> io::Result<()> {
        let cfg = &self.cfg;
        writeln!(out, "{MAGIC}")?;
        writeln!(
            out,
            "dt={} trail={} burst={} lifetime={} seed={}",
            cfg.dt, cfg.trail, cfg.burst, cfg.particle_lifetime, cfg.seed
        )?;
        for input in &self.inputs {
            writeln!(out, "{} {}", input.left, input.right)?;
        }
        out.flush()
    }

    pub fn read(input: impl BufRead) -> io::Result<Self> {
        let mut lines = input.lines();
        let mut next = || lines.next().transpose();
        if next()?.as_deref() != Some(MAGIC) {
            return Err(invalid(format!("missing `{MAGIC}` header")));
        }
        let header = next()?.ok_or_else(|| invalid("missing config line".into()))?;
        let mut cfg = PongConfig::default();
        for field in header.split_whitespace() {
            let (key, value) = field
                .split_once('=')
                .ok_or_else(|| invalid(format!("bad config field `{field}`")))?;
            match key {
                "dt" => cfg.dt = parse(value)?,
                "trail" => cfg.trail = parse(value)?,
                "burst" => cfg.burst = parse(value)?,
                "lifetime" => cfg.particle_lifetime = parse(value)?,
                "seed" => cfg.seed = parse(value)?,
                _ => return Err(invalid(format!("unknown config field `{key}`"))),
            }
        }
        let mut inputs = Vec::new();
        while let Some(line) = next()? {
            let (left, right) = line
                .split_once(' ')
                .ok_or_else(|| invalid(format!("bad input line `{line}`")))?;
            inputs.push(Input {
                left: parse(left)?,
                right: parse(right)?,
            });
        }
        Ok(Self { cfg, inputs })
    }
}

pub fn replay<A: Allocator>(replay: &Replay) -> Outcome {
    let mut game: Pong<A> = Pong::new(&replay.cfg);
    let start = Instant::now();
    for &input in &replay.inputs {
        game.step(input);
    }
    game.finish(start.elapsed())
}

fn parse<T: std::str::FromStr>(value: &str) -> io::Result<T> {
    value
        .parse()
        .map_err(|_| invalid(format!("bad value `{value}`")))
}

fn invalid(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::{Replay, replay};
    use crate::{
        alloc::{arena::ArenaAllocator, heap::HeapAllocator, pool::PoolAllocator},
        pong::{Cpu, CpuConfig, Input, Pong, PongConfig, Side},
    };

    #[test]
    fn recorded_match_replays_identically_on_every_allocator() {
        let cfg = PongConfig {
            trail: 6,
            burst: 5,
            particle_lifetime: 20,
            ..PongConfig::default()
        };
        let mut live = Pong::<HeapAllocator>::new(&cfg);
        let mut cpu = Cpu::new(Side::Right, CpuConfig::default());
        let mut inputs = Vec::new();
        for frame in 0..1_800u32 {
            let input = Input {
                left: [1.0, 0.0, -1.0, 1.0 / 3.0][(frame / 37 % 4) as usize],
                right: cpu.input(live.paddle_y(Side::Right), live.ball()),
            };
            live.step(input);
            inputs.push(input);
        }
        assert!(live.score()[0] + live.score()[1] > 0);

        let mut bytes = Vec::new();
        Replay { cfg, inputs }.write(&mut bytes).unwrap();
        let recorded = Replay::read(bytes.as_slice()).unwrap();
        assert_eq!(recorded.cfg, cfg);

        for outcome in [
            replay::<HeapAllocator>(&recorded),
            replay::<PoolAllocator>(&recorded),
            replay::<ArenaAllocator>(&recorded),
        ] {
            assert_eq!(outcome.score, live.score());
            assert_eq!(outcome.checksum, live.checksum());
        }
    }

    #[test]
    fn replay_rejects_foreign_files() {
        assert!(Replay::read(&b"allocator,count\n1,2\n"[..]).is_err());
        assert!(Replay::read(&b"pong-replay v1\ndt=0.1 seed=1\n0 nope\n"[..]).is_err());
    }
}
