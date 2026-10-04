#!/usr/bin/env python3

import argparse
import csv
import io
import statistics
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BIN = ROOT / "target" / "release" / "game-engine"
ALLOCATORS = ["heap", "pool", "arena"]


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Run the release benchmark for each allocator and summarize the results."
    )
    parser.add_argument("--runs", type=int, default=3, help="processes per allocator and count")
    parser.add_argument("--counts", type=int, nargs="+", default=[10_000, 100_000, 1_000_000])
    parser.add_argument("--allocators", nargs="+", choices=ALLOCATORS, default=ALLOCATORS)
    parser.add_argument("--updates", type=int, default=100)
    parser.add_argument("--warmups", type=int, default=2)
    parser.add_argument("--trials", type=int, default=10, help="measured trials per process")
    parser.add_argument("--out", type=Path, default=ROOT / "results" / "bench.csv")
    args = parser.parse_args()

    subprocess.run(["cargo", "build", "--release", "--quiet"], cwd=ROOT, check=True)

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


if __name__ == "__main__":
    main()
