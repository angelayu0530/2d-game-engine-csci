use super::{Action, Frame, Outcome, Pokemon, drive};
use crate::alloc::Allocator;
use std::{
    io::{self, Read, Write},
    time::Instant,
};

pub struct Replay {
    pub initial: String,
    pub actions: Vec<Action>,
    pub checksum: u64,
}
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Invalid or outdated Pokemon recording; record a new session with the current workload",
    )
}
impl Replay {
    pub fn config(&self) -> io::Result<super::Config> {
        let mut words = self.initial.split_whitespace();
        let version = words.next();
        if !matches!(version, Some("POKEMON_SAVE_1" | "POKEMON_SAVE_2")) {
            return Err(invalid());
        }
        let mut next = || words.next().ok_or_else(invalid);
        let cfg = super::Config {
            seed: next()?.parse().map_err(|_| invalid())?,
            burst: next()?.parse().map_err(|_| invalid())?,
            lifetime: next()?.parse().map_err(|_| invalid())?,
            trail: if version == Some("POKEMON_SAVE_2") {
                next()?.parse().map_err(|_| invalid())?
            } else {
                0
            },
            dt: if version == Some("POKEMON_SAVE_2") {
                next()?.parse().map_err(|_| invalid())?
            } else {
                1.0 / 60.0
            },
        };
        if cfg.lifetime == 0 || !cfg.dt.is_finite() || cfg.dt <= 0.0 {
            return Err(invalid());
        }
        Ok(cfg)
    }

    pub fn write(&self, mut out: impl Write) -> io::Result<()> {
        writeln!(
            out,
            "POKEMON_REPLAY_2\n{}\n{}",
            self.checksum,
            self.initial.len()
        )?;
        out.write_all(self.initial.as_bytes())?;
        for action in &self.actions {
            let (code, slot) = match *action {
                Action::Wait => (0, 0),
                Action::Up => (1, 0),
                Action::Down => (2, 0),
                Action::Left => (3, 0),
                Action::Right => (4, 0),
                Action::Attack => (5, 0),
                Action::Catch => (6, 0),
                Action::Run => (7, 0),
                Action::Heal => (8, 0),
                Action::Switch(i) => (9, i),
                Action::Release(i) => (10, i),
            };
            writeln!(out, "{code} {slot}")?;
        }
        out.flush()
    }
    pub fn read(mut input: impl Read) -> io::Result<Self> {
        let mut text = String::new();
        input.read_to_string(&mut text)?;
        let (magic, text) = text.split_once('\n').ok_or_else(invalid)?;
        if magic != "POKEMON_REPLAY_2" {
            return Err(invalid());
        }
        let (checksum, text) = text.split_once('\n').ok_or_else(invalid)?;
        let checksum = checksum.parse().map_err(|_| invalid())?;
        let (size, text) = text.split_once('\n').ok_or_else(invalid)?;
        let size: usize = size.parse().map_err(|_| invalid())?;
        let initial = text.get(..size).ok_or_else(invalid)?.to_owned();
        let mut actions = Vec::new();
        for line in text.get(size..).ok_or_else(invalid)?.lines() {
            let (code, slot) = line.split_once(' ').ok_or_else(invalid)?;
            let slot: usize = slot.parse().map_err(|_| invalid())?;
            let action = match (code, slot) {
                ("0", 0) => Action::Wait,
                ("1", 0) => Action::Up,
                ("2", 0) => Action::Down,
                ("3", 0) => Action::Left,
                ("4", 0) => Action::Right,
                ("5", 0) => Action::Attack,
                ("6", 0) => Action::Catch,
                ("7", 0) => Action::Run,
                ("8", 0) => Action::Heal,
                ("9", i) if i < 5 => Action::Switch(i),
                ("10", i) if i < 5 => Action::Release(i),
                _ => return Err(invalid()),
            };
            actions.push(action);
        }
        if actions.len() > u32::MAX as usize {
            return Err(invalid());
        }
        Ok(Self {
            initial,
            actions,
            checksum,
        })
    }
}
pub fn replay<A: Allocator>(
    recording: &Replay,
    trace: Option<&mut dyn FnMut(Frame)>,
) -> io::Result<Outcome> {
    let setup = Instant::now();
    let game = Pokemon::<A>::from_snapshot(&recording.initial)?;
    let setup = setup.elapsed();
    let outcome = drive(
        game,
        recording.actions.len(),
        |_, i| recording.actions[i],
        setup,
        trace,
    );
    if outcome.checksum != recording.checksum {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Replay checksum does not match recorded gameplay",
        ));
    }
    Ok(outcome)
}
