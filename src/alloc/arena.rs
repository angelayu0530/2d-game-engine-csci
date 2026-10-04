use std::mem::size_of;

use crate::{alloc::Allocator, particle::Particle};

/// Block size when the arena grows without a capacity hint.
const MIN_BLOCK_SLOTS: usize = 1024;

/// Linear arena: bump through fixed blocks, free nothing until `reclaim` resets every cursor.
///
/// Each block is a `Vec` whose `len` is its bump cursor. A block never pushes past its
/// capacity, so its buffer never moves and handed-out pointers stay valid until `reclaim`.
#[derive(Debug, Default)]
pub struct ArenaAllocator {
    blocks: Vec<Vec<Particle>>,
    /// First block that may still have room; earlier blocks are full.
    current: usize,
    live: usize,
}

impl ArenaAllocator {
    fn block_with_room(&mut self) -> &mut Vec<Particle> {
        while let Some(block) = self.blocks.get(self.current) {
            if block.len() < block.capacity() {
                break;
            }
            self.current += 1;
        }
        if self.current == self.blocks.len() {
            let slots = self
                .blocks
                .last()
                .map_or(MIN_BLOCK_SLOTS, |block| block.capacity().saturating_mul(2));
            self.blocks.push(Vec::with_capacity(slots));
        }
        &mut self.blocks[self.current]
    }

    #[cfg(debug_assertions)]
    fn assert_owned(&self, handle: *mut Particle) {
        let ptr = handle.cast_const();
        let owned = self.blocks.iter().any(|block| {
            let start = block.as_ptr();
            ptr >= start && ptr < start.wrapping_add(block.len())
        });
        assert!(owned, "arena handle {handle:p} is not a slot of this arena");
    }
}

impl Allocator for ArenaAllocator {
    type AllocatedReferenceKey = *mut Particle;

    const NAME: &'static str = "arena";

    fn with_capacity(capacity: usize) -> Self {
        let blocks = if capacity == 0 {
            Vec::new()
        } else {
            vec![Vec::with_capacity(capacity)]
        };
        Self {
            blocks,
            current: 0,
            live: 0,
        }
    }

    fn create(&mut self, particle: Particle) -> Self::AllocatedReferenceKey {
        let block = self.block_with_room();
        let index = block.len();
        // No reallocation: `block_with_room` guarantees spare capacity.
        block.push(particle);
        // SAFETY: `index < len`, so the offset stays inside the block's buffer.
        let handle = unsafe { block.as_mut_ptr().add(index) };
        self.live += 1;
        handle
    }

    fn get(&self, handle: Self::AllocatedReferenceKey) -> &Particle {
        #[cfg(debug_assertions)]
        self.assert_owned(handle);
        // SAFETY: handles point into a block buffer that stays put and initialized until `reclaim`.
        unsafe { &*handle }
    }

    fn get_mut(&mut self, handle: Self::AllocatedReferenceKey) -> &mut Particle {
        #[cfg(debug_assertions)]
        self.assert_owned(handle);
        // SAFETY: as in `get`; `&mut self` prevents overlapping borrows through this allocator.
        unsafe { &mut *handle }
    }

    /// Bookkeeping only; the slot's memory comes back at `reclaim`.
    #[cfg_attr(not(debug_assertions), allow(unused_variables))]
    fn retire(&mut self, handle: Self::AllocatedReferenceKey) {
        #[cfg(debug_assertions)]
        self.assert_owned(handle);
        self.live -= 1;
    }

    /// Resets every cursor; blocks stay reserved for the next batch.
    fn reclaim(&mut self) {
        assert_eq!(self.live, 0, "reclaim called with live arena objects");
        for block in &mut self.blocks {
            block.clear();
        }
        self.current = 0;
    }

    fn live_count(&self) -> usize {
        self.live
    }

    fn reserved_bytes(&self) -> usize {
        self.blocks
            .iter()
            .map(|block| block.capacity() * size_of::<Particle>())
            .sum()
    }
}
