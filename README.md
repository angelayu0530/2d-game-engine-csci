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
src/bin/play.rs         playable Pong in a window; records the match with --record
src/bin/pong.rs         headless Pong benchmark: CPU vs. CPU or a recorded match
results/                CSV output and plots (gitignored except this folder)
scripts/bench.py        runs every allocator N times per count, writes CSV, prints a summary
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
cargo run --release --bin play -- --allocator pool --record results/match.pongrec
cargo run --release --bin pong -- --allocator arena --replay results/match.pongrec --trials 10
cargo run --release --bin pong -- --allocator heap --frames 3600 --trials 10
```

`play` opens a 1024x768 window. W/S or Up/Down move the left paddle against
the computer; `--two-player` puts the right paddle on Up/Down. Esc quits.
The window title shows frame rate, live particles, and mean simulation time
per update.

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

## Status

Allocators, workload, peak RSS, the benchmark CLI, and the run script are
implemented. Counts run so far: 10K, 100K, and 1M. Pong is playable in a
window, and recorded matches replay through every allocator headlessly.

Next:

- Plot script under `scripts/`. Read `results/bench.csv` and graph runtime and peak memory against object count.
- 5M object count. Not run yet.
