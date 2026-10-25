#!/usr/bin/env python3
"""Compare allocator costs for identical automated or recorded Pokemon workloads."""
import argparse
import csv
import io
import random
import math
import shutil
import statistics
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ALLOCATORS = ['heap', 'pool', 'arena']


def positive(value):
    value = int(value)
    if value <= 0:
        raise argparse.ArgumentTypeError('must be positive')
    return value


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--frames', type=positive, default=None, help='automated turns (default 3600)')
    parser.add_argument('--trials', type=positive, default=10)
    parser.add_argument('--warmups', type=int, default=2)
    parser.add_argument('--seed', type=int, default=None)
    parser.add_argument('--bursts', type=positive, nargs='+', default=None,
                        help='effects per action to sweep (default 32); auto mode only')
    parser.add_argument('--trail', type=int, default=None, help='particles every step, same as Pong (default 64)')
    parser.add_argument('--dt', type=float, default=None, help='particle timestep, same as Pong (default 1/60)')
    parser.add_argument('--counts', type=positive, nargs='+', help='exact peak-live particle counts; one-turn lifetimes (default 100 turns)')
    parser.add_argument('--lifetime', type=positive, default=None)
    parser.add_argument('--replay', type=Path, help='session recorded with pokemon-play --record')
    parser.add_argument('--trace', action='store_true', help='record per-turn traces during the timing trials. This adds overhead to sim_ns. The chart is written either way')
    parser.add_argument('--no-plot', action='store_true', help='write the CSV only')
    parser.add_argument('--out', type=Path, default=ROOT / 'results' / 'pokemon.csv')
    args = parser.parse_args()
    if args.trail is not None and args.trail < 0:
        parser.error('--trail must be nonnegative')
    if args.dt is not None and (not math.isfinite(args.dt) or args.dt <= 0):
        parser.error('--dt must be finite and positive')
    if args.counts and args.dt is not None:
        parser.error('--counts uses the default timestep; omit --dt')
    if args.warmups < 0:
        parser.error('--warmups must be nonnegative')
    if args.counts and (args.bursts is not None or args.lifetime is not None or args.trail is not None):
        parser.error('--counts fixes burst=count and lifetime=1; omit --bursts and --lifetime')
    if args.replay and not args.replay.is_file():
        parser.error(f'Recording not found: {args.replay}. Record with pokemon-play --record first, or use --counts 1000000 for automated testing.')
    if args.replay and any(v is not None for v in [args.frames, args.seed, args.bursts, args.lifetime, args.counts, args.trail, args.dt]):
        parser.error('--replay uses its recorded workload; do not supply auto workload flags')
    subprocess.run(['cargo', 'build', '--release', '--no-default-features', '--bin', 'pokemon'], cwd=ROOT, check=True)
    args.out = args.out.resolve()
    args.out.parent.mkdir(parents=True, exist_ok=True)
    if args.replay:
        workloads = [('replay', ['--replay', str(args.replay.resolve())])]
    elif args.counts:
        workloads = [(f'count-{count}', ['--frames', str(args.frames or 100), '--seed', str(args.seed if args.seed is not None else 1),
                      '--burst', str(count), '--lifetime', '1', '--trail', '0']) for count in args.counts]
    else:
        workloads = [(f'burst-{burst}', ['--frames', str(args.frames or 3600), '--seed', str(args.seed if args.seed is not None else 1),
                      '--burst', str(burst), '--lifetime', str(args.lifetime or 45), '--trail', str(args.trail if args.trail is not None else 64), '--dt', str(args.dt if args.dt is not None else 1/60)]) for burst in (args.bursts or [32])]
    rows = []
    order = random.Random(1)
    fields = ['checksum', 'encounters', 'wins', 'losses', 'catches', 'level',
              'created', 'retired', 'particle_updates', 'initial_live', 'live', 'peak_live']
    trace_files = {name: [] for name, _ in workloads}
    binary = ROOT / 'target/release/pokemon'
    for name, workload in workloads:
        measured = []
        for trial in range(args.trials):
            allocators = ALLOCATORS.copy()
            order.shuffle(allocators)
            for allocator in allocators:
                trace_path = None
                if args.trace:
                    trace_path = args.out.with_name(f'{args.out.stem}-{name}-{allocator}-{trial}-trace.csv')
                    trace_files[name].append(trace_path)
                print(f'{name}: trial {trial + 1}/{args.trials}, {allocator}', flush=True)
                parsed = measure(binary, allocator, workload, args.warmups, 1, trace_path, f'{name}, {allocator}, trial {trial + 1}')
                row = parsed[0]
                if args.counts and int(row['peak_live']) != int(name.removeprefix('count-')):
                    raise SystemExit(f'{name}: requested peak-live particle count was not reached')
                row['trial'] = str(trial)
                measured.append(row)
        for field in fields:
            if len({row[field] for row in measured}) != 1:
                raise SystemExit(f'{name}: allocator/trial mismatch in {field}; results rejected')
        rows.extend(measured)
        for allocator in ALLOCATORS:
            selected = [r for r in measured if r['allocator'] == allocator]
            times = [int(r['sim_ns']) / 1e6 for r in selected]
            print(f'{name} {allocator}: median simulation {statistics.median(times):.3f} ms '
                  f'(min {min(times):.3f}, max {max(times):.3f}); '
                  f'peak live {selected[0]["peak_live"]}; '
                  f'median peak RSS {statistics.median(int(r["peak_rss_bytes"]) for r in selected):.0f} bytes',
                  flush=True)
    with args.out.open('w', newline='') as out:
        writer = csv.DictWriter(out, fieldnames=list(rows[0]))
        writer.writeheader()
        writer.writerows(rows)
    print(f'All workloads match across allocators and trials; wrote {args.out}', flush=True)
    if not args.no_plot:
        write_charts(args, rows, workloads, trace_files, fields, binary)


def measure(binary, allocator, workload, warmups, trials, trace_path, label):
    command = [str(binary), '--allocator', allocator, '--trials', str(trials), '--warmups', str(warmups), *workload]
    if trace_path is not None:
        command += ['--trace', str(trace_path)]
    result = subprocess.run(command, cwd=ROOT, text=True, capture_output=True)
    if result.returncode != 0:
        detail = result.stderr.strip() or result.stdout.strip() or 'No diagnostic output (process may have been killed).'
        raise SystemExit(f'{label} failed (exit {result.returncode}):\n{detail}')
    parsed = list(csv.DictReader(io.StringIO(result.stdout)))
    if len(parsed) != trials:
        raise SystemExit(f'{label}: expected {trials} measured row(s), got {len(parsed)}')
    return parsed


def plot_python():
    candidates = [sys.executable]
    for name in ('python3.12', 'python3.13', 'python3.11', 'python3.14', 'python3'):
        found = shutil.which(name)
        if found:
            candidates.append(found)
    candidates.append('/Library/Frameworks/Python.framework/Versions/3.12/bin/python3.12')
    checked = []
    for candidate in candidates:
        if candidate in checked or not Path(candidate).exists():
            continue
        checked.append(candidate)
        probe = subprocess.run([candidate, '-c', 'import matplotlib'], capture_output=True)
        if probe.returncode == 0:
            return candidate
    return None


def run_plot(python, csvs, png, presentation=False):
    command = [python, str(ROOT / 'scripts' / 'plot.py'), '--csv', *[str(path) for path in csvs], '--out', str(png)]
    if presentation:
        command.append('--presentation')
    result = subprocess.run(
        command,
        cwd=ROOT, text=True, capture_output=True,
    )
    if result.returncode != 0:
        detail = result.stderr.strip() or result.stdout.strip() or 'plot.py produced no output'
        raise SystemExit(f'Chart failed. The results CSV is already saved.\n{detail}')
    if result.stdout:
        print(result.stdout, end='' if result.stdout.endswith('\n') else '\n')
    if result.stderr:
        print(result.stderr, end='' if result.stderr.endswith('\n') else '\n', file=sys.stderr)


def write_charts(args, rows, workloads, trace_files, fields, binary):
    python = plot_python()
    if python is None:
        raise SystemExit('matplotlib is required to write charts: pip install matplotlib. The results CSV is already saved.')
    multi = len(workloads) > 1
    if multi:
        run_plot(python, [args.out], args.out.with_suffix('.png'))
    if not multi and not trace_files[workloads[0][0]]:
        name, workload = workloads[0]
        for allocator in ALLOCATORS:
            path = args.out.with_name(f'{args.out.stem}-{name}-{allocator}-trace.csv')
            print(f'{name}: chart sample, {allocator}', flush=True)
            parsed = measure(binary, allocator, workload, args.warmups, args.trials, path, f'{name}, {allocator}, chart')
            reference = next(row for row in rows if row['allocator'] == allocator)
            compare = fields + ['reserved_bytes']
            for sample in parsed:
                mismatched = [field for field in compare if sample[field] != reference[field]]
                if mismatched:
                    raise SystemExit(f'{name} chart sample for {allocator} does not match the timed run: {", ".join(mismatched)}')
            trace_files[name].append(path)
    for name, _ in workloads:
        paths = trace_files[name]
        if not paths:
            continue
        png = args.out.with_name(f'{args.out.stem}-{name}.png') if multi else args.out.with_suffix('.png')
        run_plot(python, paths, png, presentation=True)


if __name__ == '__main__':
    main()
