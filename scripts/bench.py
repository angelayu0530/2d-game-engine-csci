#!/usr/bin/env python3
"""Benchmark every allocator.

1. Summary: the basic workload, repeated per allocator and count (results/bench.csv, bench.svg).
2. Traces: every game at every count, one fresh process per allocator, one CSV row per frame
   (results/traces/<count>/<game>-<allocator>.csv), charted as one games x metrics grid per
   count (results/bench-<count>.svg).
"""

import argparse
import csv
import io
import statistics
import subprocess
import sys
from pathlib import Path

from bench_pokemon import plot_python, run_plot

ROOT = Path(__file__).resolve().parent.parent
RELEASE = ROOT / "target" / "release"
BIN = RELEASE / "game-engine"
ALLOCATORS = ["heap", "pool", "arena"]
GAMES = ["basic", "pong", "pokemon", "fight"]
# Particle lifetime in steps for Pong and Pokemon (both binaries' default).
LIFETIME = 45
# Summary columns that must match across allocators, where a game reports them.
SUMMARY_WORK = ["checksum", "game_checksum", "created", "retired", "particle_updates", "peak_live"]
# Trace columns that describe the workload, not its cost.
TRACE_WORK = ["frame", "live", "created", "retired"]


def main() -> None:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("--runs", type=int, default=3, help="summary: processes per allocator and count")
    parser.add_argument("--counts", type=int, nargs="+", default=[10_000, 100_000, 1_000_000])
    parser.add_argument("--allocators", nargs="+", choices=ALLOCATORS, default=ALLOCATORS)
    parser.add_argument("--updates", type=int, default=100, help="summary: update passes per trial")
    parser.add_argument("--warmups", type=int, default=2, help="summary: warmup runs per process")
    parser.add_argument("--trials", type=int, default=10, help="summary: measured trials per process")
    parser.add_argument("--out", type=Path, default=ROOT / "results" / "bench.csv")
    parser.add_argument("--games", nargs="+", choices=GAMES, default=GAMES, help="traces: grid columns")
    parser.add_argument("--frames", type=int, default=600,
                        help="traces: frames per game (basic: update passes, plus spawn and teardown)")
    parser.add_argument("--fight-replay", type=Path, default=ROOT / "results" / "fight.fightrec",
                        help="traces: boss fight recorded with fight-window --record")
    parser.add_argument("--trace-dir", type=Path, default=ROOT / "results" / "traces")
    parser.add_argument("--no-summary", action="store_true", help="skip the basic-workload summary")
    parser.add_argument("--no-traces", action="store_true", help="skip the per-frame game traces")
    parser.add_argument("--no-plot", action="store_true", help="write CSVs only")
    args = parser.parse_args()
    if args.frames <= 0:
        parser.error("--frames must be positive")
    if not args.no_traces and "fight" in args.games and not args.fight_replay.is_file():
        parser.error(f"no fight recording at {args.fight_replay}; record one with "
                     "fight-window --record, or drop fight from --games")
    python = None
    if not args.no_plot:
        python = plot_python()
        if python is None:
            parser.error("matplotlib is required for charts: pip install matplotlib, or pass --no-plot")

    subprocess.run(["cargo", "build", "--release", "--quiet"], cwd=ROOT, check=True)

    if not args.no_summary:
        run_summary(args)
        if python:
            run_plot(python, [args.out], args.out.with_suffix(".svg"))
    if not args.no_traces:
        for count in args.counts:
            paths = run_traces(args, count)
            if python:
                label = format_count(count)
                title = (f"{label} target live particles: one fresh process per allocator, "
                         f"no warmup, {args.frames} frames")
                run_plot(python, paths, args.out.with_name(f"{args.out.stem}-{label}.svg"),
                         extra=["--grid", "--title", title])


def run_summary(args):
    rows = []
    for count in args.counts:
        for allocator in args.allocators:
            for run in range(args.runs):
                print(f"{allocator:>5} count={count} run {run + 1}/{args.runs}", file=sys.stderr)
                out = subprocess.run(
                    [
                        str(BIN),
                        f"--allocator={allocator}",
                        f"--count={count}",
                        f"--updates={args.updates}",
                        f"--warmups={args.warmups}",
                        f"--trials={args.trials}",
                    ],
                    check=True,
                    capture_output=True,
                    text=True,
                ).stdout
                for row in csv.DictReader(io.StringIO(out)):
                    row["run"] = str(run)
                    rows.append(row)

    args.out.parent.mkdir(parents=True, exist_ok=True)
    with args.out.open("w", newline="") as f:
        writer = csv.DictWriter(f, fieldnames=["run"] + [k for k in rows[0] if k != "run"])
        writer.writeheader()
        writer.writerows(rows)
    print(f"wrote {len(rows)} rows to {args.out}", file=sys.stderr)

    summarize(rows, args.counts, args.allocators)


def run_traces(args, count):
    folder = args.trace_dir / format_count(count)
    folder.mkdir(parents=True, exist_ok=True)
    paths = []
    for game in args.games:
        summaries, traces = {}, {}
        for allocator in args.allocators:
            path = folder / f"{game}-{allocator}.csv"
            print(f"trace {game:>7} {allocator:>5} count={count}", file=sys.stderr)
            summaries[allocator] = run_one(trace_command(game, allocator, count, args, path),
                                           f"{game}, {allocator}, count={count}")
            with path.open(newline="") as f:
                traces[allocator] = list(csv.DictReader(f))
            paths.append(path)
        check_same_work(game, count, summaries, traces)
        summarize_traces(game, count, summaries, traces)
    return paths


def trace_command(game, allocator, count, args, trace):
    common = ["--allocator", allocator, "--warmups", "0", "--trials", "1", "--trace", str(trace)]
    frames = str(args.frames)
    if game == "basic":
        return [str(BIN), "--count", str(count), "--updates", frames, *common]
    # Games also draw every frame offscreen, as their windows do, so FPS includes drawing.
    if game == "fight":
        return [str(RELEASE / "fight"), "--replay", str(args.fight_replay),
                "--particles", str(count), "--frames", frames, "--draw", *common]
    trail, burst = effect_load(game, count)
    return [str(RELEASE / game), "--frames", frames, "--trail", str(trail), "--burst", str(burst),
            "--lifetime", str(LIFETIME), "--draw", *common]


def effect_load(game, count):
    """Trail and burst sizes, in the games' default 2:1 ratio, that keep about `count`
    particles alive. Pong bursts only on bounces and points; Pokemon bursts on nearly
    every turn, so its trail carries two thirds of the load."""
    per_step = count / LIFETIME
    trail = per_step if game == "pong" else per_step * 2 / 3
    return round(trail), round(trail / 2)


def run_one(command, label):
    result = subprocess.run(command, cwd=ROOT, text=True, capture_output=True)
    if result.returncode != 0:
        detail = result.stderr.strip() or result.stdout.strip() or "no output (killed?)"
        sys.exit(f"{label} failed (exit {result.returncode}):\n{detail}")
    rows = list(csv.DictReader(io.StringIO(result.stdout)))
    if len(rows) != 1:
        sys.exit(f"{label}: expected one summary row, got {len(rows)}")
    return rows[0]


def check_same_work(game, count, summaries, traces):
    allocators = list(summaries)
    first = allocators[0]
    for allocator in allocators[1:]:
        for field in SUMMARY_WORK:
            if field in summaries[first] and summaries[allocator][field] != summaries[first][field]:
                sys.exit(f"{game} count={count}: {field} differs between {first} and {allocator}")
        a, b = traces[first], traces[allocator]
        if len(a) != len(b) or any(x[f] != y[f] for x, y in zip(a, b) for f in TRACE_WORK):
            sys.exit(f"{game} count={count}: per-frame workload differs between {first} and {allocator}")


def summarize_traces(game, count, summaries, traces):
    print(f"\n{game} count={count}: {len(next(iter(traces.values())))} frames")
    print(f"{'allocator':<9} {'avg fps':>9} {'median fps':>11} {'1% low fps':>11} {'min fps':>8} "
          f"{'peak live':>10} {'reserved MB':>12} {'peak RSS MB':>12} {'faults':>8}")
    for allocator, rows in traces.items():
        steps = [(int(r["step_ns"]) + int(r.get("draw_ns", 0))) / 1e9 for r in rows]
        p99 = statistics.quantiles(steps, n=100)[98] if len(steps) >= 2 else steps[0]
        print(
            f"{allocator:<9} {len(steps) / sum(steps):>9.0f} {1 / statistics.median(steps):>11.0f} "
            f"{1 / p99:>11.0f} {1 / max(steps):>8.0f} {max(int(r['live']) for r in rows):>10} "
            f"{max(int(r['reserved_bytes']) for r in rows) / 1e6:>12.2f} "
            f"{int(summaries[allocator]['peak_rss_bytes']) / 1e6:>12.2f} "
            f"{sum(int(r['minor_faults']) for r in rows):>8}"
        )


def summarize(rows, counts, allocators) -> None:
    print(
        f"{'allocator':<9} {'count':>9} {'total ms':>10} {'spread':>8} {'spawn ms':>9} "
        f"{'update ms':>10} {'retire ms':>10} {'reclaim ms':>11} {'Mupd/s':>8} "
        f"{'reserved MB':>12} {'peak RSS MB':>12}"
    )
    for count in counts:
        checksums = set()
        for allocator in allocators:
            group = [r for r in rows if r["allocator"] == allocator and int(r["count"]) == count]
            if not group:
                continue
            checksums.update(r["checksum"] for r in group)

            def ms(field):
                return statistics.median(int(r[field]) for r in group) / 1e6

            totals = [int(r["total_ns"]) / 1e6 for r in group]
            updates = int(group[0]["updates"])
            update_s = ms("update_ns") / 1e3
            print(
                f"{allocator:<9} {count:>9} {statistics.median(totals):>10.2f} "
                f"{spread(totals):>7.1f}% {ms('spawn_ns'):>9.2f} {ms('update_ns'):>10.2f} "
                f"{ms('retire_ns'):>10.2f} {ms('reclaim_ns'):>11.2f} "
                f"{count * updates / update_s / 1e6:>8.1f} "
                f"{max(int(r['reserved_bytes']) for r in group) / 1e6:>12.2f} "
                f"{max(int(r['peak_rss_bytes']) for r in group) / 1e6:>12.2f}"
            )
        if len(checksums) > 1:
            sys.exit(f"checksum mismatch at count={count}: {sorted(checksums)}")


def spread(values) -> float:
    if len(values) < 2:
        return 0.0
    q1, _, q3 = statistics.quantiles(values, n=4)
    return (q3 - q1) / statistics.median(values) * 100


def format_count(count: int) -> str:
    if count >= 1_000_000 and count % 1_000_000 == 0:
        return f"{count // 1_000_000}M"
    if count >= 1_000 and count % 1_000 == 0:
        return f"{count // 1_000}k"
    return str(count)


if __name__ == "__main__":
    main()
