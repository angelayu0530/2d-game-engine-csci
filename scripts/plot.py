#!/usr/bin/env python3

import argparse
import csv
import statistics
import sys
from collections import defaultdict
from pathlib import Path

try:
    import matplotlib

    matplotlib.use("Agg")
    import matplotlib.pyplot as plt
except ImportError:
    sys.exit("matplotlib is required: pip install matplotlib")

ROOT = Path(__file__).resolve().parent.parent
ALLOCATORS = ["heap", "pool", "arena"]
COLORS = {"heap": "#4c78a8", "pool": "#f58518", "arena": "#54a24b"}
MARKERS = {"heap": "o", "pool": "s", "arena": "^"}
REQUIRED = ("allocator", "count", "total_ns", "peak_rss_bytes")


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Plot median runtime and peak RSS from a bench.csv against object count."
    )
    parser.add_argument("--csv", type=Path, default=ROOT / "results" / "bench.csv")
    parser.add_argument("--out", type=Path, default=ROOT / "results" / "bench.png")
    args = parser.parse_args()

    rows = load_rows(args.csv)
    counts, allocators, grouped = series(rows)
    plot(counts, allocators, grouped, args.out)
    print_table(counts, allocators, grouped)
    print(f"wrote {args.out}", file=sys.stderr)


def load_rows(path: Path) -> list[dict[str, str]]:
    if not path.is_file():
        sys.exit(f"no such file: {path}")
    with path.open(newline="") as f:
        reader = csv.DictReader(f)
        fields = reader.fieldnames or []
        missing = [name for name in REQUIRED if name not in fields]
        if missing:
            sys.exit(f"{path} is missing columns: {', '.join(missing)}")
        rows = list(reader)
    if not rows:
        sys.exit(f"{path} has no data rows")
    return rows


def series(rows):
    grouped = defaultdict(list)
    for row in rows:
        grouped[(row["allocator"], int(row["count"]))].append(row)
    counts = sorted({count for _, count in grouped})
    present = [name for name in ALLOCATORS if any(name == alloc for alloc, _ in grouped)]
    present.extend(sorted({alloc for alloc, _ in grouped if alloc not in present}))
    return counts, present, grouped


def points(grouped, allocator, counts, field, scale):
    xs, ys, lows, highs = [], [], [], []
    for count in counts:
        group = grouped.get((allocator, count))
        if not group:
            continue
        values = [int(row[field]) / scale for row in group]
        mid = statistics.median(values)
        low, high = iqr_bounds(values, mid)
        xs.append(count)
        ys.append(mid)
        lows.append(low)
        highs.append(high)
    return xs, ys, lows, highs


def iqr_bounds(values, mid):
    if len(values) < 2:
        return 0.0, 0.0
    q1, _, q3 = statistics.quantiles(values, n=4)
    return max(0.0, mid - q1), max(0.0, q3 - mid)


def plot(counts, allocators, grouped, out: Path) -> None:
    fig, axes = plt.subplots(1, 2, figsize=(9.2, 4.2), layout="constrained")
    draw(
        axes[0],
        counts,
        allocators,
        grouped,
        "total_ns",
        1e6,
        "median total runtime (ms)",
    )
    draw(
        axes[1],
        counts,
        allocators,
        grouped,
        "peak_rss_bytes",
        1e6,
        "median peak RSS (MB)",
    )
    for ax in axes:
        ax.set_xlabel("object count")
    out.parent.mkdir(parents=True, exist_ok=True)
    fig.savefig(out, dpi=150)
    plt.close(fig)


def draw(ax, counts, allocators, grouped, field, scale, ylabel) -> None:
    for index, allocator in enumerate(allocators):
        xs, ys, lows, highs = points(grouped, allocator, counts, field, scale)
        if not xs:
            continue
        ax.errorbar(
            xs,
            ys,
            yerr=[lows, highs],
            label=allocator,
            color=COLORS.get(allocator, f"C{index}"),
            marker=MARKERS.get(allocator, "o"),
            linewidth=1.6,
            capsize=3,
            markersize=6,
        )
    ax.set_xscale("log")
    ax.set_xticks(counts)
    ax.set_xticklabels([format_count(count) for count in counts])
    ax.xaxis.set_minor_locator(plt.NullLocator())
    ax.set_ylabel(ylabel)
    ax.grid(True, which="major", alpha=0.3)
    ax.legend()


def format_count(count: int) -> str:
    if count >= 1_000_000 and count % 1_000_000 == 0:
        return f"{count // 1_000_000}M"
    if count >= 1_000 and count % 1_000 == 0:
        return f"{count // 1_000}k"
    return str(count)


def print_table(counts, allocators, grouped) -> None:
    print(f"{'allocator':<9} {'count':>9} {'total ms':>10} {'peak RSS MB':>12} {'trials':>7}")
    for count in counts:
        for allocator in allocators:
            group = grouped.get((allocator, count))
            if not group:
                continue
            total_ms = statistics.median(int(row["total_ns"]) for row in group) / 1e6
            peak_mb = statistics.median(int(row["peak_rss_bytes"]) for row in group) / 1e6
            print(
                f"{allocator:<9} {count:>9} {total_ms:>10.2f} {peak_mb:>12.2f} {len(group):>7}"
            )


if __name__ == "__main__":
    main()
