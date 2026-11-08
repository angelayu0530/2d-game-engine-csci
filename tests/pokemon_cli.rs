use std::process::Command;

#[test]
fn benchmark_cli_matches_across_allocators() {
    let mut checksums = Vec::new();
    for allocator in ["heap", "pool", "arena"] {
        let output = Command::new(env!("CARGO_BIN_EXE_pokemon"))
            .args(["--allocator", allocator, "--frames", "300", "--trials", "2"])
            .output()
            .unwrap();
        assert!(output.status.success());
        let csv = String::from_utf8(output.stdout).unwrap();
        let mut lines = csv.lines();
        let header: Vec<_> = lines.next().unwrap().split(',').collect();
        let index = header.iter().position(|&name| name == "checksum").unwrap();
        let rows: Vec<_> = lines.collect();
        assert_eq!(rows.len(), 2);
        for row in rows {
            let fields: Vec<_> = row.split(',').collect();
            assert_eq!(fields.len(), header.len());
            checksums.push(fields[index].to_owned());
        }
    }
    assert!(checksums.iter().all(|value| value == &checksums[0]));
}

#[test]
fn rejects_zero_lifetime() {
    let output = Command::new(env!("CARGO_BIN_EXE_pokemon"))
        .args(["--allocator", "pool", "--lifetime", "0"])
        .output()
        .unwrap();
    assert!(!output.status.success());
}

#[test]
fn million_particle_workload_is_identical_on_all_allocators() {
    let mut expected = None;
    for allocator in ["heap", "pool", "arena"] {
        let output = Command::new(env!("CARGO_BIN_EXE_pokemon"))
            .args([
                "--allocator",
                allocator,
                "--frames",
                "3",
                "--trail",
                "0",
                "--burst",
                "1000000",
                "--lifetime",
                "1",
            ])
            .output()
            .unwrap();
        assert!(output.status.success());
        let csv = String::from_utf8(output.stdout).unwrap();
        let mut lines = csv.lines();
        let header: Vec<_> = lines.next().unwrap().split(',').collect();
        let row: Vec<_> = lines.next().unwrap().split(',').collect();
        let field = |name| row[header.iter().position(|&h| h == name).unwrap()];
        assert_eq!(field("peak_live"), "1000000");
        assert_eq!(field("created"), "3000000");
        assert_eq!(field("retired"), "2000000");
        assert_eq!(field("particle_updates"), "2000000");
        let checksum = field("checksum").to_owned();
        if let Some(expected) = &expected {
            assert_eq!(&checksum, expected);
        } else {
            expected = Some(checksum);
        }
    }
}

#[test]
fn replay_can_scale_effects_but_rejects_corrupt_recordings() {
    use game_engine::{
        alloc::pool::PoolAllocator,
        pokemon::{Config, Pokemon, Replay},
    };
    let mut game = Pokemon::<PoolAllocator>::new(Config::default());
    let initial = game.snapshot();
    let mut actions = Vec::new();
    for _ in 0..30 {
        let action = game.auto_action();
        actions.push(action);
        game.step(action);
    }
    let mut recording = Replay {
        initial,
        actions,
        checksum: game.checksum(),
    };
    let path =
        std::env::temp_dir().join(format!("pokemon-scaled-{}.pokemonrec", std::process::id()));
    recording
        .write(std::fs::File::create(&path).unwrap())
        .unwrap();
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_pokemon"))
            .args(["--allocator", "pool", "--particles", "10000", "--replay"])
            .arg(&path)
            .output()
            .unwrap()
    };
    let output = run();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    recording.checksum ^= 1;
    recording
        .write(std::fs::File::create(&path).unwrap())
        .unwrap();
    assert!(!run().status.success());
    std::fs::remove_file(path).unwrap();
}
