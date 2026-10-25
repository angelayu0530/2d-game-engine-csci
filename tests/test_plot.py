"""Run with a Python environment containing matplotlib."""
import importlib.util
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('plot', Path(__file__).parents[1] / 'scripts/plot.py')
plot = importlib.util.module_from_spec(spec)
spec.loader.exec_module(plot)


def row(time, live=10, trial='0'):
    return dict(mode='pokemon-auto', allocator='heap', frame='0', trial=trial,
                step_ns=str(time), live=str(live), reserved_bytes='200', wins='0', catches='0')


class TraceTests(unittest.TestCase):
    def test_files_are_trials_not_lines(self):
        series = plot.trace_series([Path('a.csv'), Path('b.csv')], [[row(1000)], [row(3000)]])
        self.assertEqual(len(series), 1)
        self.assertEqual(series[0]['label'], 'heap')
        self.assertEqual(series[0]['trials'], 2)
        self.assertEqual(series[0]['step_us'], [2.0])

    def test_multiple_trials_in_one_file(self):
        series = plot.trace_series([Path('a.csv')], [[row(1000), row(3000, trial='1')]])
        self.assertEqual(series[0]['step_us'], [2.0])
        self.assertEqual(series[0]['trials'], 2)

    def test_different_particle_loads_are_rejected(self):
        with self.assertRaisesRegex(ValueError, 'different workloads'):
            plot.trace_series([Path('a.csv'), Path('b.csv')], [[row(1000)], [row(3000, live=20)]])


def summary_row(allocator, count, sim, rss, burst=None):
    return {
        'allocator': allocator,
        'peak_live': str(count),
        'burst': str(count if burst is None else burst),
        'sim_ns': str(sim),
        'peak_rss_bytes': str(rss),
    }


class SummaryTests(unittest.TestCase):
    def test_object_count_axis_when_peak_live_matches_burst(self):
        rows = [summary_row('heap', 10000, 1, 1), summary_row('heap', 1000000, 2, 2)]
        self.assertEqual(plot.scale_axis(rows), ('peak_live', 'object count'))

    def test_peak_live_axis_when_burst_is_not_the_population(self):
        rows = [
            summary_row('heap', 1000, 1, 1, burst=32),
            summary_row('heap', 5000, 1, 1, burst=256),
        ]
        self.assertEqual(plot.scale_axis(rows), ('peak_live', 'peak live particles'))

    def test_single_load_has_no_scale_axis(self):
        self.assertEqual(plot.scale_axis([summary_row('heap', 10, 1, 1)]), (None, None))

    def test_summary_png_uses_simulation_time(self):
        rows = []
        for allocator, sim in (('heap', 500), ('pool', 100), ('arena', 200)):
            for count in (10000, 1000000):
                rows.append(summary_row(allocator, count, sim * count, count * 30))
        out = Path(tempfile.mkdtemp()) / 'summary.png'
        plot.write_game_summary(rows, out)
        self.assertGreater(out.stat().st_size, 1000)


if __name__ == '__main__':
    unittest.main()
