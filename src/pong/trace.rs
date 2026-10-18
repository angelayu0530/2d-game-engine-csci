use std::{
    io::{self, Write},
    time::{Duration, Instant},
};

use super::{Input, Pong};
use crate::alloc::Allocator;

pub const TRACE_HEADER: &str =
    "mode,allocator,trial,frame,step_ns,live,reserved_bytes,score_left,score_right";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceMode {
    CpuVsCpu,
    HumanReplay,
    HumanLive,
}

impl TraceMode {
    pub fn label(self) -> &'static str {
        match self {
            TraceMode::CpuVsCpu => "cpu-vs-cpu",
            TraceMode::HumanReplay => "human-replay",
            TraceMode::HumanLive => "human-live",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub index: u32,
    pub step: Duration,
    pub live: usize,
    pub reserved_bytes: usize,
    pub score: [u32; 2],
}

impl Frame {
    pub fn write_csv(
        &self,
        out: &mut impl Write,
        mode: TraceMode,
        allocator: &str,
        trial: u32,
    ) -> io::Result<()> {
        writeln!(
            out,
            "{},{allocator},{trial},{},{},{},{},{},{}",
            mode.label(),
            self.index,
            self.step.as_nanos(),
            self.live,
            self.reserved_bytes,
            self.score[0],
            self.score[1],
        )
    }
}

impl<A: Allocator> Pong<A> {
    pub fn timed_step(&mut self, input: Input) -> Frame {
        let index = self.frame;
        let start = Instant::now();
        self.step(input);
        let step = start.elapsed();
        Frame {
            index,
            step,
            live: self.alloc.live_count(),
            reserved_bytes: self.alloc.reserved_bytes(),
            score: self.score,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        alloc::{Allocator, arena::ArenaAllocator, heap::HeapAllocator, pool::PoolAllocator},
        pong::{CpuConfig, Frame, PongConfig, Replay, replay, run},
    };

    fn replay_trace<A: Allocator>(recorded: &Replay) -> Vec<(u32, usize, [u32; 2])> {
        let mut frames = Vec::new();
        replay::<A>(
            recorded,
            Some(&mut |f: Frame| frames.push((f.index, f.live, f.score))),
        );
        frames
    }

    #[test]
    fn traced_run_matches_untraced_run_frame_for_frame() {
        let cfg = PongConfig {
            trail: 8,
            ..PongConfig::default()
        };
        let cpu = CpuConfig::default();
        let mut frames: Vec<Frame> = Vec::new();
        let traced = run::<PoolAllocator>(&cfg, &cpu, 3_600, Some(&mut |f| frames.push(f)));
        let plain = run::<PoolAllocator>(&cfg, &cpu, 3_600, None);

        assert_eq!(traced.checksum, plain.checksum);
        assert_eq!(frames.len(), 3_600);
        assert!(frames.iter().enumerate().all(|(i, f)| f.index == i as u32));
        let last = frames.last().unwrap();
        assert_eq!(last.score, plain.score);
        assert_eq!(last.live, plain.live_particles);
        assert_eq!(frames.iter().map(|f| f.live).max(), Some(plain.peak_live));
        assert_eq!(
            frames.iter().map(|f| f.reserved_bytes).max(),
            Some(plain.peak_reserved_bytes)
        );
    }

    #[test]
    fn replay_traces_agree_across_allocators_except_memory_and_time() {
        let cfg = PongConfig {
            trail: 4,
            ..PongConfig::default()
        };
        let inputs = (0..1_200)
            .map(|i| crate::pong::Input {
                left: [1.0, -1.0, 0.0][i / 50 % 3],
                right: [-1.0, 0.0, 1.0][i / 70 % 3],
            })
            .collect();
        let recorded = Replay { cfg, inputs };
        let heap = replay_trace::<HeapAllocator>(&recorded);
        assert_eq!(heap.len(), 1_200);
        assert_eq!(replay_trace::<PoolAllocator>(&recorded), heap);
        assert_eq!(replay_trace::<ArenaAllocator>(&recorded), heap);
    }

    #[test]
    fn csv_row_matches_header_columns() {
        let frame = Frame {
            index: 7,
            step: std::time::Duration::from_nanos(1_234),
            live: 56,
            reserved_bytes: 4_096,
            score: [2, 3],
        };
        let mut row = Vec::new();
        frame
            .write_csv(&mut row, super::TraceMode::HumanReplay, "pool", 1)
            .unwrap();
        let row = String::from_utf8(row).unwrap();
        assert_eq!(row, "human-replay,pool,1,7,1234,56,4096,2,3\n");
        assert_eq!(
            row.trim_end().split(',').count(),
            super::TRACE_HEADER.split(',').count()
        );
    }
}
