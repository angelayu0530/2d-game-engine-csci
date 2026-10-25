use std::io::{self, BufRead, Write};

use super::{Action, Config, MAP, Pokemon, State};
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
    if before.position != after.position && after.losses == before.losses {
        writeln!(
            out,
            "Moved to {}, {}.",
            after.position[0], after.position[1]
        )?;
    }
    if after.encounters > before.encounters {
        writeln!(out, "A wild monster appeared!")?;
        if let Some(wild) = after.wild {
            writeln!(out, "{}", wild.description())?;
        }
    }
    if after.wins > before.wins {
        writeln!(out, "Wild monster defeated! Gained 10 XP and a ball.")?;
    }
    if after.catches > before.catches {
        writeln!(out, "Caught it! Gained 10 XP and a reward ball.")?;
        if before.collection.len() == 5 {
            writeln!(out, "Party full: this catch was released.")?;
        }
    }
    if after.losses > before.losses {
        writeln!(
            out,
            "Your party fainted. Returned to H; party healed and balls restocked."
        )?;
    }
    if before.starter.element == after.starter.element && after.starter.level > before.starter.level
    {
        writeln!(out, "Level up! HP restored.")?;
    }
    match action {
        Action::Attack if before.wild.is_some() => {
            let wild = before.wild.unwrap();
            let hp = after.wild.map_or(0, |m| m.hp);
            if after.losses == before.losses {
                writeln!(
                    out,
                    "Your attack dealt {} damage.",
                    wild.hp.saturating_sub(hp)
                )?;
            }
        }
        Action::Catch if before.wild.is_some() && before.balls == 0 => {
            writeln!(out, "No balls left. Run and return to H to restock.")?;
        }
        Action::Catch if before.wild.is_some() && before.catches == after.catches => {
            writeln!(out, "It broke free! Try weakening it first.")?;
        }
        Action::Run if before.wild.is_some() => {
            writeln!(out, "Escaped safely.")?;
        }
        Action::Heal if before.wild.is_some() => {
            writeln!(out, "You cannot heal during battle.")?;
        }
        Action::Heal if before.position == [1, 1] => {
            writeln!(out, "Party healed and balls restocked.")?;
        }
        Action::Heal => {
            writeln!(out, "Return to H to heal.")?;
        }
        Action::Release(_) => {
            if before.wild.is_some() {
                writeln!(out, "You cannot release a monster during battle.")?;
            } else if after.collection.len() < before.collection.len() {
                writeln!(out, "Reserve released. You have room for another catch.")?;
            } else {
                writeln!(out, "That reserve slot is empty.")?;
            }
        }
        Action::Switch(index) => {
            if before.collection.get(index).is_some_and(|m| m.hp > 0) {
                writeln!(out, "Sent out reserve {}.", index + 1)?;
            } else {
                writeln!(out, "That reserve slot is empty or fainted.")?;
            }
        }
        Action::Up | Action::Down | Action::Left | Action::Right => {
            if before.wild.is_some() {
                writeln!(
                    out,
                    "You are in battle. Use attack, catch, switch N, or run."
                )?;
            } else if before.position == after.position {
                writeln!(out, "A wall blocks the way.")?;
            }
        }
        Action::Attack | Action::Catch | Action::Run if before.wild.is_none() => {
            writeln!(
                out,
                "There is no wild monster here. Explore the tall grass."
            )?;
        }
        _ => {}
    }
    if before.wild.is_some() && after.losses == before.losses {
        if after.collection.iter().filter(|m| m.hp == 0).count()
            > before.collection.iter().filter(|m| m.hp == 0).count()
        {
            writeln!(
                out,
                "Your active monster fainted; a healthy reserve stepped in."
            )?;
        } else if !matches!(action, Action::Switch(_)) && after.starter.hp < before.starter.hp {
            writeln!(
                out,
                "The wild monster hit back for {} damage.",
                before.starter.hp - after.starter.hp
            )?;
        }
    }
    Ok(())
}

fn goal(state: &State) -> bool {
    use super::Element;
    [Element::Fire, Element::Grass, Element::Water]
        .iter()
        .all(|element| {
            std::iter::once(&state.starter)
                .chain(state.collection.iter())
                .any(|m| m.element == *element)
        })
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
    let mut celebrated = goal(game.state());
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
                    if !celebrated && goal(game.state()) {
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
