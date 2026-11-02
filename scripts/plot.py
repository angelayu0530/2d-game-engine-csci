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
COLORS = {"heap": "#fb6a4a", "pool": "#de2d26", "arena": "#a50f15"}
MARKERS = {"heap": "o", "pool": "s", "arena": "^"}
LINESTYLES = {"heap": "solid", "pool": "dashed", "arena": "dashdot"}
REQUIRED = ("allocator", "count", "total_ns", "peak_rss_bytes")
TRACE_REQUIRED = ("allocator", "trial", "frame", "step_ns", "live", "reserved_bytes")
MODES = ["cpu-vs-cpu", "human-replay", "human-live", "pokemon-auto", "pokemon-replay", "fight-replay"]
MODE_TITLES = {
    "pokemon-auto": "Pokemon automated expedition",
    "pokemon-replay": "Pokemon recorded player session",
    "cpu-vs-cpu": "CPU vs CPU",
    "human-replay": "human match, replayed headless",
    "human-live": "human match, live window",
    "fight-replay": "Boss fight, recorded human session",
}
SMOOTH_FRAMES = 30
# size_of::<Particle>() in src/particle.rs (pos, vel: [f32; 2], lifetime: u32).
PARTICLE_BYTES = 20


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Plot median runtime and peak RSS from a bench.csv against object count, "
        "the same pair from a Pokemon or Pong summary CSV, "
        "or per-frame step time and memory from --trace CSVs."
    )
    parser.add_argument(
        "--csv", type=Path, nargs="+", default=[ROOT / "results" / "bench.csv"]
    )
    parser.add_argument("--out", type=Path, default=None)
    parser.add_argument("--presentation", action="store_true", help="Wide slide layout with benchmark title and metric footer (trace CSVs)")
    parser.add_argument("--by-allocator", action="store_true", help="Separate allocator columns with shared metric scales (trace CSVs)")
    args = parser.parse_args()

    tables = [load_rows(path) for path in args.csv]
    if all(is_trace(fields) for fields, _ in tables):
        out = args.out or ROOT / "results" / "trace.png"
        try:
            traces = trace_series(args.csv, [rows for _, rows in tables])
        except ValueError as error:
            sys.exit(str(error))
        plot_trace(traces, out, presentation=args.presentation, by_allocator=args.by_allocator)
        print_trace_table(traces)
    elif len(tables) == 1 and is_bench(tables[0][0]):
        _, rows = tables[0]
        out = args.out or ROOT / "results" / "bench.png"
        counts, allocators, grouped = series(rows)
        plot(counts, allocators, grouped, out)
        print_table(counts, allocators, grouped)
    elif len(tables) == 1 and is_game_summary(tables[0][0]):
        out = args.out or args.csv[0].with_suffix(".png")
        write_game_summary(tables[0][1], out)
    elif len(tables) == 1:
        missing = [name for name in REQUIRED if name not in tables[0][0]]
        sys.exit(f"{args.csv[0]} is missing columns: {', '.join(missing)}")
    else:
        sys.exit("pass one bench.csv, one game summary CSV, or one or more trace CSVs")
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


def is_bench(fields) -> bool:
    return all(name in fields for name in REQUIRED)


def is_game_summary(fields) -> bool:
    return {"allocator", "sim_ns", "peak_rss_bytes"} <= set(fields) and (
        "peak_live" in fields or "burst" in fields
    )


def scale_axis(rows):
    def values(name):
        if not rows or name not in rows[0]:
            return set()
        return {int(row[name]) for row in rows}

    particles = values("particles")
    if len(particles) >= 2:
        return "particles", "target live particles"
    live = values("peak_live")
    burst = values("burst")
    if len(live) >= 2 and live == burst:
        return "peak_live", "object count"
    if len(live) >= 2:
        return "peak_live", "peak live particles"
    if len(burst) >= 2:
        return "burst", "effects per action"
    return None, None


def write_game_summary(rows, out: Path) -> None:
    field, xlabel = scale_axis(rows)
    if field is None:
        sys.exit(
            "summary CSV has one particle load. "
            "Pass trace CSVs to plot step time, live particles, and reserved memory."
        )
    counts, allocators, grouped = series(rows, field)
    if field == "particles" and "frames" in rows[0]:
        # Boss fight: one step is one frame, so show how many frames per
        # second the simulation alone could sustain at each load.
        for row in rows:
            row["fps"] = str(int(row["frames"]) * 1e9 / int(row["sim_ns"]))
            row["rss_per_particle"] = str(int(row["peak_rss_bytes"]) / int(row["peak_live"]))
        plot(
            counts,
            allocators,
            grouped,
            out,
            time_field="fps",
            time_label="median simulated frames per second\n(game logic only, higher is better)",
            time_scale=1,
            xlabel=xlabel,
            log_y=True,
            fps_lines=(60, 30),
            mem_field="rss_per_particle",
            mem_scale=1,
            mem_label="median peak RSS per peak live particle (bytes)\n(whole process, lower is better)",
            mem_line=(PARTICLE_BYTES, f"Particle struct ({PARTICLE_BYTES} B)"),
        )
        print_table(counts, allocators, grouped, time_field="fps", time_header="fps", scale=1)
        print_per_particle(counts, allocators, grouped)
        return
    plot(
        counts,
        allocators,
        grouped,
        out,
        time_field="sim_ns",
        time_label="median simulation time (ms)",
        xlabel=xlabel,
        log_y=True,
    )
    print_table(counts, allocators, grouped, time_field="sim_ns", time_header="sim ms")


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
        trials = defaultdict(dict)
        for path, rows in sources:
            for row in rows:
                key = (str(path), row["trial"])
                frame = int(row["frame"])
                if frame in trials[key]:
                    raise ValueError(f"Duplicate frame {frame} in {path}, trial {row['trial']}")
                trials[key][frame] = row
        reference = next(iter(trials.values()))
        xs = sorted(reference)
        def state(row):
            if mode.startswith("pokemon-"):
                score = (int(row["wins"]), int(row["catches"]))
            elif mode.startswith("fight-"):
                score = (int(row["knight_hp"]), int(row["boss_hp"]))
            else:
                score = (int(row["score_left"]), int(row["score_right"]))
            return (int(row["live"]), score)
        for trial in trials.values():
            if sorted(trial) != xs or any(state(trial[x]) != state(reference[x]) for x in xs):
                raise ValueError(f"{mode}/{allocator}: traces contain different workloads. Plot each particle load or session separately.")
        series.append({
            "mode": mode, "label": allocator, "allocator": allocator, "frames": xs,
            "step_us": [statistics.median(int(t[x]["step_ns"]) for t in trials.values()) / 1e3 for x in xs],
            "live": [state(reference[x])[0] for x in xs],
            "reserved_kb": [statistics.median(int(t[x]["reserved_bytes"]) for t in trials.values()) / 1e3 for x in xs],
            "score": [state(reference[x])[1] for x in xs],
            "trials": len(trials),
        })
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


def plot_trace(series, out: Path, presentation=False, by_allocator=False) -> None:
    modes = list(dict.fromkeys(s["mode"] for s in series))
    columns = ([[s] for s in series] if by_allocator else
               [[s for s in series if s["mode"] == mode] for mode in modes])
    fig, axes = plt.subplots(
        3,
        len(columns),
        figsize=((13.333, 7.5) if presentation else ((10, 8.5) if len(columns) == 1 else (7 * len(columns), 8.5))),
        sharey="row",
        squeeze=False,
        layout=None if presentation else "constrained",
    )
    if presentation:
        fig.subplots_adjust(left=0.11, right=0.97, top=0.79, bottom=0.11, hspace=0.25, wspace=0.07)
        title = "Game"
        if all(mode.startswith("pokemon-") for mode in modes):
            title = "Pokémon"
        elif all(mode.startswith("fight-") for mode in modes):
            title = "Boss Fight"
        elif all(not mode.startswith(("pokemon-", "fight-")) for mode in modes):
            title = "Pong"
        fig.text(0.025, 0.955, f"{title} Benchmark Results", fontfamily="serif", fontsize=36, va="top")
        fig.add_artist(plt.Line2D([0, 1], [0.855, 0.855], transform=fig.transFigure, color="0.3", linewidth=1))
        fig.text(0.025, 0.025, "Step time (top)  •  Live particles (middle)  •  Reserved memory (bottom)",
                 fontfamily="serif", fontsize=19, va="bottom")
    for col, column in enumerate(columns):
        mode = column[0]["mode"]
        top, middle, bottom = axes[0][col], axes[1][col], axes[2][col]
        for index, s in enumerate(column):
            color = COLORS.get(s["allocator"], f"C{index}") if len(column) <= 3 else f"C{index}"
            top.plot(
                s["frames"],
                rolling_median(s["step_us"], SMOOTH_FRAMES),
                color=color,
                linewidth=1.6,
                label=s["label"],
            )
            live = rolling_median(s["live"], SMOOTH_FRAMES) if mode.startswith("pokemon-") else s["live"]
            reserved = rolling_median(s["reserved_kb"], SMOOTH_FRAMES) if mode.startswith("pokemon-") else s["reserved_kb"]
            middle.plot(s["frames"], live, color=color, linewidth=1.4, label=s["label"])
            bottom.plot(s["frames"], reserved, color=color, linewidth=1.4, label=s["label"])

        reference = column[0]
        points = [
            frame
            for frame, prev, cur in zip(
                reference["frames"][1:], reference["score"], reference["score"][1:]
            )
            if cur != prev
        ]
        final = reference["score"][-1]
        if mode.startswith("pokemon-"):
            summary = f"{len(reference['frames'])} turns, {final[0]} wins, {final[1]} catches"
        elif mode.startswith("fight-"):
            summary = f"{len(reference['frames'])} frames, knight {final[0]} / boss {final[1]} hearts"
        else:
            summary = f"{len(reference['frames'])} frames, final {final[0]}:{final[1]}"
        trial_counts = {s["trials"] for s in column}
        trial_note = f", median of {next(iter(trial_counts))} trials" if len(trial_counts) == 1 and next(iter(trial_counts)) > 1 else ""
        if by_allocator:
            top.set_title(f"{reference['allocator'].upper()} — {MODE_TITLES.get(mode, mode)}\n{summary}\nMedian of {reference['trials']} trials", fontsize=10)
        elif presentation:
            top.set_title(f"{MODE_TITLES.get(mode, mode)}\n{summary}", fontsize=11)
        else:
            top.set_title(f"{MODE_TITLES.get(mode, mode)}\n{summary}{trial_note}")
        show_events = len(points) <= 40
        for ax in (top, middle, bottom):
            for frame in points if show_events else []:
                ax.axvline(frame, color="0.6", linewidth=0.6, linestyle=":")
            ax.grid(True, alpha=0.3)
            ax.legend(loc="upper right", fontsize=8 if presentation else None, framealpha=1)
        top.set_yscale("log")
        unit = "turn" if mode.startswith("pokemon-") else "frame"
        event = ("wins/catches" if mode.startswith("pokemon-") else
                 "hearts lost or healed" if mode.startswith("fight-") else "points scored")
        bottom.set_xlabel(f"{unit} (dotted lines: {event})" if show_events else unit)

    step_unit = "turn" if modes and all(mode.startswith("pokemon-") for mode in modes) else "frame"
    axes[0][0].set_ylabel(f"step time (µs)\n{SMOOTH_FRAMES}-{step_unit} rolling median")
    if modes and all(mode.startswith("pokemon-") for mode in modes):
        axes[1][0].set_ylabel(f"live particles\n{SMOOTH_FRAMES}-{step_unit} rolling median")
        axes[2][0].set_ylabel(f"reserved (KB)\n{SMOOTH_FRAMES}-{step_unit} rolling median")
    else:
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


def series(rows, count_field="count"):
    grouped = defaultdict(list)
    for row in rows:
        grouped[(row["allocator"], int(row[count_field]))].append(row)
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
        values = [float(row[field]) / scale for row in group]
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


def plot(
    counts,
    allocators,
    grouped,
    out: Path,
    time_field="total_ns",
    time_label="median total runtime (ms)",
    time_scale=1e6,
    xlabel="object count",
    log_y=False,
    fps_lines=(),
    mem_field="peak_rss_bytes",
    mem_scale=1e6,
    mem_label="median peak RSS (MB)",
    mem_line=None,
) -> None:
    fig, axes = plt.subplots(1, 2, figsize=(9.2, 4.2), layout="constrained")
    draw(axes[0], counts, allocators, grouped, time_field, time_scale, time_label, log_y=log_y)
    for fps in fps_lines:
        axes[0].axhline(fps, color="0.35", linewidth=1, linestyle=":")
        axes[0].annotate(f"{fps} fps", (0.02, fps), xycoords=("axes fraction", "data"),
                         xytext=(0, 3), textcoords="offset points", fontsize=8, color="0.3")
    if fps_lines:
        axes[0].yaxis.set_major_formatter(plt.FuncFormatter(lambda v, _: f"{v:,.0f}"))
        top = format_count(counts[-1])
        handles, labels = axes[0].get_legend_handles_labels()
        for i, allocator in enumerate(labels):
            _, ys, _, _ = points(grouped, allocator, counts, time_field, time_scale)
            if ys:
                labels[i] = f"{allocator}  ({ys[-1]:.0f} fps @ {top})"
        axes[0].legend(handles, labels, loc="upper right", fontsize=9)
    draw(axes[1], counts, allocators, grouped, mem_field, mem_scale, mem_label, log_y=log_y)
    if mem_line is not None:
        value, label = mem_line
        axes[1].axhline(value, color="0.35", linewidth=1, linestyle=":")
        axes[1].annotate(label, (0.02, value), xycoords=("axes fraction", "data"),
                         xytext=(0, 3), textcoords="offset points", fontsize=8, color="0.3")
        axes[1].yaxis.set_major_formatter(plt.FuncFormatter(lambda v, _: f"{v:,.0f}"))
        top = format_count(counts[-1])
        handles, labels = axes[1].get_legend_handles_labels()
        for i, allocator in enumerate(labels):
            _, ys, _, _ = points(grouped, allocator, counts, mem_field, mem_scale)
            if ys:
                labels[i] = f"{allocator}  ({ys[-1]:.0f} B @ {top})"
        axes[1].legend(handles, labels, loc="upper right", fontsize=9)
    for ax in axes:
        ax.set_xlabel(xlabel)
    out.parent.mkdir(parents=True, exist_ok=True)
    fig.savefig(out, dpi=150)
    plt.close(fig)


def draw(ax, counts, allocators, grouped, field, scale, ylabel, log_y=False) -> None:
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
            linestyle=LINESTYLES.get(allocator, "solid"),
            linewidth=1.6,
            capsize=3,
            markersize=6,
        )
    ax.set_xscale("log")
    if log_y:
        ax.set_yscale("log")
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


def print_per_particle(counts, allocators, grouped) -> None:
    print(f"{'allocator':<9} {'count':>9} {'peak live':>10} {'RSS B/particle':>15} {'reserved B/particle':>20}")
    for count in counts:
        for allocator in allocators:
            group = grouped.get((allocator, count))
            if not group:
                continue
            rss = statistics.median(float(row["rss_per_particle"]) for row in group)
            reserved = statistics.median(int(row["reserved_bytes"]) / int(row["peak_live"]) for row in group)
            print(f"{allocator:<9} {count:>9} {group[0]['peak_live']:>10} {rss:>15.1f} {reserved:>20.1f}")


def print_table(counts, allocators, grouped, time_field="total_ns", time_header="total ms", scale=1e6) -> None:
    print(f"{'allocator':<9} {'count':>9} {time_header:>10} {'peak RSS MB':>12} {'trials':>7}")
    for count in counts:
        for allocator in allocators:
            group = grouped.get((allocator, count))
            if not group:
                continue
            time = statistics.median(float(row[time_field]) for row in group) / scale
            peak_mb = statistics.median(int(row["peak_rss_bytes"]) for row in group) / 1e6
            print(
                f"{allocator:<9} {count:>9} {time:>10.2f} {peak_mb:>12.2f} {len(group):>7}"
            )


if __name__ == "__main__":
    main()
