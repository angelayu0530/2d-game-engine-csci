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
TRACE_REQUIRED = ("allocator", "trial", "frame", "step_ns", "live", "reserved_bytes")
MODES = ["cpu-vs-cpu", "human-replay", "human-live"]
MODE_TITLES = {
    "cpu-vs-cpu": "CPU vs CPU",
    "human-replay": "human match, replayed headless",
    "human-live": "human match, live window",
}
SMOOTH_FRAMES = 30


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Plot median runtime and peak RSS from a bench.csv against object count, "
        "or per-frame step time and memory from Pong --trace CSVs."
    )
    parser.add_argument(
        "--csv", type=Path, nargs="+", default=[ROOT / "results" / "bench.csv"]
    )
    parser.add_argument("--out", type=Path, default=None)
    args = parser.parse_args()

    tables = [load_rows(path) for path in args.csv]
    if all(is_trace(fields) for fields, _ in tables):
        out = args.out or ROOT / "results" / "trace.png"
        traces = trace_series(args.csv, [rows for _, rows in tables])
        plot_trace(traces, out)
        print_trace_table(traces)
    elif len(tables) == 1 and not is_trace(tables[0][0]):
        fields, rows = tables[0]
        missing = [name for name in REQUIRED if name not in fields]
        if missing:
            sys.exit(f"{args.csv[0]} is missing columns: {', '.join(missing)}")
        out = args.out or ROOT / "results" / "bench.png"
        counts, allocators, grouped = series(rows)
        plot(counts, allocators, grouped, out)
        print_table(counts, allocators, grouped)
    else:
        sys.exit("pass one bench.csv, or one or more Pong --trace CSVs")
    print(f"wrote {out}", file=sys.stderr)


def load_rows(path: Path) -> tuple[list[str], list[dict[str, str]]]:
    if not path.is_file():
        sys.exit(f"no such file: {path}")
    with path.open(newline="") as f:
        reader = csv.DictReader(f)
        fields = reader.fieldnames or []
        rows = list(reader)
    if not rows:
        sys.exit(f"{path} has no data rows")
    return fields, rows


def is_trace(fields) -> bool:
    return all(name in fields for name in TRACE_REQUIRED)


def trace_series(paths, tables):
    groups = defaultdict(list)
    for path, rows in zip(paths, tables):
        keys = sorted({(row.get("mode", "untagged"), row["allocator"]) for row in rows})
        for mode, allocator in keys:
            picked = [
                r for r in rows if r.get("mode", "untagged") == mode and r["allocator"] == allocator
            ]
            groups[(mode, allocator)].append((path, picked))
    series = []
    for (mode, allocator), sources in groups.items():
        for path, rows in sources:
            label = allocator if len(sources) == 1 else f"{allocator} ({path.stem})"
            frames = defaultdict(list)
            for row in rows:
                frames[int(row["frame"])].append(row)
            xs = sorted(frames)
            first = [frames[x][0] for x in xs]
            series.append(
                {
                    "mode": mode,
                    "label": label,
                    "allocator": allocator,
                    "frames": xs,
                    "step_us": [
                        statistics.median(int(r["step_ns"]) for r in frames[x]) / 1e3 for x in xs
                    ],
                    "live": [int(r["live"]) for r in first],
                    "reserved_kb": [int(r["reserved_bytes"]) / 1e3 for r in first],
                    "score": [(int(r["score_left"]), int(r["score_right"])) for r in first],
                    "trials": len({r["trial"] for r in rows}),
                }
            )
    mode_order = {name: i for i, name in enumerate(MODES)}
    alloc_order = {name: i for i, name in enumerate(ALLOCATORS)}
    series.sort(
        key=lambda s: (
            mode_order.get(s["mode"], len(mode_order)),
            alloc_order.get(s["allocator"], len(alloc_order)),
            s["label"],
        )
    )
    return series


def rolling_median(values, window):
    half = window // 2
    return [
        statistics.median(values[max(0, i - half) : i + half + 1]) for i in range(len(values))
    ]


def plot_trace(series, out: Path) -> None:
    modes = list(dict.fromkeys(s["mode"] for s in series))
    fig, axes = plt.subplots(
        3,
        len(modes),
        figsize=(7 * len(modes), 8.5),
        sharey="row",
        squeeze=False,
        layout="constrained",
    )
    for col, mode in enumerate(modes):
        column = [s for s in series if s["mode"] == mode]
        top, middle, bottom = axes[0][col], axes[1][col], axes[2][col]
        for index, s in enumerate(column):
            color = COLORS.get(s["allocator"], f"C{index}") if len(column) <= 3 else f"C{index}"
            top.plot(s["frames"], s["step_us"], color=color, alpha=0.15, linewidth=0.6)
            top.plot(
                s["frames"],
                rolling_median(s["step_us"], SMOOTH_FRAMES),
                color=color,
                linewidth=1.6,
                label=s["label"],
            )
            middle.plot(s["frames"], s["live"], color=color, linewidth=1.2, label=s["label"])
            bottom.plot(s["frames"], s["reserved_kb"], color=color, linewidth=1.4, label=s["label"])

        reference = column[0]
        points = [
            frame
            for frame, prev, cur in zip(
                reference["frames"][1:], reference["score"], reference["score"][1:]
            )
            if cur != prev
        ]
        final = reference["score"][-1]
        top.set_title(
            f"{MODE_TITLES.get(mode, mode)}\n"
            f"{len(reference['frames'])} frames, final {final[0]}:{final[1]}"
        )
        for ax in (top, middle, bottom):
            for frame in points:
                ax.axvline(frame, color="0.6", linewidth=0.6, linestyle=":")
            ax.grid(True, alpha=0.3)
            ax.legend(loc="upper right")
        top.set_yscale("log")
        bottom.set_xlabel("frame (dotted lines: points scored)")

    axes[0][0].set_ylabel(f"step time (µs)\n{SMOOTH_FRAMES}-frame rolling median")
    axes[1][0].set_ylabel("live particles")
    axes[2][0].set_ylabel("reserved (KB)")
    out.parent.mkdir(parents=True, exist_ok=True)
    fig.savefig(out, dpi=150)
    plt.close(fig)


def print_trace_table(series) -> None:
    print(
        f"{'mode':<13} {'series':<20} {'frames':>7} {'trials':>6} {'median µs':>10} "
        f"{'p99 µs':>8} {'max µs':>8} {'peak live':>10} {'peak KB':>9}"
    )
    for s in series:
        steps = s["step_us"]
        p99 = statistics.quantiles(steps, n=100)[98] if len(steps) >= 2 else steps[0]
        print(
            f"{s['mode']:<13} {s['label']:<20} {len(steps):>7} {s['trials']:>6} "
            f"{statistics.median(steps):>10.2f} {p99:>8.2f} {max(steps):>8.2f} "
            f"{max(s['live']):>10} {max(s['reserved_kb']):>9.1f}"
        )


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
