use std::io::{self, BufRead, Write};

use super::{Action, Config, MAP, Pokemon, State, describe};
use crate::alloc::Allocator;

const HELP: &str = "Commands (press Enter after each):
  w/up, s/down, a/left, d/right : move one tile
  attack/f : attack     catch/c : throw a ball     run/r : flee
  heal/h : restore party and balls at H
  switch 1..5 : send out a reserve monster (costs a turn in battle)
  release 1..5 : release a reserve outside battle (makes room for new catches)
  party/p : inspect your party     map/m : show the map
  look : describe active and wild Pokemon     save : save progress
  wait : pass time     help/? : commands     quit/q : exit
Map: @ you, # wall, g tall grass, H healing center, . path.
Goal: build a party containing Fire, Grass, and Water. Continue exploring afterward.
Type advantages: Fire beats Grass, Grass beats Water, Water beats Fire.";

fn action(command: &str) -> Option<Action> {
    match command {
        "w" | "up" => Some(Action::Up),
        "s" | "down" => Some(Action::Down),
        "a" | "left" => Some(Action::Left),
        "d" | "right" => Some(Action::Right),
        "attack" | "f" => Some(Action::Attack),
        "catch" | "c" => Some(Action::Catch),
        "run" | "r" => Some(Action::Run),
        "heal" | "h" => Some(Action::Heal),
        "wait" => Some(Action::Wait),
        _ if command.starts_with("release ") => command
            .strip_prefix("release ")
            .and_then(|n| n.trim().parse::<usize>().ok())
            .filter(|&n| (1..=5).contains(&n))
            .map(|n| Action::Release(n - 1)),
        _ => command
            .strip_prefix("switch ")
            .and_then(|n| n.trim().parse::<usize>().ok())
            .filter(|&n| (1..=5).contains(&n))
            .map(|n| Action::Switch(n - 1)),
    }
}

fn map(out: &mut impl Write, state: &State) -> io::Result<()> {
    for (y, row) in MAP.iter().enumerate() {
        for (x, tile) in row.bytes().enumerate() {
            write!(
                out,
                "{}",
                if state.position == [x, y] {
                    '@'
                } else {
                    tile as char
                }
            )?;
        }
        writeln!(out)?;
    }
    Ok(())
}

fn status(out: &mut impl Write, state: &State) -> io::Result<()> {
    let active = state.starter;
    writeln!(
        out,
        "Active {} ({:?}) Lv{} | HP {}/{} | XP {}/{} | Balls {} | Wins {} | Catches {}",
        active.name(),
        active.element,
        active.level,
        active.hp,
        active.max_hp(),
        active.xp,
        active.level * 20,
        state.balls,
        state.wins,
        state.catches
    )?;
    if let Some(wild) = state.wild {
        writeln!(
            out,
            "Wild {} ({:?}) Lv{} | HP {}/{} | attack / catch / switch N / run",
            wild.name(),
            wild.element,
            wild.level,
            wild.hp,
            wild.max_hp()
        )?;
    }
    Ok(())
}

fn feedback(out: &mut impl Write, before: &State, after: &State, action: Action) -> io::Result<()> {
    for line in describe(before, after, action) {
        writeln!(out, "{line}")?;
    }
    Ok(())
}

pub fn play<A: Allocator>(
    cfg: Config,
    input: &mut impl BufRead,
    out: &mut impl Write,
) -> io::Result<()> {
    let mut game = Pokemon::<A>::new(cfg);
    session(&mut game, None, input, out)
}

pub fn session<A: Allocator>(
    game: &mut Pokemon<A>,
    save_path: Option<&std::path::Path>,
    input: &mut impl BufRead,
    out: &mut impl Write,
) -> io::Result<()> {
    recorded_session(game, save_path, None, input, out)
}

pub fn recorded_session<A: Allocator>(
    game: &mut Pokemon<A>,
    save_path: Option<&std::path::Path>,
    record_path: Option<&std::path::Path>,
    input: &mut impl BufRead,
    out: &mut impl Write,
) -> io::Result<()> {
    let initial = record_path.map(|_| game.snapshot());
    let mut actions = Vec::new();
    writeln!(
        out,
        "MONSTER TRAIL - Pokemon-style terminal adventure\n{HELP}"
    )?;
    map(out, game.state())?;
    writeln!(out, "{}", game.state().starter.description())?;
    let mut celebrated = game.state().has_every_element();
    loop {
        status(out, game.state())?;
        write!(out, "> ")?;
        out.flush()?;
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            writeln!(out, "\nGoodbye!")?;
            break;
        }
        let command = line.trim().to_ascii_lowercase();
        match command.as_str() {
            "q" | "quit" => {
                writeln!(out, "Goodbye!")?;
                break;
            }
            "save" => {
                if let Some(path) = save_path {
                    game.save(path)?;
                    writeln!(out, "Progress saved.")?;
                } else {
                    writeln!(out, "Saving is disabled for this session.")?;
                }
            }
            "look" => {
                writeln!(out, "{}", game.state().starter.description())?;
                if let Some(wild) = game.state().wild {
                    writeln!(out, "{}", wild.description())?;
                }
            }
            "help" | "?" => writeln!(out, "{HELP}")?,
            "map" | "m" => map(out, game.state())?,
            "party" | "p" => {
                writeln!(out, "Active: {}", game.state().starter.description())?;
                for (i, monster) in game.state().collection.iter().enumerate() {
                    writeln!(
                        out,
                        "Reserve {}: {} ({:?}) Lv{} HP {}/{}",
                        i + 1,
                        monster.name(),
                        monster.element,
                        monster.level,
                        monster.hp,
                        monster.max_hp()
                    )?;
                }
                for monster in &game.state().collection {
                    writeln!(out, "{}", monster.description())?;
                }
                if game.state().collection.is_empty() {
                    writeln!(out, "No reserves yet. Catch wild monsters in tall grass.")?;
                }
            }
            "" => {}
            _ => match action(&command) {
                Some(action) => {
                    let before = game.state().clone();
                    game.step(action);
                    if record_path.is_some() {
                        actions.push(action);
                    }
                    feedback(out, &before, game.state(), action)?;
                    if game.state().wild.is_none() {
                        map(out, game.state())?;
                    }
                    if !celebrated && game.state().has_every_element() {
                        writeln!(
                            out,
                            "Goal complete! Your party contains all three elements. Keep exploring!"
                        )?;
                        celebrated = true;
                    }
                }
                None => writeln!(out, "Unknown command. Type help for controls.")?,
            },
        }
    }
    if let Some(path) = save_path {
        game.save(path)?;
        writeln!(out, "Progress saved to {}.", path.display())?;
    }
    if let (Some(path), Some(initial)) = (record_path, initial) {
        super::Replay {
            initial,
            actions,
            checksum: game.checksum(),
        }
        .write(std::io::BufWriter::new(std::fs::File::create(path)?))?;
        writeln!(out, "Recorded actions to {}.", path.display())?;
    }
    writeln!(out, "Final checksum: {}", game.checksum())?;
    Ok(())
}
