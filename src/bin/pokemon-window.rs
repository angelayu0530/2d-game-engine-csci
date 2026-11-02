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
        Action, Config, Pokemon, Replay, describe,
        render::{self, HEIGHT, Log, WIDTH},
    },
    window::{self, Controls, Game, Key, Scale, WindowConfig},
    with_allocator,
};

#[derive(Parser)]
#[command(about = "Play the Pokemon-style adventure in a window")]
struct Cli {
    #[arg(long, value_enum, default_value_t = Backend::Pool)]
    allocator: Backend,
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

const MOVES: [(Action, [Key; 2]); 4] = [
    (Action::Up, [Key::Up, Key::W]),
    (Action::Down, [Key::Down, Key::S]),
    (Action::Left, [Key::Left, Key::A]),
    (Action::Right, [Key::Right, Key::D]),
];
const COMMANDS: [(Action, Key); 4] = [
    (Action::Attack, Key::F),
    (Action::Catch, Key::C),
    (Action::Run, Key::R),
    (Action::Heal, Key::H),
];
const SLOTS: [Key; 5] = [Key::Key1, Key::Key2, Key::Key3, Key::Key4, Key::Key5];

/// The action for this update: the first newly pressed key, else `Wait` so
/// effects keep animating between turns.
fn action(controls: &Controls) -> Action {
    for (action, keys) in MOVES {
        if keys.iter().any(|&key| controls.pressed(key)) {
            return action;
        }
    }
    for (action, key) in COMMANDS {
        if controls.pressed(key) {
            return action;
        }
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
    Action::Wait
}

struct PokemonGame<A: Allocator> {
    game: Pokemon<A>,
    log: Log,
    celebrated: bool,
    actions: Option<Vec<Action>>,
}

impl<A: Allocator> Game for PokemonGame<A> {
    fn update(&mut self, controls: &Controls, _dt: f32) {
        let action = action(controls);
        let before = self.game.state().clone();
        self.game.step(action);
        if let Some(actions) = &mut self.actions {
            actions.push(action);
        }
        if action == Action::Wait {
            return;
        }
        self.log.next_turn();
        for line in describe(&before, self.game.state(), action) {
            self.log.push(&line);
        }
        if !self.celebrated && self.game.state().has_every_element() {
            self.log
                .push("Goal complete! Your party contains all three elements. Keep exploring!");
            self.celebrated = true;
        }
    }

    fn draw(&self, canvas: &mut Canvas) {
        render::draw(&self.game, &self.log, canvas);
    }

    fn status(&self) -> Option<String> {
        Some(format!("{} particles", self.game.live_particles()))
    }
}

fn play<A: Allocator>(cli: &Cli) -> Result<(), Box<dyn Error>> {
    if !cli.no_save && cli.record.as_ref() == Some(&cli.save) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Save and recording paths must differ",
        )
        .into());
    }
    let seed = cli.seed.unwrap_or_else(|| {
        std::collections::hash_map::RandomState::new().hash_one(std::time::SystemTime::now())
    });
    let fresh = || {
        Pokemon::<A>::new(Config {
            seed,
            trail: cli.trail,
            ..Config::default()
        })
    };
    let mut log = Log::default();
    let game = if cli.no_save || cli.new {
        fresh()
    } else {
        match Pokemon::<A>::resume(&cli.save)? {
            Some(game) => {
                log.push(&format!("Loaded progress from {}.", cli.save.display()));
                if cli.seed.is_some() {
                    log.push("Existing save takes precedence over --seed.");
                }
                game
            }
            None => fresh(),
        }
    };
    log.push(game.state().starter.description());
    log.push("Walk into the tall grass to find wild monsters. Build a party with Fire, Grass, and Water.");

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
        log,
        actions: initial.as_ref().map(|_| Vec::new()),
    };
    window::run(&config, &mut session)?;

    if !cli.no_save {
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
