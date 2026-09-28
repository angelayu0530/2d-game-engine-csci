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
Cargo.toml              workspace; release profile used for all measurements
crates/
  engine/               library: particle type, allocator trait, three backends, workload loop
    src/particle.rs     fixed-size Particle (pos, vel, lifetime), deterministic spawn, update
    src/alloc/mod.rs    Allocator trait shared by every backend
    src/alloc/heap.rs   baseline: one allocation per object, freed individually
    src/alloc/pool.rs   fixed-size pool: preallocated slots + free list
    src/alloc/arena.rs  linear arena: bump cursor through blocks, reclaim all at once
    src/workload.rs     the fixed test loop and per-phase timing
    src/memory.rs       peak RSS measurement
  bench/                binary: runs the matrix (allocator x count), warmups, trials, CSV output
results/                CSV output and plots (gitignored except this folder)
scripts/                plotting and run-matrix helpers (to be added)
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
cargo run --release -p bench -- --help
```

## Status

Scaffold only. Every function body is `todo!()`. Implementation order:

1. `particle.rs`
2. `alloc/heap.rs`, then `pool.rs`, then `arena.rs`
3. `workload.rs` and cross-backend checksum test
4. `memory.rs`
5. `bench` harness and CSV output
6. Plot script under `scripts/`
