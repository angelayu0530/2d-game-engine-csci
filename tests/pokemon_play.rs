use std::{
    io::Write,
    process::{Command, Stdio},
};

fn play(allocator: &str, commands: &str) -> String {
    let mut child = Command::new(env!("CARGO_BIN_EXE_pokemon-play"))
        .args([
            "--allocator",
            allocator,
            "--no-save",
            "--seed",
            "1",
            "--trail",
            "0",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(commands.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn terminal_commands_map_validation_and_eof_work() {
    let output = play("pool", "up\nheal\nright\nheal\nparty\nswitch 99\nmap\n");
    for message in [
        "#@..gggg...#",
        "A wall blocks",
        "Party healed",
        "Moved to 2, 1",
        "Return to H",
        "No reserves yet",
        "Unknown command",
        "Goodbye!",
    ] {
        assert!(output.contains(message), "Missing {message}: {output}");
    }
    let inspect = play("pool", "help\nparty\nmap\nwrong\nquit\n");
    let quit = play("pool", "quit\n");
    assert_eq!(inspect.lines().last(), quit.lines().last());
}

#[test]
fn actual_terminal_can_explore_encounter_battle_catch_and_flee() {
    use game_engine::{
        alloc::pool::PoolAllocator,
        pokemon::{Action, Config, Pokemon},
    };
    let mut game = Pokemon::<PoolAllocator>::new(Config::default());
    let mut commands = String::new();
    let mut fled = false;
    let mut won = false;
    for _ in 0..4_000 {
        if fled && won && game.state().catches > 0 && game.state().wild.is_none() {
            break;
        }
        let action = if let Some(wild) = game.state().wild {
            if !fled {
                fled = true;
                Action::Run
            } else if !won || wild.hp * 2 > wild.max_hp() {
                Action::Attack
            } else {
                Action::Catch
            }
        } else {
            game.auto_action()
        };
        commands.push_str(match action {
            Action::Right => "right\n",
            Action::Left => "left\n",
            Action::Up => "up\n",
            Action::Down => "down\n",
            Action::Attack => "attack\n",
            Action::Catch => "catch\n",
            Action::Run => "run\n",
            other => panic!("unexpected auto action {other:?}"),
        });
        let wins = game.state().wins;
        game.step(action);
        won |= game.state().wins > wins;
    }
    commands.push_str("party\nquit\n");
    let heap = play("heap", &commands);
    assert!(!heap.contains("Caught it! Gained 10 XP and a reward ball.\nThere is no wild monster"));
    assert_eq!(heap, play("pool", &commands));
    assert_eq!(heap, play("arena", &commands));
    for message in [
        "A wild monster appeared",
        "Your attack dealt",
        "Caught it!",
        "Wild monster defeated",
        "Escaped safely",
        "Reserve 1",
    ] {
        assert!(heap.contains(message), "Missing {message}");
    }
    assert!(heap.contains(&format!("Final checksum: {}", game.checksum())));
}

#[test]
fn quit_saves_and_next_process_resumes_without_overwriting_bad_saves() {
    let path = std::env::temp_dir().join(format!("pokemon-cli-{}.save", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let launch = |commands: &str| {
        let mut child = Command::new(env!("CARGO_BIN_EXE_pokemon-play"))
            .args(["--save", path.to_str().unwrap(), "--seed", "1"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(commands.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    };
    let first = launch("right\nright\nright\nright\nleft\nattack\nlook\nsave\nquit\n");
    assert!(first.status.success());
    let first = String::from_utf8(first.stdout).unwrap();
    assert!(first.contains("Bulbasaur"));
    assert!(first.contains("plant bulb"));
    assert!(first.contains("Progress saved"));
    let second = launch("quit\n");
    assert!(second.status.success());
    let second = String::from_utf8(second.stdout).unwrap();
    assert_eq!(first.lines().last(), second.lines().last());
    assert!(second.contains("Wild Bulbasaur"));
    assert!(launch("").status.success()); // EOF saves too.
    std::fs::write(&path, "broken save").unwrap();
    assert!(!launch("quit\n").status.success());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "broken save");
    std::fs::remove_file(path).unwrap();
}

#[test]
fn terminal_recording_runs_through_benchmark_cli() {
    let path = std::env::temp_dir().join(format!("pokemon-record-{}.rec", std::process::id()));
    let mut child = Command::new(env!("CARGO_BIN_EXE_pokemon-play"))
        .args([
            "--no-save",
            "--seed",
            "1",
            "--record",
            path.to_str().unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"right\nright\nright\nright\nleft\nattack\nlook\nquit\n")
        .unwrap();
    let played = child.wait_with_output().unwrap();
    assert!(played.status.success());
    let played = String::from_utf8(played.stdout).unwrap();
    for allocator in ["heap", "pool", "arena"] {
        let output = Command::new(env!("CARGO_BIN_EXE_pokemon"))
            .args(["--allocator", allocator, "--replay", path.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(output.status.success(), "{:?}", output);
        let csv = String::from_utf8(output.stdout).unwrap();
        let mut lines = csv.lines();
        let header: Vec<_> = lines.next().unwrap().split(',').collect();
        let row: Vec<_> = lines.next().unwrap().split(',').collect();
        let field = |name| row[header.iter().position(|&h| h == name).unwrap()];
        assert_eq!(field("frames"), "6"); // look and quit are not simulation actions
        assert_eq!(field("mode"), "pokemon-replay");
        assert!(played.contains(&format!("Final checksum: {}", field("checksum"))));
        let ns = |name| field(name).parse::<u128>().unwrap();
        assert_eq!(
            ns("total_ns"),
            ns("setup_ns") + ns("sim_ns") + ns("teardown_ns")
        );
    }
    let output = Command::new(env!("CARGO_BIN_EXE_pokemon"))
        .args([
            "--allocator",
            "pool",
            "--replay",
            path.to_str().unwrap(),
            "--seed",
            "3",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    std::fs::remove_file(path).unwrap();
}
