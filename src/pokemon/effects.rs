//! Allocator-backed visual effects; independent of gameplay randomness.
use super::*;

impl<A: Allocator> Pokemon<A> {
    pub(super) fn effects(&mut self, count: usize) {
        self.handles.reserve(count);
        for _ in 0..count {
            let particle = Particle::at(
                self.created as usize,
                self.state.position.map(|v| v as f32 * 32.0),
                self.cfg.lifetime,
            );
            self.handles.push(self.alloc.create(particle));
            self.created += 1;
        }
    }
    pub(super) fn age(&mut self) {
        self.updates += self.handles.len() as u64;
        for i in (0..self.handles.len()).rev() {
            let particle = self.alloc.get_mut(self.handles[i]);
            particle.update(self.cfg.dt);
            if particle.lifetime == 0 {
                self.alloc.retire(self.handles.swap_remove(i));
                self.retired += 1;
            }
        }
        // No handles survive: reuse arena blocks before the next effect burst.
        // Without this, short-lived effects accumulate storage until a scene reset.
        if self.handles.is_empty() {
            self.alloc.reclaim();
        }
    }

    pub(super) fn clear(&mut self) {
        for handle in self.handles.drain(..) {
            self.alloc.retire(handle);
            self.retired += 1;
        }
        self.alloc.reclaim();
    }
}
