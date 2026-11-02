#!/usr/bin/env python3
"""Replay one recorded boss fight through every allocator at increasing particle loads."""
import argparse
import csv
import io
import random
import statistics
import subprocess
from pathlib import Path

from bench_pokemon import plot_python, run_plot

ROOT = Path(__file__).resolve().parent.parent
BINARY = ROOT / 'target' / 'release' / 'fight'
ALLOCATORS = ['heap', 'pool', 'arena']
LOADS = [1_000, 10_000, 100_000, 1_000_000, 10_000_000]
# Identical for every allocator at one particle load.
WORK = ['checksum', 'created', 'retired', 'particle_updates', 'live', 'peak_live']
# Identical at every particle load: particles never feed back into gameplay.
GAMEPLAY = ['frames', 'game_checksum', 'knight_hp', 'boss_hp', 'victories', 'defeats']


def positive(value):
    value = int(value)
    if value <= 0:
        raise argparse.ArgumentTypeError('must be positive')
    return value


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--replay', type=Path, required=True, help='fight recorded with fight-window --record')
    parser.add_argument('--particles', type=positive, nargs='+', default=LOADS,
                        help='live particle targets to sweep (default 1k 10k 100k 1M 10M)')
    parser.add_argument('--allocators', nargs='+', choices=ALLOCATORS, default=ALLOCATORS)
    parser.add_argument('--trials', type=positive, default=3, help='processes per allocator and load')
    parser.add_argument('--warmups', type=int, default=1, help='unmeasured replays inside each process')
    parser.add_argument('--frames', type=positive, help='replay only the first N steps (shortens 10M runs)')
    parser.add_argument('--trace', action='store_true',
                        help='also write per-step trace CSVs and one per-load chart (adds overhead to sim_ns)')
    parser.add_argument('--no-plot', action='store_true', help='write the CSV only')
    parser.add_argument('--watch', action='store_true',
                        help='show every measured run in a window as it plays; drawing is excluded from sim_ns')
    parser.add_argument('--out', type=Path, default=ROOT / 'results' / 'fight.csv')
    args = parser.parse_args()
    if args.warmups < 0:
        parser.error('--warmups must be nonnegative')
    if not args.replay.is_file():
        parser.error(f'Recording not found: {args.replay}. Record one with '
                     'cargo run --release --bin fight-window -- --record results/fight.fightrec')

    build = ['cargo', 'build', '--release', '--bin', 'fight']
    if not args.watch:
        build[3:3] = ['--no-default-features']
    subprocess.run(build, cwd=ROOT, check=True)
    args.out = args.out.resolve()
    args.out.parent.mkdir(parents=True, exist_ok=True)
    replay = args.replay.resolve()

    rows = []
    traces = {}
    order = random.Random(1)
    for particles in args.particles:
        measured = []
        traces[particles] = []
        for trial in range(args.trials):
            allocators = list(args.allocators)
            order.shuffle(allocators)
            for allocator in allocators:
                trace = None
                if args.trace:
                    trace = args.out.with_name(f'{args.out.stem}-{particles}-{allocator}-{trial}-trace.csv')
                    traces[particles].append(trace)
                print(f'{particles} particles: trial {trial + 1}/{args.trials}, {allocator}', flush=True)
                row = measure(replay, allocator, particles, args, trace)
                row['trial'] = str(trial)
                measured.append(row)
        for field in WORK:
            if len({row[field] for row in measured}) != 1:
                raise SystemExit(f'{particles} particles: allocators did different work ({field}); results rejected')
        rows.extend(measured)
        for field in GAMEPLAY:
            if len({row[field] for row in rows}) != 1:
                raise SystemExit(f'{particles} particles: gameplay changed with particle load ({field}); results rejected')
        summarize(particles, measured, args.allocators)
        write_csv(args.out, rows)
    print(f'Every allocator did identical work at every load; wrote {args.out}', flush=True)

    if args.no_plot:
        return
    python = plot_python()
    if python is None:
        raise SystemExit('matplotlib is required to write charts: pip install matplotlib. The results CSV is already saved.')
    if len(args.particles) >= 2:
        run_plot(python, [args.out], args.out.with_suffix('.png'))
    for particles, paths in traces.items():
        if paths:
            run_plot(python, paths, args.out.with_name(f'{args.out.stem}-{particles}.png'), presentation=True)


def measure(replay, allocator, particles, args, trace):
    command = [str(BINARY), '--allocator', allocator, '--replay', str(replay),
               '--particles', str(particles), '--trials', '1', '--warmups', str(args.warmups)]
    if args.watch:
        command.append('--watch')
    if args.frames:
        command += ['--frames', str(args.frames)]
    if trace is not None:
        command += ['--trace', str(trace)]
    result = subprocess.run(command, cwd=ROOT, text=True, capture_output=True)
    label = f'{particles} particles, {allocator}'
    if result.returncode != 0:
        detail = result.stderr.strip() or result.stdout.strip() or 'No diagnostic output (process may have been killed).'
        raise SystemExit(f'{label} failed (exit {result.returncode}):\n{detail}')
    parsed = list(csv.DictReader(io.StringIO(result.stdout)))
    if len(parsed) != 1:
        raise SystemExit(f'{label}: expected 1 measured row, got {len(parsed)}')
    return parsed[0]


def summarize(particles, rows, allocators):
    frames = int(rows[0]['frames'])
    for allocator in allocators:
        picked = [row for row in rows if row['allocator'] == allocator]

        def median(field, scale):
            return statistics.median(int(row[field]) for row in picked) / scale

        sim_ms = median('sim_ns', 1e6)
        print(f'{particles:>10} {allocator:<5} sim {sim_ms:10.1f} ms ({sim_ms / frames:8.3f} ms/step)  '
              f'total {median("total_ns", 1e6):10.1f} ms  peak live {picked[0]["peak_live"]:>9}  '
              f'reserved {median("reserved_bytes", 1e6):8.1f} MB  peak RSS {median("peak_rss_bytes", 1e6):8.1f} MB',
              flush=True)


def write_csv(path, rows):
    with path.open('w', newline='') as out:
        writer = csv.DictWriter(out, fieldnames=list(rows[0]))
        writer.writeheader()
        writer.writerows(rows)


if __name__ == '__main__':
    main()
