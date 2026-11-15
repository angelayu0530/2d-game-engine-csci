use std::{
    io::{self, Write},
    time::{Duration, Instant},
};

use super::{Fight, Input};
use crate::{alloc::Allocator, memory};

pub const TRACE_HEADER: &str = "mode,allocator,particles,trial,frame,step_ns,live,reserved_bytes,\
                                knight_hp,boss_hp,created,retired,minor_faults,rss_bytes,draw_ns";
const MODE: &str = "fight-replay";

/// One step's cost and state, for per-frame charts. `created`, `retired`, and
/// `minor_faults` count this step only; `rss_bytes` is read after it, outside
/// its timing. `draw` is the time to draw the step afterwards, zero unless the
/// caller draws and times it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub index: u32,
    pub step: Duration,
    pub live: usize,
    pub reserved_bytes: usize,
    pub knight_hp: u32,
    pub boss_hp: u32,
    pub created: u64,
    pub retired: u64,
    pub minor_faults: u64,
    pub rss_bytes: usize,
    pub draw: Duration,
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
            "{MODE},{allocator},{particles},{trial},{},{},{},{},{},{},{},{},{},{},{}",
            self.index,
            self.step.as_nanos(),
            self.live,
            self.reserved_bytes,
            self.knight_hp,
            self.boss_hp,
            self.created,
            self.retired,
            self.minor_faults,
            self.rss_bytes,
            self.draw.as_nanos(),
        )
    }
}

impl<A: Allocator> Fight<A> {
    pub fn timed_step(&mut self, input: Input) -> Frame {
        let index = self.frame;
        let (created, retired) = (self.effects.created(), self.effects.retired());
        let faults = memory::minor_faults();
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
            created: self.effects.created() - created,
            retired: self.effects.retired() - retired,
            minor_faults: memory::minor_faults() - faults,
            rss_bytes: memory::current_rss_bytes(),
            draw: Duration::ZERO,
        }
    }
}
