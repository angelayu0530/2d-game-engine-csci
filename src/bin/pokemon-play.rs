use clap::Parser;
use game_engine::{
    alloc::{Allocator, Backend},
    pokemon::{Config, Pokemon, terminal},
    with_allocator,
};
use std::{hash::BuildHasher, io, path::PathBuf};

#[derive(Parser)]
#[command(about = "Play and save the Pokemon-style adventure in your terminal")]
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
    #[arg(long, conflicts_with_all = ["save", "new"], help = "Disable saving/loading (useful for tests)")]
    no_save: bool,
    #[arg(
        long,
        help = "Record this session for allocator benchmarks, including its starting save state"
    )]
    record: Option<PathBuf>,
    #[arg(long, default_value_t = 64)]
    trail: usize,
}
fn main() -> io::Result<()> {
    let cli = Cli::parse();
    with_allocator!(cli.allocator, play(&cli))
}
fn play<A: Allocator>(cli: &Cli) -> io::Result<()> {
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
    let mut game = if cli.no_save || cli.new {
        fresh()
    } else {
        match Pokemon::<A>::load(&cli.save) {
            Ok(game) => {
                eprintln!("Loaded progress from {}.", cli.save.display());
                if cli.seed.is_some() {
                    eprintln!("Existing save takes precedence over --seed.");
                }
                game
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => fresh(),
            Err(error) => return Err(error),
        }
    };
    if !cli.no_save && cli.record.as_ref() == Some(&cli.save) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Save and recording paths must differ",
        ));
    }
    terminal::recorded_session(
        &mut game,
        (!cli.no_save).then_some(cli.save.as_path()),
        cli.record.as_deref(),
        &mut io::stdin().lock(),
        &mut io::stdout().lock(),
    )
}
