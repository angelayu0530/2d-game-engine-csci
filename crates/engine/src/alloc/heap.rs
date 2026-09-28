use super::Allocator;
use crate::particle::Particle;

#[derive(Debug, Default)]
pub struct HeapAllocator;

impl Allocator for HeapAllocator {
    type Handle = *mut Particle;

    const NAME: &'static str = "heap";

    fn with_capacity(_capacity: usize) -> Self {
        todo!()
    }

    fn create(&mut self, _particle: Particle) -> Self::Handle {
        todo!()
    }

    fn get(&self, _handle: Self::Handle) -> &Particle {
        todo!()
    }

    fn get_mut(&mut self, _handle: Self::Handle) -> &mut Particle {
        todo!()
    }

    fn retire(&mut self, _handle: Self::Handle) {
        todo!()
    }

    fn reclaim(&mut self) {
        todo!()
    }

    fn live_count(&self) -> usize {
        todo!()
    }

    fn reserved_bytes(&self) -> usize {
        todo!()
    }
}
