use std::{
    io::{self, Write},
    time::{Duration, Instant},
};

use super::{Fight, Input};
use crate::alloc::Allocator;

pub const TRACE_HEADER: &str =
    "mode,allocator,particles,trial,frame,step_ns,live,reserved_bytes,knight_hp,boss_hp";
const MODE: &str = "fight-replay";

/// One step's cost and state, for per-frame charts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub index: u32,
    pub step: Duration,
    pub live: usize,
    pub reserved_bytes: usize,
    pub knight_hp: u32,
    pub boss_hp: u32,
}

impl Frame {
    pub fn write_csv(
        &self,
        out: &mut impl Write,
        allocator: &str,
        particles: usize,
        trial: u32,
    ) -> io::Result<()> {
        writeln!(
            out,
            "{MODE},{allocator},{particles},{trial},{},{},{},{},{},{}",
            self.index,
            self.step.as_nanos(),
            self.live,
            self.reserved_bytes,
            self.knight_hp,
            self.boss_hp,
        )
    }
}

impl<A: Allocator> Fight<A> {
    pub fn timed_step(&mut self, input: Input) -> Frame {
        let index = self.frame;
        let start = Instant::now();
        self.step(input);
        let step = start.elapsed();
        Frame {
            index,
            step,
            live: self.effects.live(),
            reserved_bytes: self.effects.reserved_bytes(),
            knight_hp: self.knight.hp,
            boss_hp: self.boss.hp,
        }
    }
}
