use super::*;
use crate::alloc::{arena::ArenaAllocator, heap::HeapAllocator, pool::PoolAllocator};

#[test]
fn walls_and_stationary_actions_do_not_trigger_encounters() {
    let mut game = Pokemon::<PoolAllocator>::new(Config::default());
    for action in [
        Action::Up,
        Action::Left,
        Action::Wait,
        Action::Attack,
        Action::Catch,
    ] {
        game.step(action);
        assert_eq!(game.state.position, [1, 1]);
        assert_eq!(game.state.encounters, 0);
    }
    game.step(Action::Right);
    assert_eq!(game.state.position, [2, 1]);
    game.state.starter.hp = 1;
    game.step(Action::Heal);
    assert_eq!(game.state.starter.hp, 1);
    game.step(Action::Left);
    game.step(Action::Heal);
    assert_eq!(game.state.starter.hp, game.state.starter.max_hp());
}

#[test]
fn battle_turns_type_advantage_and_leveling() {
    let mut game = Pokemon::<HeapAllocator>::new(Config::default());
    let wild = Monster::new(Element::Grass, 1);
    assert!(damage(game.state.starter, wild) > damage(wild, game.state.starter));
    game.state.wild = Some(wild);
    game.step(Action::Right);
    assert_eq!(game.state.position, [1, 1]);
    assert_eq!(game.state.wild, Some(wild));
    game.state.starter.xp = 10;
    for _ in 0..3 {
        game.step(Action::Attack);
    }
    assert!(game.state.wild.is_none());
    assert_eq!(game.state.wins, 1);
    assert_eq!(game.state.starter.level, 2);
    assert_eq!(game.state.starter.hp, game.state.starter.max_hp());
}

#[test]
fn defeat_returns_to_healing_center_and_run_ends_battle() {
    let mut game = Pokemon::<ArenaAllocator>::new(Config::default());
    game.state.position = [4, 1];
    game.state.starter.hp = 1;
    game.state.wild = Some(Monster::new(Element::Water, 4));
    game.step(Action::Attack);
    assert_eq!(game.state.losses, 1);
    assert_eq!(game.state.position, [1, 1]);
    assert!(game.state.wild.is_none());
    assert_eq!(game.state.starter.hp, game.state.starter.max_hp());
    game.state.wild = Some(Monster::new(Element::Water, 4));
    game.step(Action::Run);
    assert!(game.state.wild.is_none());
}

#[test]
fn catching_consumes_balls_and_party_is_bounded() {
    let mut game = Pokemon::<PoolAllocator>::new(Config::default());
    for _ in 0..100 {
        game.state.wild = Some(Monster {
            hp: 1,
            ..Monster::new(Element::Grass, 1)
        });
        game.state.balls = 10;
        game.step(Action::Catch);
        assert!(game.state.balls == 9 || game.state.balls == 10);
    }
    assert!(game.state.catches > 5);
    assert_eq!(game.state.collection.len(), 5);
    game.state.balls = 0;
    game.state.wild = Some(Monster::new(Element::Water, 1));
    let before = game.state.clone();
    game.step(Action::Catch);
    assert_eq!(game.state, before);
}

fn expedition<A: Allocator>() -> (Outcome, Vec<(usize, u64, u64)>) {
    let mut frames = Vec::new();
    let result = run::<A>(
        Config::default(),
        3600,
        Some(&mut |f| frames.push((f.live, f.wins, f.catches))),
    );
    (result, frames)
}
#[test]
fn all_allocators_produce_identical_gameplay_and_effects() {
    let (heap, trace) = expedition::<HeapAllocator>();
    let (pool, pool_frames) = expedition::<PoolAllocator>();
    let (arena, arena_frames) = expedition::<ArenaAllocator>();
    assert!(heap.state.encounters > 0 && heap.state.wins > 0 && heap.state.catches > 0);
    assert!(arena.reserved_bytes > pool.reserved_bytes);
    for (result, frames) in [(pool, pool_frames), (arena, arena_frames)] {
        assert_eq!(result.state, heap.state);
        assert_eq!(result.checksum, heap.checksum);
        assert_eq!(result.created, heap.created);
        assert_eq!(result.retired, heap.retired);
        assert_eq!(result.particle_updates, heap.particle_updates);
        assert_eq!(frames, trace);
        assert_eq!(result.created - result.retired, result.live as u64);
    }
    assert_eq!(
        run::<HeapAllocator>(Config::default(), 3600, None).checksum,
        heap.checksum
    );
    assert_ne!(
        run::<HeapAllocator>(
            Config {
                seed: 2,
                ..Config::default()
            },
            3600,
            None
        )
        .checksum,
        heap.checksum
    );
}

#[test]
fn benchmark_workload_keeps_arena_storage_above_the_pool() {
    let cfg = Config {
        trail: 64,
        ..Config::default()
    };
    let pool = run::<PoolAllocator>(cfg, 400, None);
    let arena = run::<ArenaAllocator>(cfg, 400, None);
    assert_eq!(pool.checksum, arena.checksum);
    assert!(arena.reserved_bytes > pool.reserved_bytes);
}

#[test]
fn particles_expire_and_arena_storage_is_bounded() {
    let mut game = Pokemon::<ArenaAllocator>::new(Config {
        lifetime: 2,
        burst: 4,
        ..Config::default()
    });
    game.step(Action::Right);
    assert_eq!(game.handles.len(), 4);
    game.step(Action::Wait);
    game.step(Action::Wait);
    assert!(game.handles.is_empty());
    assert_eq!(game.created, game.retired);
    let short = run::<ArenaAllocator>(Config::default(), 3600, None);
    let long = run::<ArenaAllocator>(Config::default(), 36000, None);
    assert_eq!(short.reserved_bytes, long.reserved_bytes);
}

#[test]
fn reserves_can_battle_and_switching_costs_a_turn() {
    let mut game = Pokemon::<PoolAllocator>::new(Config::default());
    game.state.collection.push(Monster::new(Element::Water, 2));
    let starter = game.state.starter;
    game.step(Action::Switch(0));
    assert_eq!(game.state.starter.element, Element::Water);
    assert_eq!(game.state.collection[0], starter);
    game.state.wild = Some(Monster::new(Element::Grass, 1));
    game.step(Action::Switch(0));
    assert_eq!(game.state.starter.element, Element::Fire);
    assert!(game.state.starter.hp < starter.hp);
    let before = game.state.clone();
    for action in [Action::Switch(99), Action::Release(0), Action::Heal] {
        game.step(action);
        assert_eq!(game.state, before);
    }
}

#[test]
fn fainted_active_uses_reserve_before_blackout() {
    let mut game = Pokemon::<PoolAllocator>::new(Config::default());
    game.state.starter.hp = 1;
    game.state.collection.push(Monster::new(Element::Grass, 1));
    game.state.wild = Some(Monster::new(Element::Water, 4));
    game.step(Action::Attack);
    assert_eq!(game.state.losses, 0);
    assert!(game.state.wild.is_some());
    assert_eq!(game.state.starter.element, Element::Grass);
    assert_eq!(game.state.collection[0].hp, 0);
    let before = game.state.clone();
    game.step(Action::Switch(0));
    assert_eq!(game.state, before);
    game.state.starter.hp = 1;
    game.step(Action::Attack);
    assert_eq!(game.state.losses, 1);
    assert!(game.state.wild.is_none());
    assert!(game.state.collection.iter().all(|m| m.hp == m.max_hp()));
}

#[test]
fn full_party_can_make_room_for_new_catches() {
    let mut game = Pokemon::<PoolAllocator>::new(Config::default());
    game.state.collection = vec![Monster::new(Element::Fire, 1); 5];
    game.step(Action::Release(2));
    assert_eq!(game.state.collection.len(), 4);
    let before = game.state.clone();
    game.step(Action::Release(4));
    assert_eq!(game.state, before);
}

#[test]
fn varied_player_actions_keep_state_valid_across_allocators() {
    let cfg = Config {
        burst: 2,
        lifetime: 3,
        ..Config::default()
    };
    let mut heap = Pokemon::<HeapAllocator>::new(cfg);
    let mut pool = Pokemon::<PoolAllocator>::new(cfg);
    let mut arena = Pokemon::<ArenaAllocator>::new(cfg);
    let actions = [
        Action::Up,
        Action::Down,
        Action::Left,
        Action::Right,
        Action::Attack,
        Action::Catch,
        Action::Run,
        Action::Heal,
        Action::Wait,
        Action::Switch(0),
        Action::Switch(4),
        Action::Release(0),
    ];
    for index in 0..20000 {
        let action = actions[splitmix64(index) as usize % actions.len()];
        heap.step(action);
        pool.step(action);
        arena.step(action);
        assert_eq!(heap.checksum(), pool.checksum());
        assert_eq!(heap.checksum(), arena.checksum());
        let s = heap.state();
        assert!(s.starter.hp > 0 && s.starter.hp <= s.starter.max_hp());
        assert!(s.balls <= 10 && s.collection.len() <= 5);
        assert_ne!(MAP[s.position[1]].as_bytes()[s.position[0]], b'#');
        assert!(s.wild.is_none_or(|m| m.hp > 0 && m.hp <= m.max_hp()));
    }
}

#[test]
fn save_restores_battle_party_rng_and_exact_continuation() {
    let path = std::env::temp_dir().join(format!("pokemon-roundtrip-{}.save", std::process::id()));
    let mut original = Pokemon::<HeapAllocator>::new(Config::default());
    for _ in 0..120 {
        original.step(original.auto_action());
    }
    original.state.wild = Some(Monster::new(Element::Water, 3));
    original
        .state
        .collection
        .push(Monster::new(Element::Grass, 2));
    original.state.collection.truncate(5);
    original.save(&path).unwrap();
    let mut restored = Pokemon::<ArenaAllocator>::load(&path).unwrap();
    assert_eq!(original.state(), restored.state());
    assert_eq!(original.checksum(), restored.checksum());
    for _ in 0..1000 {
        let action = original.auto_action();
        original.step(action);
        restored.step(action);
        assert_eq!(original.checksum(), restored.checksum());
    }
    restored.save(&path).unwrap();
    let pool = Pokemon::<PoolAllocator>::load(&path).unwrap();
    assert_eq!(restored.checksum(), pool.checksum());
    std::fs::write(&path, "POKEMON_SAVE_1\n1 32 0").unwrap();
    assert!(Pokemon::<PoolAllocator>::load(&path).is_err());
    std::fs::remove_file(path).unwrap();
}

#[test]
fn grass_encounters_have_varied_intervals_and_seeds() {
    fn intervals(seed: u64) -> Vec<usize> {
        let mut game = Pokemon::<PoolAllocator>::new(Config {
            seed,
            burst: 0,
            ..Config::default()
        });
        game.state.position = [4, 1];
        let mut gaps = Vec::new();
        let mut steps = 0;
        for _ in 0..300 {
            if game.state.wild.is_some() {
                game.step(Action::Run);
            }
            steps += 1;
            let x = game.state.position[0];
            game.step(if x == 4 { Action::Right } else { Action::Left });
            if game.state.wild.is_some() {
                gaps.push(steps);
                steps = 0;
            }
        }
        gaps
    }
    let a = intervals(1);
    assert!(a.len() > 20);
    assert!(a.iter().any(|&gap| gap > 1));
    assert!(a.windows(2).any(|pair| pair[0] != pair[1]));
    assert_eq!(a, intervals(1));
    assert_ne!(a, intervals(2));
}

#[test]
fn recorded_resumed_session_matches_live_and_rejects_wrong_checksum() {
    let mut live = Pokemon::<PoolAllocator>::new(Config::default());
    for _ in 0..100 {
        live.step(live.auto_action());
    }
    let initial = live.snapshot();
    let mut actions = Vec::new();
    for _ in 0..500 {
        let action = live.auto_action();
        actions.push(action);
        live.step(action);
    }
    let mut bytes = Vec::new();
    Replay {
        initial,
        actions,
        checksum: live.checksum(),
    }
    .write(&mut bytes)
    .unwrap();
    let mut recording = Replay::read(bytes.as_slice()).unwrap();
    let heap = replay::<HeapAllocator>(&recording, None).unwrap();
    let pool = replay::<PoolAllocator>(&recording, None).unwrap();
    let mut samples = Vec::new();
    let arena = replay::<ArenaAllocator>(&recording, Some(&mut |f| samples.push(f))).unwrap();
    for outcome in [&heap, &pool, &arena] {
        assert_eq!(outcome.state, *live.state());
        assert_eq!(outcome.checksum, live.checksum());
        assert_eq!(outcome.created, heap.created);
        assert_eq!(outcome.retired, heap.retired);
        assert_eq!(outcome.particle_updates, heap.particle_updates);
        assert_eq!(
            outcome.initial_live as u64 + outcome.created - outcome.retired,
            outcome.live as u64
        );
    }
    assert_eq!(samples.len(), recording.actions.len());
    recording.checksum ^= 1;
    assert!(replay::<PoolAllocator>(&recording, None).is_err());
    assert!(Replay::read(b"POKEMON_REPLAY_1\n1\n999999\n".as_slice()).is_err());
    assert!(Replay::read(b"not a recording".as_slice()).is_err());
}

#[test]
fn continuous_trail_matches_pong_particle_lifecycle() {
    fn check<A: Allocator>() {
        let cfg = Config {
            trail: 4,
            burst: 0,
            lifetime: 3,
            ..Config::default()
        };
        let mut pokemon = Pokemon::<A>::new(cfg);
        let mut pong = crate::pong::Pong::<A>::new(&crate::pong::PongConfig {
            trail: cfg.trail,
            burst: 0,
            particle_lifetime: cfg.lifetime,
            dt: cfg.dt,
            seed: cfg.seed,
        });
        pong.set_ball(crate::pong::Ball {
            pos: [512.0, 384.0],
            vel: [0.0, 0.0],
        });
        for _ in 0..100 {
            pokemon.step(Action::Wait);
            pong.step(crate::pong::Input::default());
            assert_eq!(pokemon.alloc.live_count(), pong.live_particles());
            assert_eq!(pokemon.handles.len(), pokemon.alloc.live_count());
        }
        assert_eq!(pokemon.handles.len(), 12);
        pokemon.state.wild = Some(Monster::new(Element::Grass, 1));
        pokemon.step(Action::Run);
        assert_eq!(pokemon.handles.len(), 4);
        assert_eq!(pokemon.created - pokemon.retired, 4);
    }
    check::<HeapAllocator>();
    check::<PoolAllocator>();
    check::<ArenaAllocator>();
}
