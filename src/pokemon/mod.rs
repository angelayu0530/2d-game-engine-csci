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
pub struct Monster {
    pub element: Element,
    pub level: u32,
    pub hp: u32,
    pub xp: u32,
}
impl Monster {
    pub fn name(&self) -> &'static str {
        match self.element {
            Element::Fire => "Charmander",
            Element::Grass => "Bulbasaur",
            Element::Water => "Squirtle",
        }
    }
    pub fn description(&self) -> &'static str {
        match self.element {
            Element::Fire => {
                "Charmander is a small orange lizard with a flame on its tail. Its Fire attacks are strong against Grass and weak against Water."
            }
            Element::Grass => {
                "Bulbasaur is a green, four-legged creature carrying a plant bulb on its back. Its Grass attacks are strong against Water and weak against Fire."
            }
            Element::Water => {
                "Squirtle is a small blue turtle protected by a sturdy shell. Its Water attacks are strong against Fire and weak against Grass."
            }
        }
    }

    pub fn max_hp(&self) -> u32 {
        24 + self.level * 6
    }
    fn new(element: Element, level: u32) -> Self {
        let mut monster = Self {
            element,
            level,
            hp: 0,
            xp: 0,
        };
        monster.hp = monster.max_hp();
        monster
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
    pub collection: Vec<Monster>,
    pub balls: u32,
    pub encounters: u64,
    pub wins: u64,
    pub losses: u64,
    pub catches: u64,
}

#[derive(Debug, Clone)]
pub struct Outcome {
    pub state: State,
    pub checksum: u64,
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
    fn random(&mut self) -> u64 {
        self.rng = splitmix64(self.rng);
        self.rng
    }

    pub fn step(&mut self, action: Action) {
        let was_in_battle = self.state.wild.is_some();
        let mut burst = false;
        if let Some(mut wild) = self.state.wild {
            let mut retaliate = false;
            match action {
                Action::Attack => {
                    let damage = damage(self.state.starter, wild);
                    wild.hp = wild.hp.saturating_sub(damage);
                    burst = true;
                    if wild.hp == 0 {
                        self.state.wins += 1;
                        self.reward();
                        self.state.wild = None;
                    } else {
                        self.state.wild = Some(wild);
                        retaliate = true;
                    }
                }
                Action::Catch if self.state.balls > 0 => {
                    self.state.balls -= 1;
                    burst = true;
                    let chance = 25 + 65 * (wild.max_hp() - wild.hp) / wild.max_hp();
                    if self.random() % 100 < u64::from(chance) {
                        self.state.catches += 1;
                        if self.state.collection.len() < 5 {
                            self.state.collection.push(wild);
                        }
                        self.reward();
                        self.state.wild = None;
                    } else {
                        retaliate = true;
                    }
                }
                Action::Switch(index) if self.switch(index) => {
                    retaliate = true;
                }
                Action::Run => {
                    self.state.wild = None;
                }
                _ => {}
            }
            if retaliate {
                self.state.starter.hp = self
                    .state
                    .starter
                    .hp
                    .saturating_sub(damage(wild, self.state.starter));
                if self.state.starter.hp == 0 {
                    if let Some(index) = self.state.collection.iter().position(|m| m.hp > 0) {
                        self.switch(index);
                    } else {
                        self.state.losses += 1;
                        self.state.position = [1, 1];
                        self.state.wild = None;
                        self.heal();
                    }
                }
            }
        } else {
            match action {
                Action::Switch(index) => {
                    self.switch(index);
                }
                Action::Release(index) if index < self.state.collection.len() => {
                    self.state.collection.remove(index);
                }
                _ => {}
            }
            let [x, y] = self.state.position;
            let next = match action {
                Action::Up => [x, y.saturating_sub(1)],
                Action::Down => [x, y + 1],
                Action::Left => [x.saturating_sub(1), y],
                Action::Right => [x + 1, y],
                _ => [x, y],
            };
            if action == Action::Heal && MAP[y].as_bytes()[x] == b'H' {
                self.heal();
            }
            if next != [x, y]
                && next[1] < MAP.len()
                && next[0] < MAP[0].len()
                && MAP[next[1]].as_bytes()[next[0]] != b'#'
            {
                self.state.position = next;
                burst = true;
                if MAP[next[1]].as_bytes()[next[0]] == b'g' && self.random().is_multiple_of(3) {
                    let element = match self.random() % 3 {
                        0 => Element::Fire,
                        1 => Element::Grass,
                        _ => Element::Water,
                    };
                    let level = (self.random() % 4 + 1) as u32;
                    self.state.wild = Some(Monster::new(element, level));
                    self.state.encounters += 1;
                }
            }
        }
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
    fn switch(&mut self, index: usize) -> bool {
        match self.state.collection.get_mut(index) {
            Some(monster) if monster.hp > 0 => {
                std::mem::swap(&mut self.state.starter, monster);
                true
            }
            _ => false,
        }
    }
    fn heal(&mut self) {
        self.state.starter.hp = self.state.starter.max_hp();
        for monster in &mut self.state.collection {
            monster.hp = monster.max_hp();
        }
        self.state.balls = 10;
    }
    fn reward(&mut self) {
        let starter = &mut self.state.starter;
        starter.xp = starter.xp.saturating_add(10);
        if starter.xp >= starter.level * 20 && starter.level < 100 {
            starter.xp -= starter.level * 20;
            starter.level += 1;
            starter.hp = starter.max_hp();
        }
        self.state.balls = (self.state.balls + 1).min(10);
    }
    fn effects(&mut self, count: usize) {
        for _ in 0..count {
            let particle = Particle::at(
                self.created as usize,
                self.state.position.map(|v| v as f32 * 32.0),
                self.cfg.lifetime,
            );
            self.handles.push(self.alloc.create(particle));
            self.created += 1;
        }
    }
    fn age(&mut self) {
        self.updates += self.handles.len() as u64;
        for i in (0..self.handles.len()).rev() {
            let particle = self.alloc.get_mut(self.handles[i]);
            particle.update(self.cfg.dt);
            if particle.lifetime == 0 {
                self.alloc.retire(self.handles.swap_remove(i));
                self.retired += 1;
            }
        }
    }

    fn clear(&mut self) {
        for handle in self.handles.drain(..) {
            self.alloc.retire(handle);
            self.retired += 1;
        }
        self.alloc.reclaim();
    }
    pub fn checksum(&self) -> u64 {
        let mut hash = 0xcbf29ce484222325u64;
        let mut mix = |value: u64| {
            hash = (hash ^ value).wrapping_mul(0x100000001b3);
        };
        for value in [
            self.frame,
            self.rng,
            self.created,
            self.retired,
            self.updates,
            self.state.position[0] as u64,
            self.state.position[1] as u64,
            u64::from(self.state.balls),
            self.state.encounters,
            self.state.wins,
            self.state.losses,
            self.state.catches,
            self.state.wild.is_some() as u64,
            self.state.collection.len() as u64,
        ] {
            mix(value);
        }
        for monster in std::iter::once(&self.state.starter)
            .chain(self.state.wild.iter())
            .chain(self.state.collection.iter())
        {
            for v in [
                monster.element as u32,
                monster.level,
                monster.hp,
                monster.xp,
            ] {
                mix(u64::from(v));
            }
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
fn damage(attacker: Monster, defender: Monster) -> u32 {
    let base = 4 + attacker.level * 2;
    match (attacker.element, defender.element) {
        (Element::Fire, Element::Grass)
        | (Element::Grass, Element::Water)
        | (Element::Water, Element::Fire) => base * 2,
        (Element::Grass, Element::Fire)
        | (Element::Water, Element::Grass)
        | (Element::Fire, Element::Water) => base / 2,
        _ => base,
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

fn drive<A: Allocator>(
    mut game: Pokemon<A>,
    steps: usize,
    mut input: impl FnMut(&Pokemon<A>, usize) -> Action,
    setup: Duration,
    mut trace: Option<&mut dyn FnMut(Frame)>,
) -> Outcome {
    let initial_live = game.handles.len();
    let initial_created = game.created;
    let initial_retired = game.retired;
    let initial_updates = game.updates;
    game.peak_live = initial_live;
    game.peak_reserved = game.alloc.reserved_bytes();
    let start = Instant::now();
    for index in 0..steps {
        let action = input(&game, index);
        if let Some(observe) = trace.as_deref_mut() {
            let step = Instant::now();
            game.step(action);
            let elapsed = step.elapsed();
            observe(Frame {
                index: index as u32,
                step: elapsed,
                live: game.handles.len(),
                reserved_bytes: game.alloc.reserved_bytes(),
                wins: game.state.wins,
                catches: game.state.catches,
            });
        } else {
            game.step(action);
        }
    }
    let elapsed = start.elapsed();
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

mod save;
pub mod terminal;

#[cfg(test)]
mod tests;
