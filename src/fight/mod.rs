//! A one-room boss fight. A knight with a bow faces a demon with
//! [`BOSS_HEARTS`] hearts. Gameplay state lives in ordinary fields and never
//! reads particles, so a recorded fight plays out identically at every
//! particle load; [`FightConfig::particles`] only scales the effects, which
//! are allocated through the allocator under test.

mod effects;
pub mod render;
mod replay;
mod trace;

use std::{
    f32::consts::{FRAC_1_SQRT_2, FRAC_PI_2, PI, TAU},
    time::{Duration, Instant},
};

pub use effects::{Burst, Effects, Kind, MAX_LIFE};
pub use replay::Replay;
pub use trace::{Frame, TRACE_HEADER};

use crate::{
    alloc::Allocator,
    particle::{splitmix64, unit},
};

pub const WIDTH: f32 = 480.0;
pub const HEIGHT: f32 = 320.0;
/// Walkable floor: `[left, top, right, bottom]`.
pub const ROOM: [f32; 4] = [16.0, 48.0, 464.0, 304.0];

pub const KNIGHT_HEARTS: u32 = 6;
pub const POTIONS: u32 = 4;
pub const BOSS_HEARTS: u32 = 40;
pub const KNIGHT_RADIUS: f32 = 6.0;
pub const BOSS_RADIUS: f32 = 18.0;
pub const BOLT_RADIUS: f32 = 3.0;
pub const ARROW_RADIUS: f32 = 2.0;
/// Steps between shots while Space is held.
pub const SHOT_COOLDOWN: u32 = 24;
pub const HEAL_FRAMES: u32 = 30;
pub const TELEGRAPH_FRAMES: u32 = 40;

const KNIGHT_SPEED: f32 = 110.0;
const KNIGHT_START: [f32; 2] = [96.0, 176.0];
const BOSS_START: [f32; 2] = [384.0, 176.0];
const ARROW_SPEED: f32 = 300.0;
const HEAL_AMOUNT: u32 = 2;
const INVULNERABLE_FRAMES: u32 = 60;
const BOSS_FLASH_FRAMES: u32 = 8;
const DASH_SPEED: f32 = 280.0;
const DASH_FRAMES: u32 = 45;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FightConfig {
    pub dt: f32,
    /// Target live particle count; embers top the room up to this every step.
    pub particles: usize,
    pub seed: u64,
}

impl Default for FightConfig {
    fn default() -> Self {
        Self {
            dt: 1.0 / 60.0,
            particles: 10_000,
            seed: 1,
        }
    }
}

/// One step of player input, already resolved to held/pressed buttons.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Input {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    /// Held: shoot an arrow whenever the bow is ready.
    pub attack: bool,
    /// Pressed this step: drink a potion.
    pub heal: bool,
    /// Held: throw a stream of particles where the knight faces.
    pub spray: bool,
    /// Pressed this step: start a new fight once this one is over.
    pub restart: bool,
}

impl Input {
    pub fn bits(self) -> u8 {
        [
            self.up,
            self.down,
            self.left,
            self.right,
            self.attack,
            self.heal,
            self.spray,
            self.restart,
        ]
        .iter()
        .enumerate()
        .fold(0, |bits, (i, &on)| bits | u8::from(on) << i)
    }

    pub fn from_bits(bits: u8) -> Self {
        let on = |i: u8| bits & 1 << i != 0;
        Self {
            up: on(0),
            down: on(1),
            left: on(2),
            right: on(3),
            attack: on(4),
            heal: on(5),
            spray: on(6),
            restart: on(7),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Fighting,
    Victory,
    Defeat,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Knight {
    pub pos: [f32; 2],
    pub facing: [f32; 2],
    pub hp: u32,
    pub potions: u32,
    /// Steps until the bow can shoot again.
    pub cooldown: u32,
    pub healing: u32,
    pub invulnerable: u32,
}

impl Knight {
    fn new() -> Self {
        Self {
            pos: KNIGHT_START,
            facing: [1.0, 0.0],
            hp: KNIGHT_HEARTS,
            potions: POTIONS,
            cooldown: 0,
            healing: 0,
            invulnerable: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BossMove {
    Walk,
    Telegraph { dir: [f32; 2] },
    Dash { dir: [f32; 2] },
    Spiral { angle: f32 },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Boss {
    pub pos: [f32; 2],
    pub hp: u32,
    pub flash: u32,
    pub action: BossMove,
    timer: u32,
}

impl Boss {
    fn new() -> Self {
        Self {
            pos: BOSS_START,
            hp: BOSS_HEARTS,
            flash: 0,
            action: BossMove::Walk,
            timer: 120,
        }
    }

    /// 1 above two thirds health, 2 above one third, else 3.
    pub fn phase(&self) -> u32 {
        if self.hp * 3 > BOSS_HEARTS * 2 {
            1
        } else if self.hp * 3 > BOSS_HEARTS {
            2
        } else {
            3
        }
    }
}

/// A boss bolt or a knight's arrow.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Projectile {
    pub pos: [f32; 2],
    pub vel: [f32; 2],
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Event {
    Shot { pos: [f32; 2], dir: [f32; 2] },
    BossHit([f32; 2]),
    KnightHit([f32; 2]),
    Heal([f32; 2]),
    BossDeath([f32; 2]),
    KnightDeath([f32; 2]),
}

#[derive(Debug, Clone, Copy)]
pub struct Outcome {
    pub frames: usize,
    pub knight_hp: u32,
    pub boss_hp: u32,
    pub victories: u32,
    pub defeats: u32,
    pub game_checksum: u64,
    pub checksum: u64,
    pub created: u64,
    pub retired: u64,
    pub particle_updates: u64,
    pub live: usize,
    pub peak_live: usize,
    pub peak_reserved_bytes: usize,
    pub setup: Duration,
    pub sim: Duration,
    pub teardown: Duration,
}

pub struct Fight<A: Allocator> {
    cfg: FightConfig,
    knight: Knight,
    boss: Boss,
    bolts: Vec<Projectile>,
    arrows: Vec<Projectile>,
    status: Status,
    rng: u64,
    events: Vec<Event>,
    effects: Effects<A>,
    frame: u32,
    victories: u32,
    defeats: u32,
    peak_live: usize,
    peak_reserved: usize,
}

impl<A: Allocator> Fight<A> {
    pub fn new(cfg: &FightConfig) -> Self {
        let mut fight = Self {
            cfg: *cfg,
            knight: Knight::new(),
            boss: Boss::new(),
            bolts: Vec::new(),
            arrows: Vec::new(),
            status: Status::Fighting,
            rng: cfg.seed,
            events: Vec::new(),
            effects: Effects::new(cfg.seed),
            frame: 0,
            victories: 0,
            defeats: 0,
            peak_live: 0,
            peak_reserved: 0,
        };
        fight.effects.embers(cfg.particles, ROOM, [1, MAX_LIFE]);
        fight.track_peaks();
        fight
    }

    pub fn step(&mut self, input: Input) {
        self.play(input);
        self.effects.age(self.cfg.dt);
        self.emit(input);
        self.frame += 1;
        self.track_peaks();
    }

    pub fn config(&self) -> &FightConfig {
        &self.cfg
    }

    pub fn knight(&self) -> &Knight {
        &self.knight
    }

    pub fn boss(&self) -> &Boss {
        &self.boss
    }

    pub fn bolts(&self) -> &[Projectile] {
        &self.bolts
    }

    pub fn arrows(&self) -> &[Projectile] {
        &self.arrows
    }

    pub fn status(&self) -> Status {
        self.status
    }

    pub fn frame(&self) -> u32 {
        self.frame
    }

    pub fn effects(&self) -> &Effects<A> {
        &self.effects
    }

    pub fn live_particles(&self) -> usize {
        self.effects.live()
    }

    /// Hash of gameplay state only; identical at every particle load.
    pub fn game_checksum(&self) -> u64 {
        let k = &self.knight;
        let b = &self.boss;
        let mut hash = mix(0xCBF2_9CE4_8422_2325, self.frame);
        for word in [
            self.status as u32,
            self.victories,
            self.defeats,
            k.hp,
            k.potions,
            k.cooldown,
            k.healing,
            k.invulnerable,
            b.hp,
            b.flash,
            b.timer,
            self.rng as u32,
            (self.rng >> 32) as u32,
        ] {
            hash = mix(hash, word);
        }
        for value in [k.pos, k.facing, b.pos].into_iter().flatten() {
            hash = mix(hash, value.to_bits());
        }
        for bolt in self.bolts.iter().chain(&self.arrows) {
            for value in [bolt.pos, bolt.vel].into_iter().flatten() {
                hash = mix(hash, value.to_bits());
            }
        }
        hash
    }

    /// Gameplay plus every live particle.
    pub fn checksum(&self) -> u64 {
        self.effects.checksum(self.game_checksum())
    }

    fn track_peaks(&mut self) {
        self.peak_live = self.peak_live.max(self.effects.live());
        self.peak_reserved = self.peak_reserved.max(self.effects.reserved_bytes());
    }

    fn play(&mut self, input: Input) {
        if self.status != Status::Fighting {
            if input.restart {
                self.knight = Knight::new();
                self.boss = Boss::new();
                self.bolts.clear();
                self.arrows.clear();
                self.status = Status::Fighting;
            }
            return;
        }
        self.move_knight(input);
        self.shoot(input);
        self.drink(input);
        if self.status == Status::Fighting {
            self.boss_turn();
        }
        if self.status == Status::Fighting {
            self.move_arrows();
        }
        if self.status == Status::Fighting {
            self.move_bolts();
        }
        if distance(self.knight.pos, self.boss.pos) < KNIGHT_RADIUS + BOSS_RADIUS {
            self.hurt_knight();
        }
        let k = &mut self.knight;
        k.cooldown = k.cooldown.saturating_sub(1);
        k.healing = k.healing.saturating_sub(1);
        k.invulnerable = k.invulnerable.saturating_sub(1);
        self.boss.flash = self.boss.flash.saturating_sub(1);
    }

    fn move_knight(&mut self, input: Input) {
        let x = f32::from(u8::from(input.right)) - f32::from(u8::from(input.left));
        let y = f32::from(u8::from(input.down)) - f32::from(u8::from(input.up));
        if x == 0.0 && y == 0.0 {
            return;
        }
        let norm = if x != 0.0 && y != 0.0 {
            FRAC_1_SQRT_2
        } else {
            1.0
        };
        let dir = [x * norm, y * norm];
        let k = &mut self.knight;
        k.facing = dir;
        k.pos = clamp_to_room(
            [
                k.pos[0] + dir[0] * KNIGHT_SPEED * self.cfg.dt,
                k.pos[1] + dir[1] * KNIGHT_SPEED * self.cfg.dt,
            ],
            KNIGHT_RADIUS,
        );
    }

    fn shoot(&mut self, input: Input) {
        let k = &mut self.knight;
        if !input.attack || k.cooldown > 0 || k.healing > 0 {
            return;
        }
        k.cooldown = SHOT_COOLDOWN;
        let pos = [
            k.pos[0] + k.facing[0] * KNIGHT_RADIUS,
            k.pos[1] + k.facing[1] * KNIGHT_RADIUS,
        ];
        self.arrows.push(Projectile {
            pos,
            vel: [k.facing[0] * ARROW_SPEED, k.facing[1] * ARROW_SPEED],
        });
        self.events.push(Event::Shot { pos, dir: k.facing });
    }

    /// Flies arrows; each one that reaches the boss costs it a heart.
    fn move_arrows(&mut self) {
        let dt = self.cfg.dt;
        let [left, top, right, bottom] = ROOM;
        let boss = self.boss.pos;
        let mut hits = Vec::new();
        self.arrows.retain_mut(|arrow| {
            arrow.pos[0] += arrow.vel[0] * dt;
            arrow.pos[1] += arrow.vel[1] * dt;
            let [x, y] = arrow.pos;
            if x < left || x > right || y < top || y > bottom {
                return false;
            }
            if distance(arrow.pos, boss) < ARROW_RADIUS + BOSS_RADIUS {
                hits.push(arrow.pos);
                return false;
            }
            true
        });
        for pos in hits {
            if self.boss.hp == 0 {
                break;
            }
            self.boss.hp -= 1;
            self.boss.flash = BOSS_FLASH_FRAMES;
            self.events.push(Event::BossHit(pos));
            if self.boss.hp == 0 {
                self.status = Status::Victory;
                self.victories += 1;
                self.bolts.clear();
                self.arrows.clear();
                self.events.push(Event::BossDeath(self.boss.pos));
            }
        }
    }

    fn drink(&mut self, input: Input) {
        let k = &mut self.knight;
        if input.heal && k.potions > 0 && k.hp < KNIGHT_HEARTS && k.healing == 0 {
            k.potions -= 1;
            k.hp = (k.hp + HEAL_AMOUNT).min(KNIGHT_HEARTS);
            k.healing = HEAL_FRAMES;
            self.events.push(Event::Heal(k.pos));
        }
    }

    fn hurt_knight(&mut self) {
        let k = &mut self.knight;
        if k.invulnerable > 0 || self.status != Status::Fighting {
            return;
        }
        k.hp -= 1;
        k.invulnerable = INVULNERABLE_FRAMES;
        self.events.push(Event::KnightHit(k.pos));
        if k.hp == 0 {
            self.status = Status::Defeat;
            self.defeats += 1;
            self.bolts.clear();
            self.arrows.clear();
            self.events.push(Event::KnightDeath(k.pos));
        }
    }

    fn boss_turn(&mut self) {
        let dt = self.cfg.dt;
        let phase = self.boss.phase();
        let to_knight = direction(self.boss.pos, self.knight.pos);
        self.boss.timer = self.boss.timer.saturating_sub(1);
        match self.boss.action {
            BossMove::Walk => {
                let speed = [38.0, 46.0, 58.0][phase as usize - 1];
                self.boss.pos = clamp_to_room(
                    [
                        self.boss.pos[0] + to_knight[0] * speed * dt,
                        self.boss.pos[1] + to_knight[1] * speed * dt,
                    ],
                    BOSS_RADIUS,
                );
                if self.boss.timer == 0 {
                    self.attack(phase, to_knight);
                }
            }
            BossMove::Telegraph { dir } => {
                if self.boss.timer == 0 {
                    self.boss.action = BossMove::Dash { dir };
                    self.boss.timer = DASH_FRAMES;
                }
            }
            BossMove::Dash { dir } => {
                let next = [
                    self.boss.pos[0] + dir[0] * DASH_SPEED * dt,
                    self.boss.pos[1] + dir[1] * DASH_SPEED * dt,
                ];
                self.boss.pos = clamp_to_room(next, BOSS_RADIUS);
                if self.boss.pos != next || self.boss.timer == 0 {
                    self.rest(phase);
                }
            }
            BossMove::Spiral { angle } => {
                if self.boss.timer.is_multiple_of(4) {
                    for arm in [0.0, PI] {
                        self.fire(angle + arm, 95.0);
                    }
                }
                self.boss.action = BossMove::Spiral {
                    angle: angle + 0.09,
                };
                if self.boss.timer == 0 {
                    self.rest(phase);
                }
            }
        }
    }

    fn attack(&mut self, phase: u32, to_knight: [f32; 2]) {
        let aim = to_knight[1].atan2(to_knight[0]);
        match self.roll(phase + 1) {
            0 => {
                let count = [12, 16, 20][phase as usize - 1];
                let speed = [85.0, 100.0, 115.0][phase as usize - 1];
                let offset = unit(self.next_random() as u32) * TAU;
                for i in 0..count {
                    self.fire(offset + TAU * i as f32 / count as f32, speed);
                }
                self.rest(phase);
            }
            1 => {
                for i in -2..=2 {
                    self.fire(aim + 0.22 * i as f32, 150.0);
                }
                self.rest(phase);
            }
            2 => {
                self.boss.action = BossMove::Telegraph { dir: to_knight };
                self.boss.timer = TELEGRAPH_FRAMES;
            }
            _ => {
                self.boss.action = BossMove::Spiral { angle: aim };
                self.boss.timer = 160;
            }
        }
    }

    fn rest(&mut self, phase: u32) {
        let [lo, hi] = [[90, 150], [70, 120], [45, 90]][phase as usize - 1];
        self.boss.action = BossMove::Walk;
        self.boss.timer = lo + self.roll(hi - lo + 1);
    }

    fn fire(&mut self, angle: f32, speed: f32) {
        self.bolts.push(Projectile {
            pos: self.boss.pos,
            vel: [angle.cos() * speed, angle.sin() * speed],
        });
    }

    fn move_bolts(&mut self) {
        let dt = self.cfg.dt;
        let [left, top, right, bottom] = ROOM;
        let mut hit = false;
        let knight = self.knight;
        self.bolts.retain_mut(|bolt| {
            bolt.pos[0] += bolt.vel[0] * dt;
            bolt.pos[1] += bolt.vel[1] * dt;
            let [x, y] = bolt.pos;
            if x < left || x > right || y < top || y > bottom {
                return false;
            }
            if knight.invulnerable == 0
                && !hit
                && distance(bolt.pos, knight.pos) < BOLT_RADIUS + KNIGHT_RADIUS
            {
                hit = true;
                return false;
            }
            true
        });
        if hit {
            self.hurt_knight();
        }
    }

    fn roll(&mut self, sides: u32) -> u32 {
        (self.next_random() % u64::from(sides)) as u32
    }

    fn next_random(&mut self) -> u64 {
        self.rng = splitmix64(self.rng);
        self.rng
    }

    /// Spawns this step's effects. Counts scale with the particle target so a
    /// swing at 10M particles throws 100k sparks, not 24.
    fn emit(&mut self, input: Input) {
        let n = self.cfg.particles;
        for event in self.events.drain(..) {
            let burst = match event {
                Event::Shot { pos, dir } => Burst {
                    kind: Kind::Shot,
                    count: share(n, 200, 12),
                    origin: pos,
                    angle: dir[1].atan2(dir[0]),
                    spread: 0.5,
                    speed: [40.0, 120.0],
                    life: [6, 14],
                },
                Event::BossHit(pos) => Burst {
                    kind: Kind::Spark,
                    count: share(n, 50, 48),
                    origin: pos,
                    angle: 0.0,
                    spread: PI,
                    speed: [40.0, 220.0],
                    life: [20, 45],
                },
                Event::KnightHit(pos) => Burst {
                    kind: Kind::Blood,
                    count: share(n, 50, 48),
                    origin: pos,
                    angle: 0.0,
                    spread: PI,
                    speed: [30.0, 150.0],
                    life: [20, 40],
                },
                Event::Heal(pos) => Burst {
                    kind: Kind::Heal,
                    count: share(n, 50, 64),
                    origin: pos,
                    angle: -FRAC_PI_2,
                    spread: 0.8,
                    speed: [20.0, 90.0],
                    life: [40, 80],
                },
                Event::BossDeath(pos) => Burst {
                    kind: Kind::Blast,
                    count: share(n, 2, 400),
                    origin: pos,
                    angle: 0.0,
                    spread: PI,
                    speed: [20.0, 260.0],
                    life: [60, MAX_LIFE],
                },
                Event::KnightDeath(pos) => Burst {
                    kind: Kind::Blood,
                    count: share(n, 4, 200),
                    origin: pos,
                    angle: 0.0,
                    spread: PI,
                    speed: [20.0, 200.0],
                    life: [20, 40],
                },
            };
            self.effects.burst(&burst);
        }
        for bolt in &self.bolts {
            self.effects.burst(&Burst {
                kind: Kind::Fire,
                count: share(n, 5_000, 1),
                origin: bolt.pos,
                angle: (-bolt.vel[1]).atan2(-bolt.vel[0]),
                spread: 0.6,
                speed: [10.0, 40.0],
                life: [8, 16],
            });
        }
        for arrow in &self.arrows {
            self.effects.burst(&Burst {
                kind: Kind::Shot,
                count: share(n, 5_000, 1),
                origin: arrow.pos,
                angle: (-arrow.vel[1]).atan2(-arrow.vel[0]),
                spread: 0.3,
                speed: [5.0, 25.0],
                life: [6, 14],
            });
        }
        if input.spray {
            let k = &self.knight;
            self.effects.burst(&Burst {
                kind: Kind::Spray,
                count: share(n, 40, 16),
                origin: k.pos,
                angle: k.facing[1].atan2(k.facing[0]),
                spread: 0.35,
                speed: [120.0, 260.0],
                life: [30, 60],
            });
        }
        let deficit = n.saturating_sub(self.effects.live());
        if self.status == Status::Victory {
            self.effects.embers(deficit, ROOM, [40, MAX_LIFE]);
        } else {
            let aura = deficit / 4;
            self.effects
                .aura(aura, self.boss.pos, [BOSS_RADIUS, BOSS_RADIUS + 14.0]);
            self.effects.embers(deficit - aura, ROOM, [40, MAX_LIFE]);
        }
    }

    fn summary(&self) -> Outcome {
        Outcome {
            frames: self.frame as usize,
            knight_hp: self.knight.hp,
            boss_hp: self.boss.hp,
            victories: self.victories,
            defeats: self.defeats,
            game_checksum: self.game_checksum(),
            checksum: self.checksum(),
            created: self.effects.created(),
            retired: self.effects.retired(),
            particle_updates: self.effects.updates(),
            live: self.effects.live(),
            peak_live: self.peak_live,
            peak_reserved_bytes: self.peak_reserved,
            setup: Duration::ZERO,
            sim: Duration::ZERO,
            teardown: Duration::ZERO,
        }
    }
}

/// Called after every step of [`run`] with the fight and that step's sample.
pub type Observer<'a, A> = &'a mut dyn FnMut(&Fight<A>, Frame);

/// Plays `inputs` from a fresh fight. Setup, the step loop, and teardown
/// (retiring every live particle) are timed separately; checksums are
/// computed between the loop and teardown, outside all timings.
///
/// With `observe`, every step is timed on its own and handed over together
/// with the fight (for traces or drawing); `sim` is then the sum of the step
/// times, so whatever the observer does is not counted.
pub fn run<A: Allocator>(
    cfg: &FightConfig,
    inputs: &[Input],
    mut observe: Option<Observer<'_, A>>,
) -> Outcome {
    let start = Instant::now();
    let mut fight = Fight::<A>::new(cfg);
    let setup = start.elapsed();

    let start = Instant::now();
    let mut stepped = Duration::ZERO;
    for &input in inputs {
        match observe.as_deref_mut() {
            None => fight.step(input),
            Some(observe) => {
                let frame = fight.timed_step(input);
                stepped += frame.step;
                observe(&fight, frame);
            }
        }
    }
    let sim = if observe.is_some() {
        stepped
    } else {
        start.elapsed()
    };

    let mut outcome = fight.summary();
    let start = Instant::now();
    drop(fight);
    outcome.teardown = start.elapsed();
    outcome.setup = setup;
    outcome.sim = sim;
    outcome
}

fn share(n: usize, divisor: usize, min: usize) -> usize {
    (n / divisor).max(min)
}

fn clamp_to_room(pos: [f32; 2], radius: f32) -> [f32; 2] {
    let [left, top, right, bottom] = ROOM;
    [
        pos[0].clamp(left + radius, right - radius),
        pos[1].clamp(top + radius, bottom - radius),
    ]
}

fn distance(a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

fn direction(from: [f32; 2], to: [f32; 2]) -> [f32; 2] {
    let d = distance(from, to);
    if d < 1e-3 {
        [0.0, 0.0]
    } else {
        [(to[0] - from[0]) / d, (to[1] - from[1]) / d]
    }
}

fn mix(hash: u64, word: u32) -> u64 {
    (hash ^ u64::from(word)).wrapping_mul(0x0100_0000_01B3)
}

#[cfg(test)]
mod tests;
