pub mod arena;
pub mod heap;
pub mod pool;

use crate::particle::Particle;

pub trait Allocator {
    type Handle: Copy;

    const NAME: &'static str;

    fn with_capacity(capacity: usize) -> Self;
    fn create(&mut self, particle: Particle) -> Self::Handle;
    fn get(&self, handle: Self::Handle) -> &Particle;
    fn get_mut(&mut self, handle: Self::Handle) -> &mut Particle;
    fn retire(&mut self, handle: Self::Handle);
    fn reclaim(&mut self);
    fn live_count(&self) -> usize;
    fn reserved_bytes(&self) -> usize;

    fn live_bytes(&self) -> usize {
        self.live_count() * std::mem::size_of::<Particle>()
    }
}
