//! Deterministic turn-based exploration and battles shared by all frontends.
//! Gameplay rules never depend on effects or allocator choice. Save/replay
//! checksums include effects; `game_checksum` compares gameplay across loads.

pub mod autoplay;
mod effects;
mod gameplay;
#[cfg(test)]
use gameplay::damage;

use std::time::{Duration, Instant};

use crate::{
    alloc::Allocator,
    particle::{Particle, splitmix64},
};

pub const MAP: [&str; 8] = [
    "############",
    "#H..gggg...#",
    "#...gggg...#",
    "#...##.....#",
    "#...gggg...#",
    "#...gggg...#",
    "#..........#",
    "############",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Wait,
    Up,
    Down,
    Left,
    Right,
    Attack,
    Catch,
    Run,
    Heal,
    Switch(usize),
    Release(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Element {
    Fire,
    Grass,
    Water,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Species {
    Charmander = 0,
    Vulpix = 1,
    Growlithe = 2,
    Bulbasaur = 3,
    Oddish = 4,
    Bellsprout = 5,
    Squirtle = 6,
    Psyduck = 7,
    Poliwag = 8,
}
impl Species {
    pub const ALL: [Species; 9] = [
        Self::Charmander,
        Self::Vulpix,
        Self::Growlithe,
        Self::Bulbasaur,
        Self::Oddish,
        Self::Bellsprout,
        Self::Squirtle,
        Self::Psyduck,
        Self::Poliwag,
    ];

    pub fn from_u8(value: u8) -> Option<Self> {
        Self::ALL.get(usize::from(value)).copied()
    }
    pub fn element(self) -> Element {
        match self {
            Self::Charmander | Self::Vulpix | Self::Growlithe => Element::Fire,
            Self::Bulbasaur | Self::Oddish | Self::Bellsprout => Element::Grass,
            Self::Squirtle | Self::Psyduck | Self::Poliwag => Element::Water,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Charmander => "Charmander",
            Self::Vulpix => "Vulpix",
            Self::Growlithe => "Growlithe",
            Self::Bulbasaur => "Bulbasaur",
            Self::Oddish => "Oddish",
            Self::Bellsprout => "Bellsprout",
            Self::Squirtle => "Squirtle",
            Self::Psyduck => "Psyduck",
            Self::Poliwag => "Poliwag",
        }
    }
    pub fn description(self) -> &'static str {
        match self {
            Self::Charmander => {
                "Charmander is a small orange lizard with a flame on its tail. Its Fire attacks are strong against Grass and weak against Water."
            }
            Self::Vulpix => {
                "Small fox with six tails and a flame at each tip. Fire is strong against Grass and weak against Water."
            }
            Self::Growlithe => {
                "Striped orange puppy. Fire is strong against Grass and weak against Water."
            }
            Self::Bulbasaur => {
                "Bulbasaur is a green, four-legged creature carrying a plant bulb on its back. Its Grass attacks are strong against Water and weak against Fire."
            }
            Self::Oddish => {
                "Round blue weed with leaves on its head. Grass is strong against Water and weak against Fire."
            }
            Self::Bellsprout => {
                "Thin yellow plant with a bell head. Grass is strong against Water and weak against Fire."
            }
            Self::Squirtle => {
                "Squirtle is a small blue turtle protected by a sturdy shell. Its Water attacks are strong against Fire and weak against Grass."
            }
            Self::Psyduck => {
                "Yellow duck with a blank stare. Water is strong against Fire and weak against Grass."
            }
            Self::Poliwag => {
                "Round blue tadpole with a pale belly. Water is strong against Fire and weak against Grass."
            }
        }
    }
    pub fn base_hp(self) -> u32 {
        match self {
            Self::Charmander | Self::Bulbasaur | Self::Squirtle => 24,
            Self::Vulpix | Self::Bellsprout | Self::Poliwag => 22,
            Self::Growlithe => 28,
            Self::Oddish => 20,
            Self::Psyduck => 26,
        }
    }
    pub fn power(self) -> u32 {
        match self {
            Self::Charmander
            | Self::Bulbasaur
            | Self::Squirtle
            | Self::Bellsprout
            | Self::Psyduck => 4,
            Self::Vulpix | Self::Growlithe => 5,
            Self::Oddish | Self::Poliwag => 3,
        }
    }
    pub fn catch_base(self) -> u32 {
        match self {
            Self::Charmander | Self::Bulbasaur | Self::Squirtle => 25,
            Self::Vulpix => 20,
            Self::Growlithe => 12,
            Self::Oddish => 45,
            Self::Bellsprout => 35,
            Self::Psyduck => 18,
            Self::Poliwag => 40,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Monster {
    pub species: Species,
    pub level: u32,
    pub hp: u32,
    pub xp: u32,
}
impl Monster {
    pub fn element(&self) -> Element {
        self.species.element()
    }
    pub fn name(&self) -> &'static str {
        self.species.name()
    }
    pub fn description(&self) -> &'static str {
        self.species.description()
    }
    pub fn max_hp(&self) -> u32 {
        self.species.base_hp() + self.level * 6
    }
    pub fn of(species: Species, level: u32) -> Self {
        let mut monster = Self {
            species,
            level,
            hp: 0,
            xp: 0,
        };
        monster.hp = monster.max_hp();
        monster
    }
    pub(super) fn new(element: Element, level: u32) -> Self {
        let species = match element {
            Element::Fire => Species::Charmander,
            Element::Grass => Species::Bulbasaur,
            Element::Water => Species::Squirtle,
        };
        Self::of(species, level)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Config {
    pub trail: usize,
    pub dt: f32,
    pub seed: u64,
    pub burst: usize,
    pub lifetime: u32,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            seed: 1,
            trail: 0,
            dt: 1.0 / 60.0,
            burst: 32,
            lifetime: 45,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    pub position: [usize; 2],
    pub starter: Monster,
    pub wild: Option<Monster>,
    pub pending: Option<Monster>,
    pub collection: Vec<Monster>,
    pub balls: u32,
    pub encounters: u64,
    pub wins: u64,
    pub losses: u64,
    pub catches: u64,
}
impl State {
    /// True once the active monster and reserves cover Fire, Grass, and Water.
    pub fn has_every_element(&self) -> bool {
        [Element::Fire, Element::Grass, Element::Water]
            .iter()
            .all(|element| {
                std::iter::once(&self.starter)
                    .chain(self.collection.iter())
                    .any(|m| m.element() == *element)
            })
    }
}

#[derive(Debug, Clone)]
pub struct Outcome {
    pub state: State,
    pub checksum: u64,
    pub game_checksum: u64,
    pub created: u64,
    pub retired: u64,
    pub particle_updates: u64,
    pub live: usize,
    pub peak_live: usize,
    pub reserved_bytes: usize,
    pub elapsed: Duration,
    pub setup: Duration,
    pub teardown: Duration,
    pub initial_live: usize,
}

pub struct Pokemon<A: Allocator> {
    alloc: A,
    handles: Vec<A::Handle>,
    cfg: Config,
    state: State,
    rng: u64,
    frame: u64,
    created: u64,
    retired: u64,
    updates: u64,
    peak_live: usize,
    peak_reserved: usize,
}

impl<A: Allocator> Pokemon<A> {
    pub fn new(cfg: Config) -> Self {
        assert!(cfg.lifetime > 0, "particle lifetime must be positive");
        assert!(
            cfg.dt.is_finite() && cfg.dt > 0.0,
            "dt must be finite and positive"
        );
        Self {
            alloc: A::with_capacity(
                cfg.trail
                    .saturating_mul(cfg.lifetime as usize)
                    .saturating_add(cfg.burst),
            ),
            handles: Vec::new(),
            cfg,
            rng: cfg.seed,
            frame: 0,
            created: 0,
            retired: 0,
            updates: 0,
            peak_live: 0,
            peak_reserved: 0,
            state: State {
                position: [1, 1],
                starter: Monster::new(Element::Fire, 1),
                wild: None,
                pending: None,
                collection: Vec::new(),
                balls: 10,
                encounters: 0,
                wins: 0,
                losses: 0,
                catches: 0,
            },
        }
    }
    pub fn config(&self) -> &Config {
        &self.cfg
    }
    pub fn state(&self) -> &State {
        &self.state
    }
    pub fn particles(&self) -> impl Iterator<Item = &Particle> {
        self.handles.iter().map(|&handle| self.alloc.get(handle))
    }
    pub fn live_particles(&self) -> usize {
        self.handles.len()
    }
    fn random(&mut self) -> u64 {
        self.rng = splitmix64(self.rng);
        self.rng
    }

    pub fn step(&mut self, action: Action) {
        let was_in_battle = self.state.wild.is_some();
        let mut burst = self.apply_action(action);
        if was_in_battle && self.state.wild.is_none() {
            self.clear();
            burst = true;
        }
        if self.frame > 0 && self.frame.is_multiple_of(240) {
            self.clear();
        }
        self.age();
        if burst {
            self.effects(self.cfg.burst);
        }
        self.effects(self.cfg.trail);
        self.frame += 1;
        self.peak_live = self.peak_live.max(self.alloc.live_count());
        self.peak_reserved = self.peak_reserved.max(self.alloc.reserved_bytes());
    }
    /// Full replay checksum, including particle order and lifetime counters.
    pub fn checksum(&self) -> u64 {
        self.checksum_inner(true)
    }

    /// Gameplay, turn, and RNG state only, comparable across particle loads.
    pub fn game_checksum(&self) -> u64 {
        self.checksum_inner(false)
    }

    fn checksum_inner(&self, include_effects: bool) -> u64 {
        let mut hash = 0xcbf29ce484222325u64;
        let mut mix = |value: u64| {
            hash = (hash ^ value).wrapping_mul(0x100000001b3);
        };
        mix(self.frame);
        mix(self.rng);
        if include_effects {
            for value in [self.created, self.retired, self.updates] {
                mix(value);
            }
        }
        for value in [
            self.state.position[0] as u64,
            self.state.position[1] as u64,
            u64::from(self.state.balls),
            self.state.encounters,
            self.state.wins,
            self.state.losses,
            self.state.catches,
            self.state.wild.is_some() as u64,
            self.state.pending.is_some() as u64,
            self.state.collection.len() as u64,
        ] {
            mix(value);
        }
        for monster in std::iter::once(&self.state.starter)
            .chain(self.state.wild.iter())
            .chain(self.state.pending.iter())
            .chain(self.state.collection.iter())
        {
            for v in [
                monster.species as u32,
                monster.level,
                monster.hp,
                monster.xp,
            ] {
                mix(u64::from(v));
            }
        }
        if !include_effects {
            return hash;
        }
        for &handle in &self.handles {
            let p = self.alloc.get(handle);
            for v in [
                p.pos[0].to_bits(),
                p.pos[1].to_bits(),
                p.vel[0].to_bits(),
                p.vel[1].to_bits(),
                p.lifetime,
            ] {
                mix(u64::from(v));
            }
        }
        hash
    }
    pub fn outcome(&self, elapsed: Duration) -> Outcome {
        Outcome {
            state: self.state.clone(),
            checksum: self.checksum(),
            game_checksum: self.game_checksum(),
            created: self.created,
            retired: self.retired,
            particle_updates: self.updates,
            live: self.handles.len(),
            peak_live: self.peak_live,
            reserved_bytes: self.peak_reserved,
            elapsed,
            setup: Duration::ZERO,
            teardown: Duration::ZERO,
            initial_live: 0,
        }
    }
    pub fn auto_action(&self) -> Action {
        if self.state.pending.is_some() {
            return Action::Run;
        }
        if let Some(wild) = self.state.wild {
            if wild.hp <= wild.max_hp() / 2
                && self.state.balls > 0
                && self.state.encounters.is_multiple_of(3)
            {
                Action::Catch
            } else {
                Action::Attack
            }
        } else {
            self.patrol()
        }
    }

    fn patrol(&self) -> Action {
        let [x, y] = self.state.position;
        if self.frame % 240 < 200 {
            if y < 6 {
                return Action::Down;
            }
            if y > 6 {
                return Action::Up;
            }
            if (self.frame / 9).is_multiple_of(2) {
                if x < 10 { Action::Right } else { Action::Left }
            } else if x > 1 {
                Action::Left
            } else {
                Action::Right
            }
        } else if y > 5 {
            Action::Up
        } else if y < 5 {
            Action::Down
        } else if x < 4 {
            Action::Right
        } else if x > 7 {
            Action::Left
        } else if (self.frame / 2).is_multiple_of(2) {
            if x < 7 { Action::Right } else { Action::Left }
        } else if x > 4 {
            Action::Left
        } else {
            Action::Right
        }
    }
}
impl<A: Allocator> Drop for Pokemon<A> {
    fn drop(&mut self) {
        self.clear();
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Frame {
    pub index: u32,
    pub step: Duration,
    pub live: usize,
    pub reserved_bytes: usize,
    pub wins: u64,
    pub catches: u64,
}

pub fn run<A: Allocator>(
    cfg: Config,
    frames: u32,
    trace: Option<&mut dyn FnMut(Frame)>,
) -> Outcome {
    let setup = Instant::now();
    let game = Pokemon::<A>::new(cfg);
    let setup = setup.elapsed();
    drive(
        game,
        frames as usize,
        |game, _| game.auto_action(),
        setup,
        trace,
    )
}

/// State observers run outside simulation timing (for GUI viewing or trace collection).
pub type Observer<'a, A> = Option<&'a mut dyn FnMut(&Pokemon<A>, Frame)>;

/// Run an automated or recorded session, optionally maintaining an exact live effect target.
/// Callers validate a recording at its original load before using scaled results.
pub fn benchmark_session<A: Allocator>(
    cfg: Config,
    frames: u32,
    recording: Option<&Replay>,
    particles: Option<usize>,
    observe: Observer<'_, A>,
) -> std::io::Result<Outcome> {
    let setup = Instant::now();
    let mut game = match recording {
        Some(r) => Pokemon::<A>::from_snapshot(&r.initial)?,
        None => Pokemon::<A>::new(cfg),
    };
    if particles.is_some() {
        game.clear();
        game.cfg.burst = 0;
        game.cfg.trail = 0;
    }
    let setup = setup.elapsed();
    let steps = recording.map_or(frames as usize, |r| r.actions.len());
    let mut explorer = autoplay::Explorer::default();
    Ok(drive_observed(
        game,
        steps,
        |game, i| match recording {
            Some(r) => r.actions[i],
            None if particles.is_some() => explorer.action(game.state()),
            None => game.auto_action(),
        },
        setup,
        particles,
        observe,
    ))
}

fn drive<A: Allocator>(
    game: Pokemon<A>,
    steps: usize,
    input: impl FnMut(&Pokemon<A>, usize) -> Action,
    setup: Duration,
    trace: Option<&mut dyn FnMut(Frame)>,
) -> Outcome {
    match trace {
        Some(trace) => drive_observed(
            game,
            steps,
            input,
            setup,
            None,
            Some(&mut |_, frame| trace(frame)),
        ),
        None => drive_observed(game, steps, input, setup, None, None),
    }
}

fn drive_observed<A: Allocator>(
    mut game: Pokemon<A>,
    steps: usize,
    mut input: impl FnMut(&Pokemon<A>, usize) -> Action,
    setup: Duration,
    particles: Option<usize>,
    mut observe: Observer<'_, A>,
) -> Outcome {
    let initial_live = game.handles.len();
    let initial_created = game.created;
    let initial_retired = game.retired;
    let initial_updates = game.updates;
    game.peak_live = initial_live;
    game.peak_reserved = game.alloc.reserved_bytes();
    let start = Instant::now();
    let mut simulation = Duration::ZERO;
    for index in 0..steps {
        let step = observe.as_ref().map(|_| Instant::now());
        let action = input(&game, index);
        game.step(action);
        if let Some(target) = particles {
            game.effects(target.saturating_sub(game.live_particles()));
            game.peak_live = game.peak_live.max(game.live_particles());
            game.peak_reserved = game.peak_reserved.max(game.alloc.reserved_bytes());
        }
        let elapsed = step.map_or(Duration::ZERO, |start| start.elapsed());
        simulation += elapsed;
        if let Some(observe) = observe.as_deref_mut() {
            observe(
                &game,
                Frame {
                    index: index as u32,
                    step: elapsed,
                    live: game.handles.len(),
                    reserved_bytes: game.alloc.reserved_bytes(),
                    wins: game.state.wins,
                    catches: game.state.catches,
                },
            );
        }
    }
    let elapsed = if observe.is_some() {
        simulation
    } else {
        start.elapsed()
    };
    let mut result = game.outcome(elapsed);
    result.setup = setup;
    result.initial_live = initial_live;
    result.created -= initial_created;
    result.retired -= initial_retired;
    result.particle_updates -= initial_updates;
    let teardown = Instant::now();
    drop(game);
    result.teardown = teardown.elapsed();
    result
}

mod replay;
pub use replay::{Replay, replay};

mod messages;
pub use messages::describe;

pub mod render;
mod save;

#[cfg(test)]
mod tests;
