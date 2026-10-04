pub mod arena;
pub mod heap;
pub mod pool;

use std::mem::size_of;

use crate::particle::Particle;

/// Backing storage for particles.
///
/// A `Handle` comes from `create` and stays valid only until `retire` on that same allocator.
/// Using a heap or arena handle after `retire` is undefined behavior.
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
        self.live_count() * size_of::<Particle>()
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Debug;

    use super::{Allocator, arena::ArenaAllocator, heap::HeapAllocator, pool::PoolAllocator};
    use crate::particle::Particle;

    fn particle(i: usize) -> Particle {
        let x = i as f32;
        Particle {
            pos: [x, -x],
            vel: [x * 0.5, 1.0],
            lifetime: i as u32,
        }
    }

    /// Grows past any initial block/capacity, interleaves retire and reuse, and checks that
    /// every live handle still resolves to its own particle.
    fn churn_keeps_values<A: Allocator>() {
        let mut alloc = A::with_capacity(8);
        let mut live: Vec<(A::Handle, usize)> =
            (0..5_000).map(|i| (alloc.create(particle(i)), i)).collect();

        // Retire every other object, then refill so pool slots get recycled.
        let mut kept = Vec::new();
        for (n, (handle, i)) in live.drain(..).enumerate() {
            if n % 2 == 0 {
                alloc.retire(handle);
            } else {
                kept.push((handle, i));
            }
        }
        kept.extend((5_000..7_500).map(|i| (alloc.create(particle(i)), i)));
        assert_eq!(alloc.live_count(), kept.len(), "{}", A::NAME);
        assert_eq!(
            alloc.live_bytes(),
            kept.len() * size_of::<Particle>(),
            "{}",
            A::NAME
        );

        for &(handle, _) in &kept {
            alloc.get_mut(handle).lifetime += 1;
        }
        for &(handle, i) in &kept {
            let expected = Particle {
                lifetime: i as u32 + 1,
                ..particle(i)
            };
            assert_eq!(*alloc.get(handle), expected, "{} handle for {i}", A::NAME);
        }

        for (handle, _) in kept {
            alloc.retire(handle);
        }
        assert_eq!(alloc.live_count(), 0, "{}", A::NAME);
        alloc.reclaim();
    }

    /// A second identical batch after `reclaim` must fit in the storage the first one reserved.
    fn reclaim_reuses_storage<A: Allocator>() {
        let mut alloc = A::with_capacity(0);
        let mut reserved = Vec::new();
        for _ in 0..2 {
            let handles: Vec<_> = (0..3_000).map(|i| alloc.create(particle(i))).collect();
            reserved.push(alloc.reserved_bytes());
            for handle in handles {
                alloc.retire(handle);
            }
            alloc.reclaim();
        }
        assert_eq!(reserved[0], reserved[1], "{}", A::NAME);
    }

    const MASS: usize = 1_000_000;

    /// Creates `MASS` objects and checks every returned key, then retires them all. Three
    /// batches: the second runs before `reclaim` (pool reuses retired slots, arena keeps
    /// bumping), the third after it (storage reset). Run with no capacity hint, so storage grows,
    /// and with an exact hint.
    fn mass_create_and_retire<A: Allocator>()
    where
        A::Handle: Ord + Debug,
    {
        for capacity in [0, MASS] {
            let mut alloc = A::with_capacity(capacity);
            for batch in 0..3 {
                if batch == 2 {
                    alloc.reclaim();
                }
                let ctx = format!("{} capacity {capacity} batch {batch}", A::NAME);
                let offset = batch * MASS;

                let mut handles = Vec::with_capacity(MASS);
                for i in 0..MASS {
                    let expected = particle(offset + i);
                    let handle = alloc.create(expected);
                    assert_eq!(
                        *alloc.get(handle),
                        expected,
                        "{ctx}: key for object {i} resolves to the wrong particle"
                    );
                    handles.push(handle);
                }
                assert_eq!(alloc.live_count(), MASS, "{ctx}");

                // Later creates must not overwrite or move earlier objects.
                for (i, &handle) in handles.iter().enumerate() {
                    assert_eq!(
                        *alloc.get(handle),
                        particle(offset + i),
                        "{ctx}: object {i} changed after later creates"
                    );
                }

                let mut sorted = handles.clone();
                sorted.sort_unstable();
                if let Some(pair) = sorted.windows(2).find(|pair| pair[0] == pair[1]) {
                    panic!("{ctx}: key {:?} handed out twice", pair[0]);
                }

                for handle in handles {
                    alloc.retire(handle);
                }
                assert_eq!(alloc.live_count(), 0, "{ctx}");
            }
            alloc.reclaim();
        }
    }

    #[test]
    fn heap_churn_keeps_values() {
        churn_keeps_values::<HeapAllocator>();
    }

    #[test]
    fn pool_churn_keeps_values() {
        churn_keeps_values::<PoolAllocator>();
    }

    #[test]
    fn arena_churn_keeps_values() {
        churn_keeps_values::<ArenaAllocator>();
    }

    #[test]
    fn pool_reclaim_reuses_storage() {
        reclaim_reuses_storage::<PoolAllocator>();
    }

    #[test]
    fn arena_reclaim_reuses_storage() {
        reclaim_reuses_storage::<ArenaAllocator>();
    }

    #[test]
    fn heap_mass_create_and_retire() {
        mass_create_and_retire::<HeapAllocator>();
    }

    #[test]
    fn pool_mass_create_and_retire() {
        mass_create_and_retire::<PoolAllocator>();
    }

    #[test]
    fn arena_mass_create_and_retire() {
        mass_create_and_retire::<ArenaAllocator>();
    }

    #[test]
    fn pool_recycles_retired_slots() {
        let mut pool = PoolAllocator::with_capacity(4);
        let a = pool.create(particle(1));
        let _b = pool.create(particle(2));
        pool.retire(a);
        let c = pool.create(particle(3));
        assert_eq!(c, a);
        assert_eq!(*pool.get(c), particle(3));
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "not live")]
    fn pool_rejects_double_retire() {
        let mut pool = PoolAllocator::with_capacity(4);
        let a = pool.create(particle(1));
        pool.retire(a);
        pool.retire(a);
    }

    #[test]
    #[should_panic(expected = "live arena objects")]
    fn arena_reclaim_rejects_live_objects() {
        let mut arena = ArenaAllocator::with_capacity(4);
        arena.create(particle(1));
        arena.reclaim();
    }
}
