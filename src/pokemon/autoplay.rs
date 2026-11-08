//! A visible exploration policy for GUI demos, separate from the load benchmark patrol.
use std::collections::VecDeque;

use super::{Action, MAP, State};

const ROUTE: [[usize; 2]; 5] = [[7, 1], [7, 5], [4, 5], [3, 2], [4, 2]];

#[derive(Default)]
pub struct Explorer {
    waypoint: usize,
    healing: bool,
}

impl Explorer {
    pub fn action(&mut self, state: &State) -> Action {
        if state.pending.is_some() {
            return Action::Run;
        }
        if let Some(wild) = state.wild {
            return if state.balls > 0 && wild.hp <= wild.max_hp() / 2 {
                Action::Catch
            } else {
                Action::Attack
            };
        }
        // Keep heading home until healed; a battle on the way must not reset the route.
        self.healing |= state.starter.hp < state.starter.max_hp() / 2 || state.balls == 0;
        if self.healing {
            if state.position == [1, 1] {
                self.healing = false;
                return Action::Heal;
            }
            return toward(state.position, [1, 1]);
        }
        if state.position == ROUTE[self.waypoint] {
            self.waypoint = (self.waypoint + 1) % ROUTE.len();
        }
        toward(state.position, ROUTE[self.waypoint])
    }
}

/// Breadth-first search selects a shortest walkable route around the central walls.
fn toward(start: [usize; 2], target: [usize; 2]) -> Action {
    let mut seen = [[false; MAP[0].len()]; MAP.len()];
    let mut queue = VecDeque::from([(start, Action::Wait)]);
    seen[start[1]][start[0]] = true;
    while let Some(([x, y], first)) = queue.pop_front() {
        if [x, y] == target {
            return first;
        }
        for (next, action) in [
            ([x + 1, y], Action::Right),
            ([x, y + 1], Action::Down),
            ([x.saturating_sub(1), y], Action::Left),
            ([x, y.saturating_sub(1)], Action::Up),
        ] {
            let [nx, ny] = next;
            if ny < MAP.len()
                && nx < MAP[ny].len()
                && MAP[ny].as_bytes()[nx] != b'#'
                && !seen[ny][nx]
            {
                seen[ny][nx] = true;
                queue.push_back((next, if first == Action::Wait { action } else { first }));
            }
        }
    }
    Action::Wait
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        alloc::pool::PoolAllocator,
        pokemon::{Config, Pokemon},
    };

    #[test]
    fn explores_both_grass_patches_and_battles_promptly() {
        for seed in [1, 42, 2026] {
            let mut game = Pokemon::<PoolAllocator>::new(Config {
                seed,
                burst: 0,
                ..Config::default()
            });
            let mut explorer = Explorer::default();
            let mut upper = false;
            let mut lower = false;
            for turn in 0..1200 {
                game.step(explorer.action(game.state()));
                let [x, y] = game.state().position;
                if MAP[y].as_bytes()[x] == b'g' {
                    upper |= y <= 2;
                    lower |= y >= 4;
                }
                if turn == 59 {
                    assert!(game.state().encounters > 0, "seed {seed} must battle early");
                }
            }
            assert!(upper && lower, "seed {seed} must explore both patches");
            assert!(game.state().wins + game.state().catches > 0);
        }
    }

    #[test]
    fn walks_around_walls_and_heals_at_center() {
        assert_eq!(toward([4, 2], [4, 4]), Action::Left);
        let game = Pokemon::<PoolAllocator>::new(Config::default());
        let mut state = game.state().clone();
        state.balls = 0;
        let mut explorer = Explorer::default();
        assert_eq!(explorer.action(&state), Action::Heal);
    }
}
