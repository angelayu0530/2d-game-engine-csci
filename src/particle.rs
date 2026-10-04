const WORLD: f32 = 1024.0;
const MAX_SPEED: f32 = 64.0;
const GRAVITY: f32 = 9.8;
const MIN_LIFETIME: u32 = 60;
const LIFETIME_SPREAD: u32 = 240;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Particle {
    pub pos: [f32; 2],
    pub vel: [f32; 2],
    pub lifetime: u32,
}

impl Particle {
    pub fn seeded(index: usize) -> Self {
        let a = splitmix64(index as u64);
        let b = splitmix64(a);
        let c = splitmix64(b);
        Self {
            pos: [unit(a as u32) * WORLD, unit((a >> 32) as u32) * WORLD],
            vel: [
                signed_unit(b as u32) * MAX_SPEED,
                signed_unit((b >> 32) as u32) * MAX_SPEED,
            ],
            lifetime: MIN_LIFETIME + (c % u64::from(LIFETIME_SPREAD)) as u32,
        }
    }

    pub fn update(&mut self, dt: f32) {
        self.vel[1] -= GRAVITY * dt;
        self.pos[0] += self.vel[0] * dt;
        self.pos[1] += self.vel[1] * dt;
        self.lifetime = self.lifetime.saturating_sub(1);
    }
}

fn splitmix64(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn unit(bits: u32) -> f32 {
    (bits >> 8) as f32 / (1u32 << 24) as f32
}

fn signed_unit(bits: u32) -> f32 {
    unit(bits) * 2.0 - 1.0
}
