use std::mem::size_of;

use crate::{alloc::Allocator, particle::Particle};

/// Baseline: one global-allocator call per object, freed individually on retire.
#[derive(Debug, Default)]
pub struct HeapAllocator {
    live: usize,
}

/// Pointer to one `Box` allocation. Only this module can construct it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Handle(*mut Particle);

impl Allocator for HeapAllocator {
    type Handle = Handle;

    const NAME: &'static str = "heap";

    /// Nothing to preallocate: the baseline pays for every object at `create`.
    fn with_capacity(_capacity: usize) -> Self {
        Self::default()
    }

    fn create(&mut self, particle: Particle) -> Self::Handle {
        self.live += 1;
        Handle(Box::into_raw(Box::new(particle)))
    }

    fn get(&self, handle: Self::Handle) -> &Particle {
        // SAFETY: `handle.0` comes from `Box::into_raw` in `create` and stays valid until `retire`.
        unsafe { &*handle.0 }
    }

    fn get_mut(&mut self, handle: Self::Handle) -> &mut Particle {
        // SAFETY: as in `get`. `&mut self` prevents overlapping borrows through this allocator.
        unsafe { &mut *handle.0 }
    }

    fn retire(&mut self, handle: Self::Handle) {
        // SAFETY: `handle.0` came from `Box::into_raw` in `create` and is retired exactly once.
        drop(unsafe { Box::from_raw(handle.0) });
        self.live -= 1;
    }

    /// Objects were already freed one by one in `retire`.
    fn reclaim(&mut self) {
        assert_eq!(self.live, 0, "reclaim called with live heap objects");
    }

    fn live_count(&self) -> usize {
        self.live
    }

    /// Payload bytes only; per-allocation malloc headers are invisible here and show up in RSS.
    fn reserved_bytes(&self) -> usize {
        self.live * size_of::<Particle>()
    }
}
