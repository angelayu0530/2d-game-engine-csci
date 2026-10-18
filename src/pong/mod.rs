mod cpu;
mod render;
mod replay;

use std::time::{Duration, Instant};

pub use cpu::{Cpu, CpuConfig};
pub use render::draw;
pub use replay::{Replay, replay};

use crate::{alloc::Allocator, particle::Particle};

pub const WIDTH: f32 = 1024.0;
pub const HEIGHT: f32 = 768.0;
pub const PADDLE_HALF_H: f32 = 48.0;
pub const BALL_RADIUS: f32 = 8.0;
pub const LEFT_FACE: f32 = 56.0;
pub const RIGHT_FACE: f32 = WIDTH - 56.0;
const PADDLE_SPEED: f32 = 420.0;
const SERVE_VX: f32 = 320.0;
const SERVE_VY: f32 = 210.0;
const SPEED_UP: f32 = 1.08;
const MAX_SPEED: f32 = 840.0;
const MAX_DEFLECT: f32 = 0.75;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PongConfig {
    pub dt: f32,
    pub trail: usize,
    pub burst: usize,
    pub particle_lifetime: u32,
    pub seed: u64,
}

impl Default for PongConfig {
    fn default() -> Self {
        Self {
            dt: 1.0 / 60.0,
            trail: 0,
            burst: 32,
            particle_lifetime: 45,
            seed: 1,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Input {
    pub left: f32,
    pub right: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ball {
    pub pos: [f32; 2],
    pub vel: [f32; 2],
}

#[derive(Debug, Clone, Copy)]
pub struct Outcome {
    pub score: [u32; 2],
    pub checksum: u64,
    pub created: u64,
    pub retired: u64,
    pub particle_updates: u64,
    pub live_particles: usize,
    pub peak_live: usize,
    pub peak_reserved_bytes: usize,
    pub elapsed: Duration,
}

struct Paddle {
    y: f32,
}

pub struct Pong<A: Allocator> {
    alloc: A,
    handles: Vec<A::Handle>,
    dead: Vec<usize>,
    left: Paddle,
    right: Paddle,
    ball: Ball,
    score: [u32; 2],
    frame: u32,
    next_seed: usize,
    created: u64,
    retired: u64,
    particle_updates: u64,
    peak_live: usize,
    peak_reserved: usize,
    cfg: PongConfig,
}

impl<A: Allocator> Pong<A> {
    pub fn new(cfg: &PongConfig) -> Self {
        let hint = cfg
            .trail
            .saturating_mul(cfg.particle_lifetime as usize)
            .saturating_add(cfg.burst);
        let mut game = Self {
            alloc: A::with_capacity(hint),
            handles: Vec::new(),
            dead: Vec::new(),
            left: Paddle { y: HEIGHT * 0.5 },
            right: Paddle { y: HEIGHT * 0.5 },
            ball: Ball {
                pos: [0.0, 0.0],
                vel: [0.0, 0.0],
            },
            score: [0, 0],
            frame: 0,
            next_seed: cfg.seed as usize,
            created: 0,
            retired: 0,
            particle_updates: 0,
            peak_live: 0,
            peak_reserved: 0,
            cfg: *cfg,
        };
        game.serve();
        game
    }

    pub fn step(&mut self, input: Input) {
        self.move_paddle(Side::Left, input.left);
        self.move_paddle(Side::Right, input.right);

        let prev_x = self.ball.pos[0];
        self.ball.pos[0] += self.ball.vel[0] * self.cfg.dt;
        self.ball.pos[1] += self.ball.vel[1] * self.cfg.dt;

        let mut burst_count = self.bounce_walls() + self.bounce_paddles(prev_x);
        let scored_at = self.score_if_out();
        let burst_pos = scored_at.unwrap_or(self.ball.pos);
        if scored_at.is_some() {
            self.clear_particles();
            self.serve();
            burst_count += 1;
        }

        self.age_particles();
        if burst_count > 0 {
            self.spawn(burst_count * self.cfg.burst, burst_pos);
        }
        self.spawn(self.cfg.trail, self.ball.pos);

        self.frame += 1;
        self.peak_live = self.peak_live.max(self.alloc.live_count());
        self.peak_reserved = self.peak_reserved.max(self.alloc.reserved_bytes());
    }

    pub fn score(&self) -> [u32; 2] {
        self.score
    }

    pub fn ball(&self) -> Ball {
        self.ball
    }

    pub fn set_ball(&mut self, ball: Ball) {
        self.ball = ball;
    }

    pub fn paddle_y(&self, side: Side) -> f32 {
        match side {
            Side::Left => self.left.y,
            Side::Right => self.right.y,
        }
    }

    pub fn set_paddle_y(&mut self, side: Side, y: f32) {
        let y = y.clamp(PADDLE_HALF_H, HEIGHT - PADDLE_HALF_H);
        match side {
            Side::Left => self.left.y = y,
            Side::Right => self.right.y = y,
        }
    }

    pub fn config(&self) -> &PongConfig {
        &self.cfg
    }

    pub fn live_particles(&self) -> usize {
        self.alloc.live_count()
    }

    pub fn particles(&self) -> impl Iterator<Item = &Particle> {
        self.handles.iter().map(|&handle| self.alloc.get(handle))
    }

    pub fn checksum(&self) -> u64 {
        let mut hash = mix(0xCBF2_9CE4_8422_2325, self.frame);
        hash = mix(hash, self.score[0]);
        hash = mix(hash, self.score[1]);
        hash = mix_f32(hash, self.ball.pos[0]);
        hash = mix_f32(hash, self.ball.pos[1]);
        hash = mix_f32(hash, self.ball.vel[0]);
        hash = mix_f32(hash, self.ball.vel[1]);
        hash = mix_f32(hash, self.left.y);
        hash = mix_f32(hash, self.right.y);
        for &handle in &self.handles {
            let particle = self.alloc.get(handle);
            hash = mix_f32(hash, particle.pos[0]);
            hash = mix_f32(hash, particle.pos[1]);
            hash = mix_f32(hash, particle.vel[0]);
            hash = mix_f32(hash, particle.vel[1]);
            hash = mix(hash, particle.lifetime);
        }
        hash
    }

    fn finish(&self, elapsed: Duration) -> Outcome {
        Outcome {
            score: self.score,
            checksum: self.checksum(),
            created: self.created,
            retired: self.retired,
            particle_updates: self.particle_updates,
            live_particles: self.alloc.live_count(),
            peak_live: self.peak_live,
            peak_reserved_bytes: self.peak_reserved,
            elapsed,
        }
    }

    fn move_paddle(&mut self, side: Side, axis: f32) {
        let y = match side {
            Side::Left => &mut self.left.y,
            Side::Right => &mut self.right.y,
        };
        *y = (*y + axis.clamp(-1.0, 1.0) * PADDLE_SPEED * self.cfg.dt)
            .clamp(PADDLE_HALF_H, HEIGHT - PADDLE_HALF_H);
    }

    fn bounce_walls(&mut self) -> usize {
        if self.ball.pos[1] < BALL_RADIUS {
            self.ball.pos[1] = BALL_RADIUS;
            self.ball.vel[1] = self.ball.vel[1].abs();
            1
        } else if self.ball.pos[1] > HEIGHT - BALL_RADIUS {
            self.ball.pos[1] = HEIGHT - BALL_RADIUS;
            self.ball.vel[1] = -self.ball.vel[1].abs();
            1
        } else {
            0
        }
    }

    fn bounce_paddles(&mut self, prev_x: f32) -> usize {
        let x = self.ball.pos[0];
        let y = self.ball.pos[1];
        if prev_x >= LEFT_FACE && x < LEFT_FACE && overlaps(y, self.left.y) {
            self.ball.pos[0] = LEFT_FACE;
            self.deflect(1.0, self.left.y);
            1
        } else if prev_x <= RIGHT_FACE && x > RIGHT_FACE && overlaps(y, self.right.y) {
            self.ball.pos[0] = RIGHT_FACE;
            self.deflect(-1.0, self.right.y);
            1
        } else {
            0
        }
    }

    fn deflect(&mut self, dir: f32, paddle_y: f32) {
        let speed = (self.ball.vel[0] * self.ball.vel[0] + self.ball.vel[1] * self.ball.vel[1])
            .sqrt()
            .max(1.0);
        let speed = (speed * SPEED_UP).min(MAX_SPEED);
        let offset =
            ((self.ball.pos[1] - paddle_y) / (PADDLE_HALF_H + BALL_RADIUS)).clamp(-1.0, 1.0);
        let vy = speed * MAX_DEFLECT * offset;
        self.ball.vel = [dir * (speed * speed - vy * vy).sqrt(), vy];
    }

    fn score_if_out(&mut self) -> Option<[f32; 2]> {
        if self.ball.pos[0] < BALL_RADIUS {
            self.score[1] += 1;
            Some(self.ball.pos)
        } else if self.ball.pos[0] > WIDTH - BALL_RADIUS {
            self.score[0] += 1;
            Some(self.ball.pos)
        } else {
            None
        }
    }

    fn serve(&mut self) {
        let dir = if (self.score[0] + self.score[1]).is_multiple_of(2) {
            1.0
        } else {
            -1.0
        };
        self.ball.pos = [WIDTH * 0.5, HEIGHT * 0.5];
        self.ball.vel = [dir * SERVE_VX, SERVE_VY];
    }

    fn clear_particles(&mut self) {
        self.retired += self.handles.len() as u64;
        for handle in self.handles.drain(..) {
            self.alloc.retire(handle);
        }
        self.alloc.reclaim();
        self.dead.clear();
    }

    fn age_particles(&mut self) {
        self.particle_updates += self.handles.len() as u64;
        for (index, &handle) in self.handles.iter().enumerate() {
            let particle = self.alloc.get_mut(handle);
            particle.update(self.cfg.dt);
            if particle.lifetime == 0 {
                self.dead.push(index);
            }
        }
        self.retired += self.dead.len() as u64;
        for &index in self.dead.iter().rev() {
            self.alloc.retire(self.handles.swap_remove(index));
        }
        self.dead.clear();
    }

    fn spawn(&mut self, count: usize, origin: [f32; 2]) {
        for _ in 0..count {
            let particle = Particle::at(self.next_seed, origin, self.cfg.particle_lifetime);
            self.next_seed += 1;
            self.handles.push(self.alloc.create(particle));
        }
        self.created += count as u64;
    }
}

pub fn run<A: Allocator>(cfg: &PongConfig, cpu: &CpuConfig, frames: u32) -> Outcome {
    let mut game: Pong<A> = Pong::new(cfg);
    let mut left = Cpu::new(Side::Left, *cpu);
    let mut right = Cpu::new(Side::Right, *cpu);
    let start = Instant::now();
    for _ in 0..frames {
        let input = Input {
            left: left.input(game.paddle_y(Side::Left), game.ball()),
            right: right.input(game.paddle_y(Side::Right), game.ball()),
        };
        game.step(input);
    }
    game.finish(start.elapsed())
}

fn overlaps(ball_y: f32, paddle_y: f32) -> bool {
    (ball_y - paddle_y).abs() <= PADDLE_HALF_H + BALL_RADIUS
}

fn mix(hash: u64, word: u32) -> u64 {
    (hash ^ u64::from(word)).wrapping_mul(0x0100_0000_01B3)
}

fn mix_f32(hash: u64, value: f32) -> u64 {
    mix(hash, value.to_bits())
}

#[cfg(test)]
mod tests {
    use super::{
        BALL_RADIUS, Ball, CpuConfig, HEIGHT, Input, LEFT_FACE, PADDLE_HALF_H, Pong, PongConfig,
        Side, WIDTH, run,
    };
    use crate::alloc::{
        Allocator, arena::ArenaAllocator, heap::HeapAllocator, pool::PoolAllocator,
    };

    fn quiet() -> PongConfig {
        PongConfig {
            trail: 0,
            burst: 4,
            particle_lifetime: 5,
            ..PongConfig::default()
        }
    }

    #[test]
    fn input_moves_one_paddle() {
        let mut game = Pong::<HeapAllocator>::new(&quiet());
        let start = game.paddle_y(Side::Left);
        game.step(Input {
            left: 1.0,
            right: 0.0,
        });
        assert!(game.paddle_y(Side::Left) > start);
        assert_eq!(game.paddle_y(Side::Right), start);
    }

    #[test]
    fn ball_bounces_off_the_top_wall() {
        let mut game = Pong::<PoolAllocator>::new(&quiet());
        game.set_ball(Ball {
            pos: [WIDTH / 2.0, BALL_RADIUS + 1.0],
            vel: [0.0, -200.0],
        });
        game.cfg.dt = 0.1;
        game.step(Input::default());
        assert_eq!(game.ball().pos[1], BALL_RADIUS);
        assert!(game.ball().vel[1] > 0.0);
        assert_eq!(game.score(), [0, 0]);
        assert_eq!(game.live_particles(), quiet().burst);
    }

    #[test]
    fn ball_bounces_off_the_left_paddle() {
        let mut game = Pong::<ArenaAllocator>::new(&quiet());
        game.set_paddle_y(Side::Left, HEIGHT / 2.0);
        game.set_ball(Ball {
            pos: [LEFT_FACE + 10.0, HEIGHT / 2.0],
            vel: [-200.0, 20.0],
        });
        game.cfg.dt = 0.1;
        game.step(Input::default());
        assert_eq!(game.ball().pos[0], LEFT_FACE);
        assert!(game.ball().vel[0] > 0.0);
        assert_eq!(game.score(), [0, 0]);
        assert_eq!(game.live_particles(), quiet().burst);
    }

    #[test]
    fn missed_ball_scores_and_reclaims() {
        for run_one in [
            missed_ball_scores_and_reclaims_on::<HeapAllocator>,
            missed_ball_scores_and_reclaims_on::<PoolAllocator>,
            missed_ball_scores_and_reclaims_on::<ArenaAllocator>,
        ] {
            run_one();
        }
    }

    fn missed_ball_scores_and_reclaims_on<A: Allocator>() {
        let mut cfg = quiet();
        cfg.trail = 5;
        cfg.burst = 4;
        let mut game: Pong<A> = Pong::new(&cfg);
        game.step(Input::default());
        assert_eq!(game.live_particles(), 5, "{}", A::NAME);

        game.cfg.trail = 0;
        game.cfg.dt = 1.0;
        game.set_paddle_y(Side::Right, PADDLE_HALF_H);
        game.set_ball(Ball {
            pos: [980.0, 700.0],
            vel: [400.0, 0.0],
        });
        game.step(Input::default());

        assert_eq!(game.score(), [1, 0], "{}", A::NAME);
        assert_eq!(game.ball().pos, [WIDTH / 2.0, HEIGHT / 2.0], "{}", A::NAME);
        assert!(game.ball().vel[0] < 0.0, "{}", A::NAME);
        assert_eq!(game.live_particles(), 4, "{}", A::NAME);
        assert_eq!(game.retired, 5, "{}", A::NAME);
    }

    #[test]
    fn trail_particles_die_and_are_replaced() {
        let cfg = PongConfig {
            trail: 10,
            burst: 0,
            particle_lifetime: 1,
            ..quiet()
        };
        let mut game = Pong::<PoolAllocator>::new(&cfg);
        game.set_ball(Ball {
            pos: [WIDTH / 2.0, HEIGHT / 2.0],
            vel: [0.0, 0.0],
        });
        game.step(Input::default());
        assert_eq!(game.live_particles(), 10);
        game.step(Input::default());
        assert_eq!(game.live_particles(), 10);
        assert_eq!(game.created, 20);
        assert_eq!(game.retired, 10);
    }

    #[test]
    fn allocators_play_the_same_match() {
        let cfg = PongConfig {
            trail: 8,
            burst: 6,
            particle_lifetime: 12,
            ..PongConfig::default()
        };
        let cpu = CpuConfig::default();
        let heap = run::<HeapAllocator>(&cfg, &cpu, 3_600);
        let pool = run::<PoolAllocator>(&cfg, &cpu, 3_600);
        let arena = run::<ArenaAllocator>(&cfg, &cpu, 3_600);

        assert!(heap.score[0] + heap.score[1] > 0, "{heap:?}");
        assert_eq!(pool.score, heap.score);
        assert_eq!(arena.score, heap.score);
        assert_eq!(pool.checksum, heap.checksum);
        assert_eq!(arena.checksum, heap.checksum);
        assert_eq!(pool.created, heap.created);
        assert_eq!(arena.created, heap.created);
        assert_eq!(pool.retired, heap.retired);
        assert_eq!(arena.retired, heap.retired);
        assert_eq!(pool.particle_updates, heap.particle_updates);
        assert!(arena.peak_reserved_bytes > pool.peak_reserved_bytes);
        assert_eq!(
            run::<HeapAllocator>(&cfg, &cpu, 3_600).checksum,
            heap.checksum
        );
    }

    #[test]
    fn cpu_variance_makes_the_computer_miss_more() {
        let cfg = PongConfig {
            trail: 0,
            burst: 0,
            ..PongConfig::default()
        };
        let points = |variance| {
            let cpu = CpuConfig {
                variance,
                ..CpuConfig::default()
            };
            let score = run::<PoolAllocator>(&cfg, &cpu, 18_000).score;
            score[0] + score[1]
        };
        let exact = points(0.0);
        let sloppy = points(0.5);
        assert!(sloppy > exact * 3 / 2, "exact {exact}, sloppy {sloppy}");
    }

    #[test]
    fn paddle_hit_offset_sets_return_angle() {
        let mut returns = Vec::new();
        for offset in [-40.0, 0.0, 40.0] {
            let mut game = Pong::<HeapAllocator>::new(&PongConfig::default());
            game.set_paddle_y(Side::Left, HEIGHT / 2.0);
            game.set_ball(Ball {
                pos: [LEFT_FACE + 2.0, HEIGHT / 2.0 + offset],
                vel: [-300.0, 0.0],
            });
            game.step(Input::default());
            assert!(game.ball().vel[0] > 0.0, "offset {offset}");
            returns.push(game.ball().vel[1]);
        }
        assert!(returns[0] < 0.0, "{returns:?}");
        assert_eq!(returns[1], 0.0, "{returns:?}");
        assert_eq!(returns[2], -returns[0], "{returns:?}");
    }
}
