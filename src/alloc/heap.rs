use std::mem::size_of;

use crate::{alloc::Allocator, particle::Particle};

/// Baseline: one global-allocator call per object, freed individually on retire.
#[derive(Debug, Default)]
pub struct HeapAllocator {
    live: usize,
}

impl Allocator for HeapAllocator {
    type AllocatedReferenceKey = *mut Particle;

    const NAME: &'static str = "heap";

    /// Nothing to preallocate: the baseline pays for every object at `create`.
    fn with_capacity(_capacity: usize) -> Self {
        Self::default()
    }

    fn create(&mut self, particle: Particle) -> Self::AllocatedReferenceKey {
        self.live += 1;
        Box::into_raw(Box::new(particle))
    }

    fn get(&self, handle: Self::AllocatedReferenceKey) -> &Particle {
        // SAFETY: handles come from `create` (`Box::into_raw`) and stay valid until `retire`.
        unsafe { &*handle }
    }

    fn get_mut(&mut self, handle: Self::AllocatedReferenceKey) -> &mut Particle {
        // SAFETY: as in `get`; `&mut self` prevents overlapping borrows through this allocator.
        unsafe { &mut *handle }
    }

    fn retire(&mut self, handle: Self::AllocatedReferenceKey) {
        // SAFETY: `handle` came from `Box::into_raw` in `create` and is retired exactly once.
        drop(unsafe { Box::from_raw(handle) });
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
