# Memory allocation strategies in a 2D game engine

Angela Yu, Tahmid Ahmed

A controlled comparison of three allocation strategies for short-lived,
fixed-size game objects (particles) in a single-threaded 2D engine. The
engine repeatedly creates, updates, and destroys particles. We test whether
reusing memory improves speed without excessive memory cost, from 10K to 5M
objects.

## Research question

Does memory reuse (pool or arena) beat individual `new`/`delete` for
fixed-size particles, and at what peak-memory cost?

## Layout

```
Cargo.toml              package manifest; release profile used for all measurements
src/lib.rs              allocators, workload, and peak RSS
src/main.rs             benchmark CLI: one allocator per process, CSV rows on stdout
src/particle.rs         fixed-size Particle (pos, vel, lifetime), deterministic spawn, update
src/alloc/mod.rs        Allocator trait, Backend (CLI --allocator) and with_allocator! dispatch
src/alloc/heap.rs       baseline: one allocation per object, freed individually
src/alloc/pool.rs       fixed-size pool: preallocated slots + free list
src/alloc/arena.rs      linear arena: bump cursor through blocks, reclaim all at once
src/workload.rs         the fixed test loop and per-phase timing
src/memory.rs           peak RSS measurement
src/canvas.rs           software framebuffer: rects and score digits
src/window/mod.rs       WindowConfig, Game trait, run loop, fixed-step clock (minifb, `window` feature)
src/window/input.rs     backend-neutral Key/MouseButton and per-update Controls snapshot
src/pong/mod.rs         deterministic Pong sim and CPU-vs-CPU match runner
src/pong/cpu.rs         computer opponent: Cpu + CpuConfig, seeded aim variance
src/pong/render.rs      draws a Pong state onto a Canvas
src/pong/replay.rs      match recording format and replay runner
src/pong/trace.rs       per-frame timing/memory sample (Frame) and its CSV row
src/bin/play.rs         playable Pong in a window; records the match with --record
src/bin/pong.rs         headless Pong benchmark: CPU vs. CPU or a recorded match
results/                CSV output and plots (gitignored except this folder)
scripts/bench.py        runs every allocator N times per count, writes CSV, prints a summary
scripts/plot.py         plots bench.csv or a game summary vs. object count, or --trace CSVs vs. frame
```

## Workload

Every backend runs the same loop:

1. Reserve storage / initialize.
2. Spawn the same objects.
3. Run fixed-step updates.
4. Destroy objects and reclaim.

Identical object construction, update logic, lifetimes, and completed work
for every allocator. Final-state checksums are compared across backends
before any timing is trusted. Arena resets happen only once all objects are
dead.

## Experiment controls

- Object counts: 10K, 100K, 1M, 5M (intermediate counts to be decided).
- Single-threaded, same machine, release build, same workload.
- Initial settings: 2 warmups, 10 measured trials, 100 updates per batch.
- Record failures and peak memory at high object counts.

## Measurements

- Total runtime, including setup and teardown.
- Allocation and deallocation time, separate from update time.
- Object throughput: completed object updates per second.
- Peak memory: process peak RSS, plus reserved vs. live-object storage.

Report medians and run-to-run spread. Plot runtime and memory against
object count.

## Build and run

```sh
cargo build --release
cargo test

cargo run --release -- --allocator pool --count 1000000 --trials 10
cargo run --release -- --help

python3 scripts/bench.py --runs 5 --counts 10000 100000 1000000
```

Peak RSS is a per-process high-water mark, so each allocator runs in its own
process. `scripts/bench.py` fails if final-state checksums differ between
allocators at the same count.

### Pong

```sh
cargo run --release --bin play -- --allocator pool --record results/match.pongrec --trace results/live-pool.csv
cargo run --release --bin pong -- --allocator arena --replay results/match.pongrec --trials 10
cargo run --release --bin pong -- --allocator heap --frames 3600 --trials 10
```

`play` opens a 1024x768 window. W/S or Up/Down move the left paddle against
the computer; `--two-player` puts the right paddle on Up/Down. The match ends
when either side reaches `--to` points (default 11; `0` plays until Esc), and
Esc quits early. The window title shows frame rate, live particles, and mean
simulation time per update.

The simulation runs at a fixed 60 Hz step and is deterministic, so a match
is fully described by its config and per-frame input. `--record` writes both
on exit; `pong --replay` re-runs that exact match headlessly through any
allocator, reaching the same score and checksum. Without `--replay`, `pong`
plays the computer against itself. `--no-default-features` builds without
the window and the `play` binary.

The computer (`pong::Cpu`) is separate from the sim: it reads only paddle
height and ball state and returns an axis, like a keyboard. When the ball
starts approaching it picks a target height (ball height, offset to hit
off-centre) plus one random aim error for that approach, then moves toward it
at 75% of player paddle speed. `--cpu-variance` (both `play` and `pong`,
default 0.5) scales that error as a fraction of paddle reach; `0` makes it
exact. The error is seeded from `--seed`, so matches stay reproducible;
recordings already contain the computer's inputs, so `--replay` rejects it.

### Writing another game

`game_engine::window` holds no Pong code. A game implements `Game` and
hands it to `window::run` with a `WindowConfig`:

```rust
use game_engine::{canvas::Canvas, window::{self, Controls, Game, Key, WindowConfig}};

struct Square { x: f32 }

impl Game for Square {
    fn update(&mut self, controls: &Controls, dt: f32) {
        self.x += controls.axis(&[Key::Left], &[Key::Right]) * 200.0 * dt;
    }
    fn draw(&self, canvas: &mut Canvas) {
        canvas.clear(0x000000);
        canvas.fill_rect(self.x, 100.0, 32.0, 32.0, 0xFFFFFF);
    }
}

fn main() -> Result<(), window::Error> {
    let config = WindowConfig { title: "square".into(), ..WindowConfig::default() };
    window::run(&config, &mut Square { x: 0.0 })
}
```

- `WindowConfig`: title, size, `Scale` (X1/X2/X4/FitScreen), `resizable`,
  `fps` cap (`0` uncapped), simulation `dt`, `quit_key` (`None` to disable),
  and `stats_interval` for the fps/µs title readout (`None` to disable).
- `Game`: `update` runs at the fixed `dt`; `draw` runs once per frame;
  optional `status` appends to the title and `running` returning false ends
  the loop.
- `Controls`: `down`/`pressed`/`released` per key, `axis` for paired keys,
  `mouse` position in canvas pixels, `button_down`/`button_pressed`.
  `pressed` is true on exactly one `update`, even when frames outpace updates.
- `Window` and `FixedStep` stay public for a custom loop.
- minifb is referenced only inside `src/window/`, so swapping the backend
  does not touch game code.

## Full Test

```bash
cargo build --release

# 1. Play one match (ends itself at 11 points, or end early by hitting Esc)
./target/release/play --record results/match.pongrec

# 2. Benchmark it, plus CPU vs CPU, on every allocator
for a in heap pool arena; do
  ./target/release/pong --allocator $a --replay results/match.pongrec \
    --warmups 2 --trials 10 --trace results/trace-human-$a.csv > results/pong-human-$a.csv
  ./target/release/pong --allocator $a --frames 3600 \
    --warmups 2 --trials 10 --trace results/trace-cpu-$a.csv > results/pong-cpu-$a.csv
done

# 3. Check all allocators played the same match: expect one checksum per line
for m in human cpu; do echo $m; awk -F, 'FNR>1 {print $14}' results/pong-$m-*.csv | sort -u; done

# 4. Plot
python3 scripts/plot.py --csv results/trace-*.csv
open results/trace.png
```

## Status

Allocators, workload, peak RSS, the benchmark CLI, and the run script are
implemented. Counts run so far: 10K, 100K, and 1M. Pong is playable in a
window, and recorded matches replay through every allocator headlessly.

Next:

- Plot script under `scripts/`. Read `results/bench.csv` and graph runtime and peak memory against object count.
- 5M object count. Not run yet.

### Pokémon-style backend

`src/pokemon/` implements a headless tile-based monster-catching game using
this engine's `Allocator` trait and `Particle` effects. It includes a small
2D map with walls, tall-grass encounters, a healing center, a Fire starter,
Fire/Grass/Water type advantages, turn-based attacks, catching, fleeing,
experience, leveling, and recovery at the center after defeat. The party holds
one active monster plus five reserves; later catches are counted and released.
Caught monsters can be switched into battle, and healthy reserves take over
when the active monster faints. A full-party defeat returns you to the center.
Healing at the center restores HP and ten capture balls. Battle rewards also
replenish one ball so automated expeditions can continue.

This is backend-only: no new window or artwork. To drive it from another
frontend, construct `pokemon::Pokemon::<PoolAllocator>::new(Config::default())`,
call `step(Action::Right)` (or another `Action`) for each turn, and read
`state()`. One simulation step is one action, not a real-time movement frame.
Invalid actions do nothing; failed captures and attacks allow a counterattack.
`auto_action()` supplies a deterministic exploration/battle policy for benchmarks.

```sh
# Play interactively in a terminal (no graphical dependencies)
cargo run --release --no-default-features --bin pokemon-play

# Automated expedition, using the same allocator flags as Pong
cargo run --release --no-default-features --bin pokemon -- \
  --allocator pool --frames 3600 --seed 1 --warmups 2 --trials 10 \
  --trace results/pokemon-trace.csv

# Run all three allocators in separate processes and verify identical work
python3 scripts/bench_pokemon.py --frames 3600 --trials 10

# Gameplay, allocator equivalence, and CLI tests also run in normal cargo test
cargo test --no-default-features
```

The CLI emits summary CSV with gameplay outcomes, checksum, particle counts,
peak allocator storage, simulation time, and process peak RSS. `--trace` emits
per-action time, live particles, reserved bytes, wins, and catches.
`scripts/plot.py` accepts both Pokémon and Pong trace CSVs.
`--burst` controls effects per action and `--lifetime` their lifetime in steps.
All allocators receive identical effects. Particles overlap across turns. Battle completion (win, catch, escape, or full-party
defeat) clears effects and reclaims storage, like a scored point in Pong.
Retiring an individual particle does not reclaim arena storage. Gameplay state is stored normally;
the allocation experiment measures particle effects, as in Pong. Timing columns separate setup, simulation, and teardown. `total_ns` is their
sum; final checksum computation and disk I/O are excluded. Trace collection
adds overhead when explicitly enabled. The benchmark uses automated play, and `pokemon-play` accepts player commands.
Terminal play automatically loads `pokemon.save` and saves progress on `quit` or EOF.
The benchmark remains independent of terminal saves.

#### Terminal controls

Run `pokemon-play`, type a command, then press Enter. Use `w/a/s/d` or
`up/left/down/right` to explore the ASCII map. `@` is your player, `g` is tall
grass, `#` is a wall, and `H` is the healing center. Walk into grass to find
wild monsters, then use `attack`, `catch`, or `run`. The screen shows both
monsters' HP, your level, XP, balls, and action results.

`party` lists reserves; `switch 1` sends out reserve 1 (slots 1–5).
Switching during battle lets the wild monster counterattack. `release 1`
makes room for another catch outside battle. At `H`, `heal` restores the
whole party and capture balls. `map` and `help` do not consume a turn.
`quit` exits; EOF also exits cleanly. Use `--seed N` for a different expedition
or `--allocator heap|pool|arena` to choose effect storage.

The terminal goal is to assemble Fire, Grass, and Water in your party. It
announces completion and allows continued exploration. This is a small playable
prototype, not a full Pokémon campaign; there are no towns or gyms yet.

#### Random encounters and saved progress

New terminal adventures use a fresh random seed. Every successful step into
tall grass has a one-in-three encounter chance, so the number of steps between
encounters varies. Encounters are turn-based, not wall-clock timers: reading a
prompt does not trigger a fight. `--seed 1` makes a new adventure reproducible
for testing; an existing save takes precedence. The headless benchmark retains
its fixed default seed so allocator comparisons stay reproducible.

Charmander, Bulbasaur, and Squirtle have names and descriptions of appearance
and type advantages. Descriptions appear on encounters and in `party`; `look`
describes your active Pokémon and the current wild Pokémon without using a turn.

```sh
# Continue your saved adventure, or create one if no save exists
cargo run --release --no-default-features --bin pokemon-play

# Separate save slot
cargo run --release --no-default-features --bin pokemon-play -- --save adventure.save

# Start fresh (replaces the chosen save when you save or quit)
cargo run --release --no-default-features --bin pokemon-play -- --new
```

Type `save` for a checkpoint. `quit` and EOF save automatically; run the same
command from the same directory to resume. Snapshots preserve your position,
party, HP, XP, balls, current battle, statistics, random state, and live effects.
They can be loaded with any allocator. A temporary file is flushed and renamed
to replace the save; malformed saves produce an error instead of being silently
overwritten. `--no-save` disables persistence for isolated tests. Forced process
termination does not save unsaved turns; use `save` to checkpoint longer sessions.

#### Pokémon allocator experiment (same workflow as Pong)

The primary controlled experiment remains `src/workload.rs`: fixed counts of
identical particles with allocation, update, retirement, and reclamation phases.
Pong and Pokémon are application workloads for checking whether those results
also hold during gameplay. Pokémon measures short-lived particle effects;
it does not compare allocation strategies for party members or map tiles.

```sh
# 1. Play normally, saving progress and recording this session's actions
cargo run --release --no-default-features --bin pokemon-play -- --record results/session.pokemonrec

# 2. Replay the identical session on every allocator, with warmups and repeated trials
python3 scripts/bench_pokemon.py --replay results/session.pokemonrec --trials 10 --warmups 2 --out results/pokemon-human.csv

# 3. Also test increasing effect loads on the same automated adventure
python3 scripts/bench_pokemon.py --bursts 32 256 2048 --frames 3600 --trials 10 --out results/pokemon-auto.csv

# 4. Per-turn chart for a recorded session. Writes the CSV and results/pokemon-diagnostic.png
python3 scripts/bench_pokemon.py --replay results/session.pokemonrec --trials 1 --trace --out results/pokemon-diagnostic.csv
```

A recording includes the exact starting snapshot, random state, action list,
and expected final checksum. This works when continuing an existing save.
Reading descriptions, waiting for keyboard input, saving, and terminal output
are excluded from the replay workload. Random events repeat exactly across
allocators. Each replay verifies the recorded checksum; the comparison script
also checks identical gameplay outcomes and particle work across all trials.
The script writes its combined result only after these checks pass.

Each measured trial runs in its own process, with allocator order shuffled
reproducibly between trials. Warmups occur within that process. Summary output
reports median total time, min/max spread, and median peak RSS. Process peak RSS
includes warmups, input parsing, runtime overhead, and optional trace buffers;
`reserved_bytes` measures peak particle allocator storage during the workload.
For replay, restored particles start in a fresh allocator, not the original
allocator's fragmentation state. Setup time includes snapshot restoration.

`setup_ns` covers game construction/restoration, `sim_ns` the action loop, and
`teardown_ns` destruction of the game and remaining effects. `total_ns` sums
those phases; checksum computation and CSV I/O are outside these measurements.
`trace_enabled` distinguishes instrumented runs. Particle work counts refer
only to the measured session, with `initial_live + created - retired = live`
before final cleanup. `frames` is retained for compatibility but means turns
in Pokémon. Use total time for lifecycle comparisons and simulation time for
steady gameplay; do not equate a Pokémon turn with a Pong frame.

Effect-load sweeps change the number of particles per action while preserving
gameplay. Particle lifetimes and battle-boundary reclamation are identical across
allocators. Like Pong between scored points, the arena can grow between battle
endings even as individual particles expire. Report this lifecycle policy with
results rather than generalizing to all games. `--replay` rejects workload overrides to preserve the actual recording.

#### Million-particle workloads (no recording needed)

```sh
# Quick capacity/correctness check: 1M and 5M simultaneously live particles
python3 scripts/bench_pokemon.py --counts 1000000 5000000 --frames 12 --trials 1 --warmups 0 --out results/pokemon-capacity.csv

# Research sweep: 10K through 5M, 100 turns, repeated measurements.
# Also writes results/pokemon-counts.png (simulation time and peak RSS vs object count).
python3 scripts/bench_pokemon.py --counts 10000 100000 1000000 5000000 --frames 100 --trials 10 --warmups 2 --out results/pokemon-counts.csv
```

`--counts` is a controlled peak-live workload: each effect-producing action
creates exactly N particles with a one-turn lifetime. On the next turn those
particles update, retire, and storage is reclaimed before another burst. The
runner verifies that `peak_live` equals N on all three allocators. Total
`created` can be much larger than N. This mode tests repeated allocation and
reclamation at an exact concurrent scale; use `--bursts` with `--lifetime` for
overlapping, longer-lived effects. These are distinct workload policies and
should be reported separately. `--counts` cannot be combined with replay or
lifetime/burst overrides. It uses the automated player and needs no save file.

Replay requires an existing recording. A missing recording is now reported
before building, and benchmark subprocess failures print the actual error
instead of a Python traceback. Progress messages identify the count, trial,
and allocator being run. Larger counts and more turns increase runtime and
memory; the quick check above verifies capacity before the full trial sweep.

#### Matching Pong's benchmark workload

The Pokémon CLI now defaults to the same particle settings as `pong`:
`--frames 3600 --trail 64 --burst 32 --lifetime 45 --dt 0.016666667 --seed 1`.
Every simulation step creates a continuous trail; actions add event bursts.
Existing particles update and retire before the new trail/bursts spawn.
Battle endings clear and reclaim effects before the next batch, like a scored
point in Pong. Effects are also cleared every 240 turns. The arena keeps the
blocks it already reserved.
The script reports `sim_ns` as its primary timing, matching Pong's simulation
measurement; the additional setup/cleanup columns remain available.

```sh
python3 scripts/bench_pokemon.py --frames 3600 --trials 10 --warmups 2 --out results/pokemon-pong-style.csv
```

The script writes `results/pokemon-pong-style.png` in the same three-row layout as a Pong trace: step time, live particles, and reserved memory for heap, pool, and arena. The PNG comes from a separate warmed replay, and the script checks that its gameplay totals match the timed runs. `--trace` records the timing trials themselves and includes that overhead in `sim_ns`. A count or burst sweep writes the two-panel object-count chart, using median simulation time and median peak RSS. Both axes are logarithmic, since the counts span several orders of magnitude. `--no-plot` skips the PNG. `scripts/plot.py` still reads the CSVs directly.

Use `--trail` to scale continuous particle load and `--bursts` to scale action
bursts while retaining overlapping lifetimes. Actual peak live count depends
on when battles end; inspect `peak_live` rather than interpreting trail size as
concurrent population. `--counts` remains a separate one-turn batch stress test,
with trail disabled. Do not use it for a Pong-style lifecycle comparison.

The games share allocator mechanics and particle defaults, not event frequency:
a Pokémon step is a turn and a Pong step advances the ball. Their curves should
not be expected to match. Existing progress saves remain loadable, but old
recordings require re-recording because the particle workload has changed.
New recordings use version 2, and new saves preserve trail and timestep values.
A resumed save retains its recorded particle configuration; use `--new` for
new defaults. Existing CSVs are historical and are not rewritten automatically.
