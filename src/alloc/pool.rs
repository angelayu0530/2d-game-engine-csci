use std::{fmt, mem::size_of};

use crate::{alloc::Allocator, particle::Particle};

/// Free-list terminator; also caps the pool at `u32::MAX - 1` slots.
const NIL: u32 = u32::MAX;

/// A slot is either a live particle or a link in the intrusive free list.
///
/// Invariant: every slot is written in full as `Slot { particle }` before its index is handed
/// out, and retiring only overwrites `next_free`. `Particle` is all 4-byte f32/u32 fields, so it
/// has no padding in any field order and every bit pattern is valid: reading either field
/// always sees initialized bytes.
union Slot {
    particle: Particle,
    next_free: u32,
}

/// Fixed-size pool: contiguous slots reserved up front, retired slots recycled LIFO.
pub struct PoolAllocator {
    slots: Vec<Slot>,
    free_head: u32,
    live: usize,
    #[cfg(debug_assertions)]
    occupied: Vec<bool>,
}

impl PoolAllocator {
    #[cfg(debug_assertions)]
    fn assert_live(&self, handle: u32) {
        assert!(
            self.occupied.get(handle as usize).copied().unwrap_or(false),
            "pool handle {handle} is not live"
        );
    }
}

impl Default for PoolAllocator {
    fn default() -> Self {
        Self::with_capacity(0)
    }
}

impl fmt::Debug for PoolAllocator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PoolAllocator")
            .field("slots", &self.slots.len())
            .field("capacity", &self.slots.capacity())
            .field("free_head", &self.free_head)
            .field("live", &self.live)
            .finish()
    }
}

impl Allocator for PoolAllocator {
    type AllocatedReferenceKey = u32;

    const NAME: &'static str = "pool";

    fn with_capacity(capacity: usize) -> Self {
        assert!(
            capacity < NIL as usize,
            "pool capacity exceeds u32 handle space"
        );
        Self {
            slots: Vec::with_capacity(capacity),
            free_head: NIL,
            live: 0,
            #[cfg(debug_assertions)]
            occupied: Vec::with_capacity(capacity),
        }
    }

    fn create(&mut self, particle: Particle) -> Self::AllocatedReferenceKey {
        let handle = if self.free_head == NIL {
            assert!(
                self.slots.len() < NIL as usize,
                "pool handle space exhausted"
            );
            let handle = self.slots.len() as u32;
            self.slots.push(Slot { particle });
            #[cfg(debug_assertions)]
            self.occupied.push(true);
            handle
        } else {
            let handle = self.free_head;
            let slot = &mut self.slots[handle as usize];
            // SAFETY: slot bytes are always initialized (see `Slot`) and any u32 is valid.
            self.free_head = unsafe { slot.next_free };
            *slot = Slot { particle };
            #[cfg(debug_assertions)]
            {
                self.occupied[handle as usize] = true;
            }
            handle
        };
        self.live += 1;
        handle
    }

    fn get(&self, handle: Self::AllocatedReferenceKey) -> &Particle {
        #[cfg(debug_assertions)]
        self.assert_live(handle);
        // SAFETY: slot bytes are always a valid `Particle` (see `Slot`); a stale handle reads
        // garbage but never uninitialized or freed memory.
        unsafe { &self.slots[handle as usize].particle }
    }

    fn get_mut(&mut self, handle: Self::AllocatedReferenceKey) -> &mut Particle {
        #[cfg(debug_assertions)]
        self.assert_live(handle);
        // SAFETY: as in `get`.
        unsafe { &mut self.slots[handle as usize].particle }
    }

    fn retire(&mut self, handle: Self::AllocatedReferenceKey) {
        #[cfg(debug_assertions)]
        {
            self.assert_live(handle);
            self.occupied[handle as usize] = false;
        }
        self.slots[handle as usize].next_free = self.free_head;
        self.free_head = handle;
        self.live -= 1;
    }

    /// Drops every slot at once but keeps the reserved storage for the next batch.
    fn reclaim(&mut self) {
        assert_eq!(self.live, 0, "reclaim called with live pool objects");
        self.slots.clear();
        self.free_head = NIL;
        #[cfg(debug_assertions)]
        self.occupied.clear();
    }

    fn live_count(&self) -> usize {
        self.live
    }

    fn reserved_bytes(&self) -> usize {
        self.slots.capacity() * size_of::<Slot>()
    }
}

#[cfg(test)]
mod tests {
    use std::mem::size_of;

    use super::Slot;
    use crate::particle::Particle;

    // The free-list link lives inside dead particles, so a slot costs exactly one particle.
    const _: () = assert!(size_of::<Slot>() == size_of::<Particle>());
}
