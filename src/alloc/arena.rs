use std::{mem::size_of, ptr::from_mut};

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

/// Pointer into a block buffer. Only this module can construct it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Handle(*mut Particle);

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
    fn assert_owned(&self, handle: Handle) {
        let ptr = handle.0.cast_const();
        let owned = self.blocks.iter().any(|block| {
            let start = block.as_ptr();
            ptr >= start && ptr < start.wrapping_add(block.len())
        });
        assert!(
            owned,
            "arena handle {:p} is not a slot of this arena",
            handle.0
        );
    }
}

impl Allocator for ArenaAllocator {
    type Handle = Handle;

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

    fn create(&mut self, particle: Particle) -> Self::Handle {
        let block = self.block_with_room();
        let index = block.len();
        block.push(particle);
        let handle = Handle(from_mut(&mut block[index]));
        self.live += 1;
        handle
    }

    fn get(&self, handle: Self::Handle) -> &Particle {
        #[cfg(debug_assertions)]
        self.assert_owned(handle);
        // SAFETY: `handle.0` points into a block buffer that stays put and initialized until
        // `reclaim`. `block_with_room` never pushes past a block's capacity, so the buffer
        // does not move.
        unsafe { &*handle.0 }
    }

    fn get_mut(&mut self, handle: Self::Handle) -> &mut Particle {
        #[cfg(debug_assertions)]
        self.assert_owned(handle);
        // SAFETY: as in `get`. `&mut self` prevents overlapping borrows through this allocator.
        unsafe { &mut *handle.0 }
    }

    /// Bookkeeping only; the slot's memory comes back at `reclaim`.
    fn retire(&mut self, handle: Self::Handle) {
        #[cfg(debug_assertions)]
        self.assert_owned(handle);
        #[cfg(not(debug_assertions))]
        let _ = handle;
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
