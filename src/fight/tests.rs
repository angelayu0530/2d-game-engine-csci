use super::{
    BOSS_HEARTS, Fight, FightConfig, Input, KNIGHT_HEARTS, POTIONS, Replay, SHOT_COOLDOWN, Status,
    run,
};
use crate::{
    alloc::{Allocator, arena::ArenaAllocator, heap::HeapAllocator, pool::PoolAllocator},
    particle::Particle,
};

fn cfg(particles: usize) -> FightConfig {
    FightConfig {
        particles,
        ..FightConfig::default()
    }
}

/// A player who circles the room, shoots, sprays, and drinks now and then.
fn scripted(frames: usize) -> Vec<Input> {
    (0..frames)
        .map(|i| {
            let leg = i / 45 % 8;
            Input {
                up: matches!(leg, 0 | 1 | 7),
                down: matches!(leg, 3..=5),
                left: matches!(leg, 5..=7),
                right: matches!(leg, 1..=3),
                attack: i % 30 < 12,
                heal: i % 400 == 399,
                spray: i % 200 < 40,
                restart: i % 500 == 0,
            }
        })
        .collect()
}

#[test]
fn input_bits_round_trip() {
    for bits in 0..=u8::MAX {
        assert_eq!(Input::from_bits(bits).bits(), bits);
    }
}

#[test]
fn diagonal_movement_is_as_fast_as_straight() {
    let mut straight = Fight::<PoolAllocator>::new(&cfg(0));
    let mut diagonal = Fight::<PoolAllocator>::new(&cfg(0));
    let start = straight.knight().pos;
    straight.step(Input {
        right: true,
        ..Input::default()
    });
    diagonal.step(Input {
        right: true,
        up: true,
        ..Input::default()
    });
    let moved = |fight: &Fight<PoolAllocator>| {
        let p = fight.knight().pos;
        (p[0] - start[0]).hypot(p[1] - start[1])
    };
    assert!((moved(&straight) - moved(&diagonal)).abs() < 1e-4);
    assert!(diagonal.knight().pos[1] < start[1], "up is toward the top");
}

/// Knight on the left facing right, boss 200px away.
fn duel(particles: usize) -> Fight<HeapAllocator> {
    let mut fight = Fight::new(&cfg(particles));
    fight.knight.pos = [100.0, 176.0];
    fight.knight.facing = [1.0, 0.0];
    fight.boss.pos = [300.0, 176.0];
    fight
}

#[test]
fn a_held_shot_fires_once_per_cooldown_and_the_arrow_costs_one_heart() {
    let mut fight = duel(0);
    let shoot = Input {
        attack: true,
        ..Input::default()
    };
    for _ in 0..SHOT_COOLDOWN {
        fight.step(shoot);
    }
    assert_eq!(fight.arrows().len(), 1, "one shot per cooldown");
    for _ in 0..60 {
        fight.knight.invulnerable = 1_000;
        fight.step(Input::default());
    }
    assert!(fight.arrows().is_empty(), "arrow consumed");
    assert_eq!(fight.boss().hp, BOSS_HEARTS - 1);
}

#[test]
fn healing_spends_a_potion_only_when_hurt() {
    let mut fight = Fight::<PoolAllocator>::new(&cfg(0));
    let heal = Input {
        heal: true,
        ..Input::default()
    };
    fight.step(heal);
    assert_eq!(fight.knight().potions, POTIONS, "full health");

    fight.knight.hp = KNIGHT_HEARTS - 1;
    fight.step(heal);
    assert_eq!(fight.knight().hp, KNIGHT_HEARTS, "capped at max");
    assert_eq!(fight.knight().potions, POTIONS - 1);
}

#[test]
fn slaying_the_boss_wins_and_restart_brings_it_back() {
    let mut fight = duel(100);
    fight.boss.hp = 1;
    for _ in 0..60 {
        fight.knight.invulnerable = 1_000;
        fight.step(Input {
            attack: true,
            ..Input::default()
        });
    }
    assert_eq!(fight.status(), Status::Victory);
    assert!(fight.bolts().is_empty() && fight.arrows().is_empty());
    fight.step(Input {
        restart: true,
        ..Input::default()
    });
    assert_eq!(fight.status(), Status::Fighting);
    assert_eq!(fight.boss().hp, BOSS_HEARTS);
    assert_eq!(fight.victories, 1);
}

#[test]
fn an_idle_knight_eventually_dies() {
    let mut fight = Fight::<PoolAllocator>::new(&cfg(0));
    for _ in 0..60 * 60 {
        fight.step(Input::default());
    }
    assert_eq!(fight.status(), Status::Defeat);
    assert_eq!(fight.knight().hp, 0);
}

#[test]
fn every_allocator_plays_the_same_fight() {
    let inputs = scripted(1_500);
    let heap = run::<HeapAllocator>(&cfg(3_000), &inputs, None);
    let pool = run::<PoolAllocator>(&cfg(3_000), &inputs, None);
    let arena = run::<ArenaAllocator>(&cfg(3_000), &inputs, None);
    assert!(heap.boss_hp < BOSS_HEARTS, "script lands hits: {heap:?}");
    for other in [pool, arena] {
        assert_eq!(other.checksum, heap.checksum);
        assert_eq!(other.created, heap.created);
        assert_eq!(other.retired, heap.retired);
        assert_eq!(other.particle_updates, heap.particle_updates);
        assert_eq!(other.peak_live, heap.peak_live);
    }
}

#[test]
fn particle_load_never_changes_gameplay() {
    let inputs = scripted(1_500);
    let light = run::<PoolAllocator>(&cfg(10), &inputs, None);
    let heavy = run::<PoolAllocator>(&cfg(50_000), &inputs, None);
    assert_eq!(light.game_checksum, heavy.game_checksum);
    assert_eq!(
        (light.knight_hp, light.boss_hp),
        (heavy.knight_hp, heavy.boss_hp)
    );
    assert_ne!(light.checksum, heavy.checksum);
}

#[test]
fn live_particles_never_drop_below_the_target() {
    let target = 5_000;
    let mut fight = Fight::<PoolAllocator>::new(&cfg(target));
    for input in scripted(600) {
        fight.step(input);
        assert!(fight.live_particles() >= target);
    }
    assert!(fight.peak_live < target * 2, "peak {}", fight.peak_live);
}

fn reserved_after<A: Allocator>(frames: usize) -> usize {
    let mut fight = Fight::<A>::new(&cfg(5_000));
    let mut peak = 0;
    for input in scripted(frames) {
        fight.step(input);
        peak = peak.max(fight.effects().reserved_bytes());
    }
    peak
}

#[test]
fn arena_storage_is_recycled_between_generations() {
    // Without generation reclaim an arena keeps every particle ever created:
    // over 40x the target after 3000 steps.
    let bytes = reserved_after::<ArenaAllocator>(3_000);
    let bound = 12 * 5_000 * size_of::<Particle>();
    assert!(bytes < bound, "{bytes} reserved, bound {bound}");
}

#[test]
fn recording_round_trips_and_rejects_foreign_files() {
    let recorded = Replay {
        cfg: FightConfig {
            particles: 1_000_000,
            seed: 7,
            ..FightConfig::default()
        },
        inputs: scripted(900),
        game_checksum: 42,
    };
    let mut bytes = Vec::new();
    recorded.write(&mut bytes).unwrap();
    assert!(bytes.len() < 900 * 3, "run-length encoded");
    assert_eq!(Replay::read(bytes.as_slice()).unwrap(), recorded);

    assert!(Replay::read(&b"pong-replay v1\ndt=0.1\n"[..]).is_err());
    assert!(Replay::read(&b"fight-replay v1\nparticles=1 game=1\nzz 3\n"[..]).is_err());
    assert!(Replay::read(&b"fight-replay v1\nparticles=1\n00 3\n"[..]).is_err());
}
