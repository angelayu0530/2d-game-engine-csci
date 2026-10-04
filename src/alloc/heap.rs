use std::mem::size_of;

use crate::{alloc::Allocator, particle::Particle};

#[derive(Debug, Default)]
pub struct HeapAllocator {
    live: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Handle(*mut Particle);

impl Allocator for HeapAllocator {
    type Handle = Handle;

    const NAME: &'static str = "heap";

    fn with_capacity(_capacity: usize) -> Self {
        Self::default()
    }

    fn create(&mut self, particle: Particle) -> Self::Handle {
        self.live += 1;
        Handle(Box::into_raw(Box::new(particle)))
    }

    fn get(&self, handle: Self::Handle) -> &Particle {
        unsafe { &*handle.0 }
    }

    fn get_mut(&mut self, handle: Self::Handle) -> &mut Particle {
        unsafe { &mut *handle.0 }
    }

    fn retire(&mut self, handle: Self::Handle) {
        drop(unsafe { Box::from_raw(handle.0) });
        self.live -= 1;
    }

    fn reclaim(&mut self) {
        assert_eq!(self.live, 0, "reclaim called with live heap objects");
    }

    fn live_count(&self) -> usize {
        self.live
    }

    fn reserved_bytes(&self) -> usize {
        self.live * size_of::<Particle>()
    }
}
