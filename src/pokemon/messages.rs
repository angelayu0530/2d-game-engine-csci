use super::{Action, State};

/// Player-facing messages for one `step`, derived by comparing the state
/// before and after it. Shared by the terminal and window frontends.
pub fn describe(before: &State, after: &State, action: Action) -> Vec<String> {
    let mut lines = Vec::new();
    let mut say = |line: String| lines.push(line);
    if before.position != after.position && after.losses == before.losses {
        say(format!(
            "Moved to {}, {}.",
            after.position[0], after.position[1]
        ));
    }
    if after.encounters > before.encounters {
        say("A wild monster appeared!".into());
        if let Some(wild) = after.wild {
            say(wild.description().into());
        }
    }
    if after.wins > before.wins {
        say("Wild monster defeated! Gained 10 XP and a ball.".into());
    }
    if after.catches > before.catches {
        say("Caught it! Gained 10 XP and a reward ball.".into());
        if before.collection.len() == 5 {
            say("Party full: this catch was released.".into());
        }
    }
    if after.losses > before.losses {
        say("Your party fainted. Returned to H; party healed and balls restocked.".into());
    }
    if before.starter.element == after.starter.element && after.starter.level > before.starter.level
    {
        say("Level up! HP restored.".into());
    }
    match action {
        Action::Attack if before.wild.is_some() => {
            let wild = before.wild.unwrap();
            let hp = after.wild.map_or(0, |m| m.hp);
            if after.losses == before.losses {
                say(format!(
                    "Your attack dealt {} damage.",
                    wild.hp.saturating_sub(hp)
                ));
            }
        }
        Action::Catch if before.wild.is_some() && before.balls == 0 => {
            say("No balls left. Run and return to H to restock.".into());
        }
        Action::Catch if before.wild.is_some() && before.catches == after.catches => {
            say("It broke free! Try weakening it first.".into());
        }
        Action::Run if before.wild.is_some() => {
            say("Escaped safely.".into());
        }
        Action::Heal if before.wild.is_some() => {
            say("You cannot heal during battle.".into());
        }
        Action::Heal if before.position == [1, 1] => {
            say("Party healed and balls restocked.".into());
        }
        Action::Heal => {
            say("Return to H to heal.".into());
        }
        Action::Release(_) => {
            if before.wild.is_some() {
                say("You cannot release a monster during battle.".into());
            } else if after.collection.len() < before.collection.len() {
                say("Reserve released. You have room for another catch.".into());
            } else {
                say("That reserve slot is empty.".into());
            }
        }
        Action::Switch(index) => {
            if before.collection.get(index).is_some_and(|m| m.hp > 0) {
                say(format!("Sent out reserve {}.", index + 1));
            } else {
                say("That reserve slot is empty or fainted.".into());
            }
        }
        Action::Up | Action::Down | Action::Left | Action::Right => {
            if before.wild.is_some() {
                say("You are in battle. Use attack, catch, switch N, or run.".into());
            } else if before.position == after.position {
                say("A wall blocks the way.".into());
            }
        }
        Action::Attack | Action::Catch | Action::Run if before.wild.is_none() => {
            say("There is no wild monster here. Explore the tall grass.".into());
        }
        _ => {}
    }
    if before.wild.is_some() && after.losses == before.losses {
        if after.collection.iter().filter(|m| m.hp == 0).count()
            > before.collection.iter().filter(|m| m.hp == 0).count()
        {
            say("Your active monster fainted; a healthy reserve stepped in.".into());
        } else if !matches!(action, Action::Switch(_)) && after.starter.hp < before.starter.hp {
            say(format!(
                "The wild monster hit back for {} damage.",
                before.starter.hp - after.starter.hp
            ));
        }
    }
    lines
}
