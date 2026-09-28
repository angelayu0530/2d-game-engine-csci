#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub struct Particle {
    pub pos: [f32; 2],
    pub vel: [f32; 2],
    pub lifetime: u32,
}

impl Particle {
    pub fn seeded(_index: usize) -> Self {
        todo!()
    }

    pub fn update(&mut self, _dt: f32) {
        todo!()
    }
}
