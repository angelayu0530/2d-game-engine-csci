//! Battle and exploration rules. These never read or update particle state.
use super::*;

impl<A: Allocator> Pokemon<A> {
    /// Apply one turn and report whether it produces a visual burst.
    pub(super) fn apply_action(&mut self, action: Action) -> bool {
        if let Some(wild) = self.state.wild {
            self.battle_action(action, wild)
        } else if self.state.pending.is_some() {
            self.pending_action(action);
            false
        } else {
            self.explore_action(action)
        }
    }

    fn battle_action(&mut self, action: Action, mut wild: Monster) -> bool {
        let mut burst = false;
        let mut retaliate = false;
        match action {
            Action::Attack => {
                let damage = damage(self.state.starter, wild);
                wild.hp = wild.hp.saturating_sub(damage);
                burst = true;
                if wild.hp == 0 {
                    self.state.wins += 1;
                    self.reward();
                    self.state.wild = None;
                } else {
                    self.state.wild = Some(wild);
                    retaliate = true;
                }
            }
            Action::Catch if self.state.balls > 0 => {
                self.state.balls -= 1;
                burst = true;
                let catch_base = wild.species.catch_base();
                let chance =
                    catch_base + (90 - catch_base) * (wild.max_hp() - wild.hp) / wild.max_hp();
                if self.random() % 100 < u64::from(chance) {
                    if self.state.collection.len() < 5 && self.state.pending.is_none() {
                        self.state.collection.push(wild);
                        self.state.catches += 1;
                        self.reward();
                    } else {
                        self.state.pending = Some(wild);
                    }
                    self.state.wild = None;
                } else {
                    retaliate = true;
                }
            }
            Action::Switch(index) if self.switch(index) => {
                retaliate = true;
            }
            Action::Run => {
                self.state.wild = None;
            }
            _ => {}
        }
        if retaliate {
            self.state.starter.hp = self
                .state
                .starter
                .hp
                .saturating_sub(damage(wild, self.state.starter));
            if self.state.starter.hp == 0 {
                if let Some(index) = self.state.collection.iter().position(|m| m.hp > 0) {
                    self.switch(index);
                } else {
                    self.state.losses += 1;
                    self.state.position = [1, 1];
                    self.state.wild = None;
                    self.state.pending = None;
                    self.heal();
                }
            }
        }
        burst
    }

    fn pending_action(&mut self, action: Action) {
        match action {
            Action::Run => {
                self.state.pending = None;
            }
            Action::Release(index) if index < self.state.collection.len() => {
                if let Some(monster) = self.state.pending.take() {
                    self.state.collection.remove(index);
                    self.state.collection.push(monster);
                    self.state.catches += 1;
                    self.reward();
                }
            }
            _ => {}
        }
    }

    fn explore_action(&mut self, action: Action) -> bool {
        let mut burst = false;
        match action {
            Action::Switch(index) => {
                self.switch(index);
            }
            Action::Release(index) if index < self.state.collection.len() => {
                self.state.collection.remove(index);
            }
            _ => {}
        }
        let [x, y] = self.state.position;
        let next = match action {
            Action::Up => [x, y.saturating_sub(1)],
            Action::Down => [x, y + 1],
            Action::Left => [x.saturating_sub(1), y],
            Action::Right => [x + 1, y],
            _ => [x, y],
        };
        if action == Action::Heal && MAP[y].as_bytes()[x] == b'H' {
            self.heal();
        }
        if next != [x, y]
            && next[1] < MAP.len()
            && next[0] < MAP[0].len()
            && MAP[next[1]].as_bytes()[next[0]] != b'#'
        {
            self.state.position = next;
            burst = true;
            if MAP[next[1]].as_bytes()[next[0]] == b'g' && self.random().is_multiple_of(3) {
                let species = Species::ALL[(self.random() % Species::ALL.len() as u64) as usize];
                let level = (self.random() % 4 + 1) as u32;
                self.state.wild = Some(Monster::of(species, level));
                self.state.encounters += 1;
            }
        }
        burst
    }
    pub(super) fn switch(&mut self, index: usize) -> bool {
        match self.state.collection.get_mut(index) {
            Some(monster) if monster.hp > 0 => {
                std::mem::swap(&mut self.state.starter, monster);
                true
            }
            _ => false,
        }
    }
    pub(super) fn heal(&mut self) {
        self.state.starter.hp = self.state.starter.max_hp();
        for monster in &mut self.state.collection {
            monster.hp = monster.max_hp();
        }
        self.state.balls = 10;
    }
    pub(super) fn reward(&mut self) {
        let starter = &mut self.state.starter;
        starter.xp = starter.xp.saturating_add(10);
        if starter.xp >= starter.level * 20 && starter.level < 100 {
            starter.xp -= starter.level * 20;
            starter.level += 1;
            starter.hp = starter.max_hp();
        }
        self.state.balls = (self.state.balls + 1).min(10);
    }
}

pub(super) fn damage(attacker: Monster, defender: Monster) -> u32 {
    let base = attacker.species.power() + attacker.level * 2;
    match (attacker.element(), defender.element()) {
        (Element::Fire, Element::Grass)
        | (Element::Grass, Element::Water)
        | (Element::Water, Element::Fire) => base * 2,
        (Element::Grass, Element::Fire)
        | (Element::Water, Element::Grass)
        | (Element::Fire, Element::Water) => base / 2,
        _ => base,
    }
}
