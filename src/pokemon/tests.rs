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
        if game.state.pending.is_some() {
            break;
        }
        game.state.wild = Some(Monster {
            hp: 1,
            ..Monster::new(Element::Grass, 1)
        });
        game.state.balls = 10;
        let catches = game.state.catches;
        game.step(Action::Catch);
        assert!(game.state.balls == 9 || game.state.balls == 10);
        if game.state.collection.len() < 5 && game.state.catches > catches {
            assert_eq!(game.state.collection.len() as u64, game.state.catches);
            assert!(game.state.pending.is_none());
        }
    }
    assert_eq!(game.state.collection.len(), 5);
    assert_eq!(game.state.catches, 5);
    assert_eq!(game.state.pending.unwrap().species, Species::Bulbasaur);

    let catches = game.state.catches;
    game.step(Action::Run);
    assert!(game.state.pending.is_none());
    assert_eq!(game.state.catches, catches);
    assert_eq!(game.state.collection.len(), 5);

    for _ in 0..100 {
        if game.state.pending.is_some() {
            break;
        }
        game.state.wild = Some(Monster {
            hp: 1,
            ..Monster::of(Species::Psyduck, 1)
        });
        game.state.balls = 10;
        game.step(Action::Catch);
    }
    let pending = game.state.pending.unwrap();
    assert_eq!(pending.species, Species::Psyduck);
    assert_eq!(game.state.collection.len(), 5);
    assert_eq!(game.state.catches, catches);

    game.step(Action::Release(0));
    assert!(game.state.pending.is_none());
    assert_eq!(game.state.collection.len(), 5);
    assert_eq!(game.state.catches, catches + 1);
    assert_eq!(game.state.collection[4], pending);

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
    assert_eq!(game.state.starter.element(), Element::Water);
    assert_eq!(game.state.collection[0], starter);
    game.state.wild = Some(Monster::new(Element::Grass, 1));
    game.step(Action::Switch(0));
    assert_eq!(game.state.starter.element(), Element::Fire);
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
    assert_eq!(game.state.starter.element(), Element::Grass);
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
        assert!(s.pending.is_none_or(|m| m.hp > 0 && m.hp <= m.max_hp()));
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
fn grass_encounters_span_several_species() {
    let mut game = Pokemon::<PoolAllocator>::new(Config {
        seed: 1,
        burst: 0,
        ..Config::default()
    });
    game.state.position = [4, 1];
    let mut seen = [false; Species::ALL.len()];
    for _ in 0..300 {
        if let Some(wild) = game.state.wild {
            assert_eq!(wild, Monster::of(wild.species, wild.level));
            seen[wild.species as usize] = true;
            game.step(Action::Run);
        }
        let x = game.state.position[0];
        game.step(if x == 4 { Action::Right } else { Action::Left });
    }
    if let Some(wild) = game.state.wild {
        assert_eq!(wild, Monster::of(wild.species, wild.level));
        seen[wild.species as usize] = true;
    }
    assert!(seen.iter().filter(|flag| **flag).count() >= 4);
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

#[test]
fn expired_effect_batches_reuse_arena_storage() {
    let cfg = Config {
        burst: 10_000,
        trail: 0,
        lifetime: 1,
        ..Config::default()
    };
    let mut arena = Pokemon::<ArenaAllocator>::new(cfg);
    let mut pool = Pokemon::<PoolAllocator>::new(cfg);
    for _ in 0..100 {
        arena.effects(cfg.burst);
        pool.effects(cfg.burst);
        assert_eq!(arena.checksum(), pool.checksum());
        assert_eq!(
            arena.alloc.reserved_bytes(),
            cfg.burst * std::mem::size_of::<Particle>()
        );
        arena.age();
        pool.age();
        assert_eq!(arena.live_particles(), 0);
    }
    assert_eq!(arena.created, arena.retired);
}

#[test]
fn gameplay_is_independent_of_effect_load_and_allocator_each_turn() {
    for seed in [1, 42, 2026] {
        let mut light = Pokemon::<HeapAllocator>::new(Config {
            seed,
            burst: 0,
            trail: 0,
            ..Config::default()
        });
        let heavy = Config {
            seed,
            burst: 128,
            trail: 64,
            lifetime: 7,
            dt: 1.0 / 30.0,
        };
        let mut pool = Pokemon::<PoolAllocator>::new(heavy);
        let mut arena = Pokemon::<ArenaAllocator>::new(heavy);
        for _ in 0..1200 {
            let action = light.auto_action();
            assert_eq!(pool.auto_action(), action);
            assert_eq!(arena.auto_action(), action);
            light.step(action);
            pool.step(action);
            arena.step(action);
            assert_eq!(light.state(), pool.state());
            assert_eq!(light.game_checksum(), pool.game_checksum());
            assert_eq!(light.game_checksum(), arena.game_checksum());
            assert_eq!(pool.checksum(), arena.checksum());
        }
        assert_ne!(light.checksum(), pool.checksum());
    }
}

#[test]
fn invalid_battle_actions_preserve_gameplay_but_advance_the_turn() {
    let mut game = Pokemon::<PoolAllocator>::new(Config::default());
    game.state.wild = Some(Monster::of(Species::Oddish, 2));
    game.state.balls = 0;
    game.state.collection.push(Monster {
        hp: 0,
        ..Monster::of(Species::Psyduck, 1)
    });
    let state = game.state().clone();
    let rng = game.rng;
    for action in [
        Action::Catch,
        Action::Heal,
        Action::Switch(0),
        Action::Switch(99),
        Action::Release(0),
        Action::Up,
    ] {
        let frame = game.frame;
        game.step(action);
        assert_eq!(game.state(), &state);
        assert_eq!(game.rng, rng);
        assert_eq!(game.frame, frame + 1);
    }
}

#[test]
fn scaled_replay_preserves_resumed_gameplay_and_observers_do_not_change_results() {
    let cfg = Config::default();
    let mut game = Pokemon::<PoolAllocator>::new(cfg);
    let mut explorer = autoplay::Explorer::default();
    for _ in 0..40 {
        game.step(explorer.action(game.state()));
    }
    let initial = game.snapshot();
    let mut actions = Vec::new();
    for _ in 0..120 {
        let action = explorer.action(game.state());
        actions.push(action);
        game.step(action);
    }
    let recording = Replay {
        initial,
        actions,
        checksum: game.checksum(),
    };
    let reference = replay::<PoolAllocator>(&recording, None).unwrap();
    for target in [10, 10_000] {
        let mut observed_time = Duration::ZERO;
        let mut frames = 0;
        let watched = benchmark_session::<ArenaAllocator>(
            cfg,
            0,
            Some(&recording),
            Some(target),
            Some(&mut |state, frame| {
                assert_eq!(state.live_particles(), target);
                assert_eq!(frame.live, target);
                observed_time += frame.step;
                frames += 1;
            }),
        )
        .unwrap();
        let headless =
            benchmark_session::<PoolAllocator>(cfg, 0, Some(&recording), Some(target), None)
                .unwrap();
        assert_eq!(frames, 120);
        assert_eq!(watched.elapsed, observed_time);
        assert_eq!(watched.checksum, headless.checksum);
        assert_eq!(watched.game_checksum, reference.game_checksum);
        assert_eq!(watched.peak_live, target);
    }
}
