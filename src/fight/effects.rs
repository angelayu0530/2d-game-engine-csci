use std::f32::consts::TAU;

use crate::{
    alloc::Allocator,
    particle::{Particle, signed_unit, splitmix64, unit},
};

/// Longest lifetime, in steps, any effect particle may have.
pub const MAX_LIFE: u32 = 120;
/// Steps during which new particles go into the same generation.
const EPOCH: u32 = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Kind {
    Ember,
    Shot,
    Spark,
    Blood,
    Heal,
    Fire,
    Spray,
    Blast,
}

impl Kind {
    pub const ALL: [Kind; 8] = [
        Kind::Ember,
        Kind::Shot,
        Kind::Spark,
        Kind::Blood,
        Kind::Heal,
        Kind::Fire,
        Kind::Spray,
        Kind::Blast,
    ];

    /// Upper bound of this kind's spawn lifetime, for fading.
    pub const fn max_life(self) -> u32 {
        match self {
            Kind::Ember | Kind::Blast => MAX_LIFE,
            Kind::Shot => 14,
            Kind::Spark => 45,
            Kind::Blood => 40,
            Kind::Heal => 80,
            Kind::Fire => 16,
            Kind::Spray => 60,
        }
    }
}

/// A cone of particles: `count` particles leave `origin` at `angle` ±
/// `spread` radians with speeds and lifetimes drawn from the given ranges.
#[derive(Debug, Clone, Copy)]
pub struct Burst {
    pub kind: Kind,
    pub count: usize,
    pub origin: [f32; 2],
    pub angle: f32,
    pub spread: f32,
    pub speed: [f32; 2],
    pub life: [u32; 2],
}

struct Generation<A: Allocator> {
    alloc: A,
    sparks: Vec<(A::Handle, Kind)>,
    dirty: bool,
}

impl<A: Allocator> Generation<A> {
    fn new() -> Self {
        Self {
            alloc: A::with_capacity(0),
            sparks: Vec::new(),
            dirty: false,
        }
    }
}

/// Effect particles, stored through allocator `A`.
///
/// Particles spawned during one [`EPOCH`] share a generation with its own
/// allocator. A generation stops receiving particles when its epoch ends and
/// is reclaimed once its last particle dies, at most [`MAX_LIFE`] steps later,
/// so an arena never has to wait for the whole room to go quiet. Every
/// backend uses the same generations, so the comparison stays like for like.
pub struct Effects<A: Allocator> {
    gens: Vec<Generation<A>>,
    current: usize,
    epoch_left: u32,
    dead: Vec<usize>,
    live: usize,
    next_seed: u64,
    created: u64,
    retired: u64,
    updates: u64,
}

impl<A: Allocator> Effects<A> {
    pub fn new(seed: u64) -> Self {
        Self {
            gens: vec![Generation::new()],
            current: 0,
            epoch_left: EPOCH,
            dead: Vec::new(),
            live: 0,
            next_seed: splitmix64(seed ^ 0xEFF3_C75E_ED00_0000),
            created: 0,
            retired: 0,
            updates: 0,
        }
    }

    pub fn live(&self) -> usize {
        self.live
    }

    pub fn created(&self) -> u64 {
        self.created
    }

    pub fn retired(&self) -> u64 {
        self.retired
    }

    pub fn updates(&self) -> u64 {
        self.updates
    }

    pub fn generations(&self) -> usize {
        self.gens.len()
    }

    pub fn reserved_bytes(&self) -> usize {
        self.gens.iter().map(|g| g.alloc.reserved_bytes()).sum()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&Particle, Kind)> {
        self.gens.iter().flat_map(|g| {
            g.sparks
                .iter()
                .map(move |&(handle, kind)| (g.alloc.get(handle), kind))
        })
    }

    pub fn push(&mut self, particle: Particle, kind: Kind) {
        debug_assert!((1..=MAX_LIFE).contains(&particle.lifetime));
        let generation = &mut self.gens[self.current];
        let handle = generation.alloc.create(particle);
        generation.sparks.push((handle, kind));
        generation.dirty = true;
        self.live += 1;
        self.created += 1;
    }

    /// Advances every particle one step, retires the expired, reclaims
    /// drained generations, and starts a new generation every epoch.
    pub fn age(&mut self, dt: f32) {
        for (index, generation) in self.gens.iter_mut().enumerate() {
            self.updates += generation.sparks.len() as u64;
            for (i, &(handle, _)) in generation.sparks.iter().enumerate() {
                let particle = generation.alloc.get_mut(handle);
                particle.update(dt);
                if particle.lifetime == 0 {
                    self.dead.push(i);
                }
            }
            for &i in self.dead.iter().rev() {
                let (handle, _) = generation.sparks.swap_remove(i);
                generation.alloc.retire(handle);
            }
            self.live -= self.dead.len();
            self.retired += self.dead.len() as u64;
            self.dead.clear();
            if index != self.current && generation.dirty && generation.sparks.is_empty() {
                generation.alloc.reclaim();
                generation.dirty = false;
            }
        }
        self.epoch_left -= 1;
        if self.epoch_left == 0 {
            self.epoch_left = EPOCH;
            let previous = self.current;
            self.current =
                match (0..self.gens.len()).find(|&i| i != previous && !self.gens[i].dirty) {
                    Some(index) => index,
                    None => {
                        self.gens.push(Generation::new());
                        self.gens.len() - 1
                    }
                };
        }
    }

    pub fn burst(&mut self, burst: &Burst) {
        let [slow, fast] = burst.speed;
        for _ in 0..burst.count {
            let [a, b] = self.random();
            let angle = burst.angle + burst.spread * signed_unit(a as u32);
            let speed = slow + (fast - slow) * unit((a >> 32) as u32);
            let particle = Particle {
                pos: burst.origin,
                vel: [angle.cos() * speed, angle.sin() * speed],
                lifetime: life(b, burst.life),
            };
            self.push(particle, burst.kind);
        }
    }

    /// Drifting embers: `count` spread over `area` (`[left, top, right,
    /// bottom]`), with lifetimes in `life`.
    pub fn embers(&mut self, count: usize, area: [f32; 4], life_range: [u32; 2]) {
        let [left, top, right, bottom] = area;
        for _ in 0..count {
            let [a, b] = self.random();
            let particle = Particle {
                pos: [
                    left + (right - left) * unit(a as u32),
                    top + (bottom - top) * unit((a >> 32) as u32),
                ],
                vel: [
                    12.0 * signed_unit(b as u32),
                    -10.0 - 30.0 * unit((b >> 32) as u32),
                ],
                lifetime: life(splitmix64(b), life_range),
            };
            self.push(particle, Kind::Ember);
        }
    }

    /// Embers in a ring around `center`.
    pub fn aura(&mut self, count: usize, center: [f32; 2], radius: [f32; 2]) {
        for _ in 0..count {
            let [a, b] = self.random();
            let angle = TAU * unit(a as u32);
            let r = radius[0] + (radius[1] - radius[0]) * unit((a >> 32) as u32);
            let particle = Particle {
                pos: [center[0] + angle.cos() * r, center[1] + angle.sin() * r],
                vel: [
                    6.0 * signed_unit(b as u32),
                    -20.0 - 40.0 * unit((b >> 32) as u32),
                ],
                lifetime: life(splitmix64(b), [30, 90]),
            };
            self.push(particle, Kind::Ember);
        }
    }

    pub fn checksum(&self, mut hash: u64) -> u64 {
        for (particle, kind) in self.iter() {
            for word in [
                particle.pos[0].to_bits(),
                particle.pos[1].to_bits(),
                particle.vel[0].to_bits(),
                particle.vel[1].to_bits(),
                particle.lifetime,
                kind as u32,
            ] {
                hash = super::mix(hash, word);
            }
        }
        hash
    }

    fn random(&mut self) -> [u64; 2] {
        let a = splitmix64(self.next_seed);
        let b = splitmix64(a);
        self.next_seed = b;
        [a, b]
    }
}

impl<A: Allocator> Drop for Effects<A> {
    fn drop(&mut self) {
        for generation in &mut self.gens {
            for (handle, _) in generation.sparks.drain(..) {
                generation.alloc.retire(handle);
            }
            generation.alloc.reclaim();
        }
        self.live = 0;
    }
}

fn life(bits: u64, [lo, hi]: [u32; 2]) -> u32 {
    lo + (bits % u64::from(hi - lo + 1)) as u32
}
