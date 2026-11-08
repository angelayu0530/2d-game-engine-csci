#!/usr/bin/env python3
"""Run correctness tests and repeatable Pokémon allocator benchmarks in one command."""
import argparse
import json
import platform
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

from bench_pokemon import ROOT, plot_python, positive


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--watch', action='store_true', help='show scaling trials in the GUI')
    parser.add_argument('--replay', type=Path, help='GUI recording to replay across scaling loads')
    parser.add_argument('--preset', choices=['quick', 'scaling', 'full'], default='quick')
    parser.add_argument('--counts', type=positive, nargs='+', help='custom peak particle loads')
    parser.add_argument('--frames', type=positive, help='turns per scaling trial')
    parser.add_argument('--trials', type=positive, help='override repeats per allocator/load')
    parser.add_argument('--seeds', type=int, nargs='+', default=[1, 42, 2026])
    parser.add_argument('--out', type=Path, default=ROOT / 'results' / 'pokemon-suite')
    args = parser.parse_args()
    if args.replay and not args.replay.is_file():
        parser.error('--replay must name an existing GUI recording')
    if args.replay and args.frames is not None:
        parser.error('--replay uses all recorded turns; omit --frames')
    if any(seed < 0 or seed > 2**64 - 1 for seed in args.seeds):
        parser.error('--seeds must be unsigned 64-bit integers')
    python = plot_python()
    if python is None:
        parser.error('Charts require matplotlib. Install it with: python3 -m pip install matplotlib')
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    trials = args.trials or (10 if args.preset == 'full' else 3)
    counts = sorted(set(args.counts or ([1000, 10000, 100000] if args.preset == 'quick'
                                      else [10000, 100000, 500000, 1000000, 2500000, 5000000, 10000000])))
    if len(counts) < 2:
        parser.error('provide at least two distinct --counts for a scaling chart')
    frames = args.frames or (100 if args.preset == 'full' else 30)
    commands = []

    def run(command):
        commands.append([str(part) for part in command])
        print('\n>>> ' + ' '.join(commands[-1]), flush=True)
        subprocess.run(commands[-1], cwd=ROOT, check=True)

    metadata = dict(preset=args.preset, seeds=args.seeds, trials=trials,
                    counts=counts, frames=frames, platform=platform.platform(), python=python,
                    started=datetime.now(timezone.utc).isoformat(), commands=commands,
                    status='running')
    manifest = out / 'run.json'
    manifest.write_text(json.dumps(metadata, indent=2) + '\n')
    try:
        run(['cargo', 'test', '--no-default-features', '--lib', 'pokemon'])
        run(['cargo', 'test', '--no-default-features', '--test', 'pokemon_cli'])
        run([python, '-m', 'unittest', 'discover', '-s', 'tests', '-p', 'test_*.py'])
        common = [sys.executable, ROOT / 'scripts/bench_pokemon.py', '--trials', trials, '--warmups', 1]
        for seed in dict.fromkeys(args.seeds):
            run([*common, '--seed', seed, '--frames', 3600, '--out', out / f'expedition-seed-{seed}.csv'])
        scaling = [*common, '--particles', *counts]
        scaling += ['--replay', args.replay.resolve()] if args.replay else ['--frames', frames]
        if args.watch:
            scaling.append('--watch')
        run([*scaling, '--out', out / 'scaling.csv'])
        metadata['status'] = 'passed'
    except (subprocess.CalledProcessError, KeyboardInterrupt):
        metadata['status'] = 'failed'
        raise
    finally:
        metadata['finished'] = datetime.now(timezone.utc).isoformat()
        manifest.write_text(json.dumps(metadata, indent=2) + '\n')
    print(f'\nAll checks passed. Charts, raw CSVs, and run manifest: {out}')


if __name__ == '__main__':
    main()
