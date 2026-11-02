use std::io::{self, BufRead, Write};

use super::{FightConfig, Input};

/// v2: the knight shoots arrows instead of swinging a sword.
const MAGIC: &str = "fight-replay v2";

/// A recorded fight: the config it was played with, one input per step
/// (run-length encoded on disk), and the gameplay checksum it ended on.
#[derive(Debug, Clone, PartialEq)]
pub struct Replay {
    pub cfg: FightConfig,
    pub inputs: Vec<Input>,
    pub game_checksum: u64,
}

impl Replay {
    pub fn write(&self, mut out: impl Write) -> io::Result<()> {
        let cfg = &self.cfg;
        writeln!(out, "{MAGIC}")?;
        writeln!(
            out,
            "dt={} particles={} seed={} game={}",
            cfg.dt, cfg.particles, cfg.seed, self.game_checksum
        )?;
        let mut runs = self.inputs.chunk_by(|a, b| a == b);
        runs.try_for_each(|run| writeln!(out, "{:02x} {}", run[0].bits(), run.len()))?;
        out.flush()
    }

    pub fn read(input: impl BufRead) -> io::Result<Self> {
        let mut lines = input.lines();
        let mut next = || lines.next().transpose();
        if next()?.as_deref() != Some(MAGIC) {
            return Err(invalid(format!("missing `{MAGIC}` header")));
        }
        let header = next()?.ok_or_else(|| invalid("missing config line".into()))?;
        let mut cfg = FightConfig::default();
        let mut game_checksum = None;
        for field in header.split_whitespace() {
            let (key, value) = field
                .split_once('=')
                .ok_or_else(|| invalid(format!("bad config field `{field}`")))?;
            match key {
                "dt" => cfg.dt = parse(value)?,
                "particles" => cfg.particles = parse(value)?,
                "seed" => cfg.seed = parse(value)?,
                "game" => game_checksum = Some(parse(value)?),
                _ => return Err(invalid(format!("unknown config field `{key}`"))),
            }
        }
        let game_checksum = game_checksum.ok_or_else(|| invalid("missing `game=`".into()))?;
        let mut inputs = Vec::new();
        while let Some(line) = next()? {
            let (bits, count) = line
                .split_once(' ')
                .ok_or_else(|| invalid(format!("bad input line `{line}`")))?;
            let bits = u8::from_str_radix(bits, 16)
                .map_err(|_| invalid(format!("bad input bits `{bits}`")))?;
            let count: usize = parse(count)?;
            inputs.extend(std::iter::repeat_n(Input::from_bits(bits), count));
        }
        Ok(Self {
            cfg,
            inputs,
            game_checksum,
        })
    }
}

fn parse<T: std::str::FromStr>(value: &str) -> io::Result<T> {
    value
        .parse()
        .map_err(|_| invalid(format!("bad value `{value}`")))
}

fn invalid(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
