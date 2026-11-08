use std::{
    error::Error,
    fs::File,
    hash::BuildHasher,
    io::{self, BufWriter},
    path::PathBuf,
};

use clap::Parser;
use game_engine::{
    alloc::{Allocator, Backend},
    canvas::Canvas,
    pokemon::{
        Action, Config, Pokemon, Replay,
        autoplay::Explorer,
        describe,
        render::{BattleCommand, Facing, HEIGHT, Hud, Renderer, WIDTH},
    },
    window::{self, Controls, Game, Key, Scale, WindowConfig},
    with_allocator,
};

#[derive(Parser)]
#[command(about = "Play the Pokemon-style adventure in a window")]
struct Cli {
    #[arg(long, value_enum, default_value_t = Backend::Pool)]
    allocator: Backend,
    #[arg(long, conflicts_with_all = ["save", "new"], help = "Watch automated play in a fresh session without loading or saving progress")]
    autoplay: bool,
    #[arg(
        long,
        requires = "autoplay",
        help = "Use the original headless benchmark patrol instead of exploration"
    )]
    benchmark_policy: bool,
    #[arg(long, requires = "autoplay", value_parser = clap::value_parser!(u32).range(1..), help = "Close after this many automated turns")]
    turns: Option<u32>,
    #[arg(long, requires = "autoplay", default_value_t = 6, value_parser = clap::value_parser!(u32).range(1..), help = "Window updates per automated turn (6 = about 10 turns/sec)")]
    turn_delay: u32,
    #[arg(long, help = "Seed for a new game; omitted seeds are random")]
    seed: Option<u64>,
    #[arg(
        long,
        default_value = "pokemon.save",
        help = "Automatically load/save this file"
    )]
    save: PathBuf,
    #[arg(long, help = "Start fresh, replacing this save when you quit")]
    new: bool,
    #[arg(long, conflicts_with_all = ["save", "new"], help = "Disable saving/loading")]
    no_save: bool,
    #[arg(
        long,
        help = "Record this session for allocator benchmarks, including its starting save state"
    )]
    record: Option<PathBuf>,
    #[arg(long, default_value_t = 4, help = "Effects spawned every update")]
    trail: usize,
}

fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    with_allocator!(cli.allocator, play(&cli))
}

const DIRECTIONS: [([Key; 2], Action, Facing, i32, i32); 4] = [
    ([Key::Up, Key::W], Action::Up, Facing::Up, -1, 0),
    ([Key::Down, Key::S], Action::Down, Facing::Down, 1, 0),
    ([Key::Left, Key::A], Action::Left, Facing::Left, 0, -1),
    ([Key::Right, Key::D], Action::Right, Facing::Right, 0, 1),
];
const CONFIRM: [Key; 3] = [Key::Enter, Key::Space, Key::Z];
const SLOTS: [Key; 5] = [Key::Key1, Key::Key2, Key::Key3, Key::Key4, Key::Key5];

fn any_pressed(controls: &Controls, keys: &[Key]) -> bool {
    keys.iter().any(|&key| controls.pressed(key))
}

/// The action for this update, else `Wait` so effects keep animating.
fn action(
    hud: &mut Hud,
    pending: bool,
    in_battle: bool,
    party_len: usize,
    controls: &Controls,
) -> Action {
    if pending {
        if controls.pressed(Key::R) || controls.pressed(Key::X) || controls.pressed(Key::Backspace)
        {
            return Action::Run;
        }
        for (slot, &key) in SLOTS.iter().enumerate() {
            if controls.pressed(key) {
                return Action::Release(slot);
            }
        }
        return Action::Wait;
    }

    if !in_battle {
        hud.party_open = false;
        for (keys, action, facing, _, _) in DIRECTIONS {
            if any_pressed(controls, &keys) {
                hud.facing = facing;
                return action;
            }
        }
        if controls.pressed(Key::H) {
            return Action::Heal;
        }
        let shift = controls.down(Key::LeftShift) || controls.down(Key::RightShift);
        for (slot, &key) in SLOTS.iter().enumerate() {
            if controls.pressed(key) {
                return if shift {
                    Action::Release(slot)
                } else {
                    Action::Switch(slot)
                };
            }
        }
        return Action::Wait;
    }

    if controls.pressed(Key::F) {
        hud.party_open = false;
        return Action::Attack;
    }
    if controls.pressed(Key::C) {
        hud.party_open = false;
        return Action::Catch;
    }
    if controls.pressed(Key::R) {
        hud.party_open = false;
        return Action::Run;
    }
    for (slot, &key) in SLOTS.iter().enumerate() {
        if controls.pressed(key) {
            hud.party_open = false;
            return Action::Switch(slot);
        }
    }
    if hud.party_open && (controls.pressed(Key::X) || controls.pressed(Key::Backspace)) {
        hud.party_open = false;
        return Action::Wait;
    }
    if any_pressed(controls, &CONFIRM) {
        return confirm(hud, party_len);
    }
    for (keys, _, _, row, col) in DIRECTIONS {
        if any_pressed(controls, &keys) {
            if hud.party_open {
                if row != 0 {
                    let last = party_len.saturating_sub(1);
                    let slot = hud.party_slot.min(last);
                    hud.party_slot = if row < 0 {
                        slot.saturating_sub(1)
                    } else {
                        (slot + 1).min(last)
                    };
                }
            } else {
                hud.command = move_command(&hud.command, row, col);
            }
            return Action::Wait;
        }
    }
    Action::Wait
}

fn confirm(hud: &mut Hud, party_len: usize) -> Action {
    match &hud.command {
        BattleCommand::Fight => Action::Attack,
        BattleCommand::Catch => Action::Catch,
        BattleCommand::Run => Action::Run,
        BattleCommand::Pokemon => {
            if !hud.party_open {
                hud.party_open = true;
                hud.party_slot = hud.party_slot.min(party_len.saturating_sub(1));
                Action::Wait
            } else if hud.party_slot < party_len {
                Action::Switch(hud.party_slot)
            } else {
                Action::Wait
            }
        }
    }
}

fn move_command(command: &BattleCommand, row_delta: i32, col_delta: i32) -> BattleCommand {
    let (mut row, mut col) = match command {
        BattleCommand::Fight => (0, 0),
        BattleCommand::Catch => (0, 1),
        BattleCommand::Pokemon => (1, 0),
        BattleCommand::Run => (1, 1),
    };
    row = (row + row_delta).clamp(0, 1);
    col = (col + col_delta).clamp(0, 1);
    match (row, col) {
        (0, 0) => BattleCommand::Fight,
        (0, _) => BattleCommand::Catch,
        (_, 0) => BattleCommand::Pokemon,
        _ => BattleCommand::Run,
    }
}

struct PokemonGame<A: Allocator> {
    game: Pokemon<A>,
    hud: Hud,
    renderer: Renderer,
    celebrated: bool,
    actions: Option<Vec<Action>>,
    autoplay: bool,
    turn_delay: u32,
    cooldown: u32,
    turns: u32,
    turn_limit: Option<u32>,
    paused: bool,
    explorer: Explorer,
    benchmark_policy: bool,
}

impl<A: Allocator> Game for PokemonGame<A> {
    fn update(&mut self, controls: &Controls, _dt: f32) {
        if !self.running() {
            return;
        }
        if self.autoplay {
            if controls.pressed(Key::P) {
                self.paused = !self.paused;
            }
            if self.paused {
                return;
            }
            if self.cooldown > 0 {
                self.cooldown -= 1;
                return;
            }
            self.cooldown = self.turn_delay - 1;
        }
        let pending = self.game.state().pending.is_some();
        let in_battle = self.game.state().wild.is_some();
        let party_len = self.game.state().collection.len();
        let action = if self.autoplay {
            let action = if self.benchmark_policy {
                self.game.auto_action()
            } else {
                self.explorer.action(self.game.state())
            };
            for (_, direction, facing, _, _) in DIRECTIONS {
                if action == direction {
                    self.hud.facing = facing;
                }
            }
            self.hud.command = match action {
                Action::Catch => BattleCommand::Catch,
                Action::Run => BattleCommand::Run,
                _ => BattleCommand::Fight,
            };
            action
        } else {
            action(&mut self.hud, pending, in_battle, party_len, controls)
        };
        let before = self.game.state().clone();
        self.game.step(action);
        self.turns = self.turns.saturating_add(1);
        if let Some(actions) = &mut self.actions {
            actions.push(action);
        }
        if before.wild.is_some() && self.game.state().wild.is_none() && action != Action::Wait {
            self.hud.party_open = false;
            self.hud.command = BattleCommand::Fight;
        }
        if action == Action::Wait {
            return;
        }
        self.hud.log.next_turn();
        for line in describe(&before, self.game.state(), action) {
            self.hud.log.push(&line);
        }
        if !self.celebrated && self.game.state().has_every_element() {
            self.hud
                .log
                .push("Goal complete! Your party contains all three elements. Keep exploring!");
            self.celebrated = true;
        }
    }

    fn draw(&self, canvas: &mut Canvas) {
        self.renderer.draw(&self.game, &self.hud, canvas);
    }

    fn status(&self) -> Option<String> {
        let mode = if self.autoplay {
            if self.paused { "AUTO PAUSED" } else { "AUTO" }
        } else {
            "MANUAL"
        };
        Some(format!(
            "{mode} | turn {} | {} particles",
            self.turns,
            self.game.live_particles()
        ))
    }

    fn running(&self) -> bool {
        self.turn_limit.is_none_or(|limit| self.turns < limit)
    }
}

fn play<A: Allocator>(cli: &Cli) -> Result<(), Box<dyn Error>> {
    let saving = !cli.no_save && !cli.autoplay;
    if saving && cli.record.as_ref() == Some(&cli.save) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Save and recording paths must differ",
        )
        .into());
    }
    let seed = cli.seed.unwrap_or_else(|| {
        if cli.autoplay {
            1
        } else {
            std::collections::hash_map::RandomState::new().hash_one(std::time::SystemTime::now())
        }
    });
    let fresh = || {
        Pokemon::<A>::new(Config {
            seed,
            trail: cli.trail,
            ..Config::default()
        })
    };
    let mut hud = Hud::default();
    let game = if !saving || cli.new {
        fresh()
    } else {
        match Pokemon::<A>::resume(&cli.save)? {
            Some(game) => {
                hud.log
                    .push(&format!("Loaded progress from {}.", cli.save.display()));
                if cli.seed.is_some() {
                    hud.log.push("Existing save takes precedence over --seed.");
                }
                game
            }
            None => fresh(),
        }
    };
    hud.log.push(game.state().starter.description());
    hud.log
        .push("Walk into the tall grass to find wild monsters. Build a party with Fire, Grass, and Water.");

    if cli.autoplay {
        hud.log
            .push("Autoplay: P pauses, Esc exits. Progress is not saved.");
    }
    let initial = cli.record.as_ref().map(|_| game.snapshot());
    let config = WindowConfig {
        title: format!("Monster Trail [{}]", A::NAME),
        width: WIDTH,
        height: HEIGHT,
        scale: Scale::X2,
        dt: game.config().dt,
        ..WindowConfig::default()
    };
    let mut session = PokemonGame {
        celebrated: game.state().has_every_element(),
        game,
        hud,
        renderer: Renderer::new(),
        actions: initial.as_ref().map(|_| Vec::new()),
        autoplay: cli.autoplay,
        turn_delay: cli.turn_delay,
        cooldown: 0,
        turns: 0,
        turn_limit: cli.turns,
        paused: false,
        explorer: Explorer::default(),
        benchmark_policy: cli.benchmark_policy,
    };
    window::run(&config, &mut session)?;

    if saving {
        session.game.save(&cli.save)?;
        eprintln!("Progress saved to {}.", cli.save.display());
    }
    if let (Some(path), Some(initial), Some(actions)) = (&cli.record, initial, session.actions) {
        Replay {
            initial,
            actions,
            checksum: session.game.checksum(),
        }
        .write(BufWriter::new(File::create(path)?))?;
        eprintln!("Recorded actions to {}.", path.display());
    }
    eprintln!("Final checksum: {}", session.game.checksum());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use game_engine::alloc::pool::PoolAllocator;

    #[test]
    fn autoplay_matches_headless_policy_and_stops_at_exact_turn_limit() {
        let cfg = Config::default();
        let mut expected = Pokemon::<PoolAllocator>::new(cfg);
        let mut session = PokemonGame {
            game: Pokemon::<PoolAllocator>::new(cfg),
            hud: Hud::default(),
            renderer: Renderer::new(),
            celebrated: false,
            actions: Some(Vec::new()),
            autoplay: true,
            turn_delay: 3,
            cooldown: 0,
            turns: 0,
            turn_limit: Some(300),
            paused: false,
            explorer: Explorer::default(),
            benchmark_policy: true,
        };
        let controls = Controls::default();
        session.paused = true;
        session.update(&controls, cfg.dt);
        assert_eq!(session.turns, 0);
        session.paused = false;
        for _ in 0..1000 {
            session.update(&controls, cfg.dt);
        }
        for _ in 0..300 {
            expected.step(expected.auto_action());
        }
        assert!(!session.running());
        assert_eq!(session.turns, 300);
        assert_eq!(session.actions.as_ref().unwrap().len(), 300);
        assert_eq!(session.game.checksum(), expected.checksum());
    }

    #[test]
    fn autoplay_cli_protects_save_and_validates_pacing() {
        assert!(Cli::try_parse_from(["pokemon-window"]).is_ok());
        assert!(Cli::try_parse_from(["pokemon-window", "--autoplay"]).is_ok());
        assert!(
            Cli::try_parse_from(["pokemon-window", "--autoplay", "--save", "test.save"]).is_err()
        );
        assert!(
            Cli::try_parse_from(["pokemon-window", "--autoplay", "--turn-delay", "0"]).is_err()
        );
        assert!(Cli::try_parse_from(["pokemon-window", "--turns", "30"]).is_err());
    }
}
