"""Synthetic log evidence must reject hidden or stale popover benchmarks."""

import importlib.util
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "bench-popover-visible.py"
SPEC = importlib.util.spec_from_file_location("popover_visible", SCRIPT)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class PopoverVisibleTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.logs = Path(self.temp.name)
        self.launch, self.start, self.end = 100.0, 110.0, 120.0

    def write(self, events, filename="kelvo.2026-01-01.log"):
        from datetime import datetime, timezone
        lines = []
        for stamp, message in events:
            date = datetime.fromtimestamp(stamp, timezone.utc).isoformat().replace("+00:00", "Z")
            lines.append(f"{date} DEBUG kelvo_lib::popover::panel: {message}\n")
        (self.logs / filename).write_text("".join(lines))

    def state(self, events):
        from datetime import datetime, timezone
        with (self.logs / "kelvo.2026-01-01.log").open("a") as log:
            for stamp, message in events:
                date = datetime.fromtimestamp(stamp, timezone.utc).isoformat().replace("+00:00", "Z")
                log.write(f"{date} INFO kelvo_lib::state: {message}\n")

    def ready(self):
        return [(101, "popover panel created"), (103, "popover occlusion visible=true"),
                (104, 'popover opened open_ms="50.0"')]

    def validate(self):
        MODULE.validate(self.logs, self.launch, self.start, self.end)

    def test_continuous_visibility_needs_no_repeated_log_during_measurement(self):
        self.write(self.ready() + [(121, "popover occlusion visible=false")])
        self.validate()

    def test_previous_launch_cannot_supply_any_required_signal(self):
        for missing in range(3):
            with self.subTest(missing=missing):
                events = self.ready()
                stamp, message = events[missing]
                events[missing] = (stamp - 20, message)
                self.write(events)
                with self.assertRaises(ValueError):
                    self.validate()

    def test_missing_or_malformed_evidence_fails_closed(self):
        for events in [[], self.ready()[:1], self.ready()[:2]]:
            self.write(events)
            with (self.logs / "kelvo.2026-01-01.log").open("a") as log:
                log.write("not-a-dateZ DEBUG kelvo_lib::popover: popover opened open_ms=1\n")
                log.write("1970-01-01T00:01:45Z DEBUG unrelated: popover opened open_ms=1\n")
            with self.assertRaises(ValueError):
                self.validate()

    def test_hidden_even_temporarily_in_measurement_invalidates_result(self):
        for stamp in [110, 115, 120]:
            self.write(self.ready() + [(stamp, "popover occlusion visible=false"),
                                      (stamp + 0.001, "popover occlusion visible=true")])
            with self.assertRaisesRegex(ValueError, "hidden or occluded"):
                self.validate()

    def test_hidden_warmup_must_recover_before_measurement(self):
        self.write(self.ready() + [(105, "popover occlusion visible=false"),
                                  (109, "popover occlusion visible=true")])
        self.validate()
        self.write(self.ready() + [(105, "popover occlusion visible=false"),
                                  (110, "popover occlusion visible=true")])
        with self.assertRaises(ValueError):
            self.validate()

    def test_recreation_and_opening_after_measurement_start_fail(self):
        self.write(self.ready() + [(115, "popover panel created")])
        with self.assertRaisesRegex(ValueError, "recreated"):
            self.validate()
        self.write([(101, "popover panel created"), (103, "popover occlusion visible=true"),
                    (115, 'popover opened open_ms="50.0"')])
        with self.assertRaises(ValueError):
            self.validate()

    def test_rotation_and_fractional_launch_boundaries(self):
        self.launch = 100.5
        self.write([(100.4, "popover panel created")], "kelvo.2025-12-31.log")
        self.write(self.ready())
        self.validate()

    def test_paused_warmup_requires_resume_before_measurement(self):
        for resumed in [False, True]:
            self.write(self.ready())
            events = [(105, "sampling paused state host=local paused=true")]
            if resumed:
                events.append((109, "sampling paused state host=local paused=false"))
            self.state(events)
            if resumed:
                self.validate()
            else:
                with self.assertRaisesRegex(ValueError, "sampling was paused"):
                    self.validate()

    def test_resuming_another_host_cannot_mask_paused_sampling(self):
        self.write(self.ready())
        self.state([(105, "sampling paused state host=local paused=true"),
                    (109, "sampling paused state host=remote paused=false")])
        with self.assertRaisesRegex(ValueError, "sampling was paused"):
            self.validate()

    def test_pause_or_resume_during_measurement_invalidates_result(self):
        for paused in ["true", "false"]:
            for stamp in [110, 115, 120]:
                self.write(self.ready())
                self.state([(stamp, f"sampling paused state host=local paused={paused}")])
                with self.assertRaisesRegex(ValueError, "pause state changed"):
                    self.validate()

    def test_settings_change_is_allowed_before_but_not_during_measurement(self):
        self.write(self.ready())
        self.state([(109, "settings changed revision=2")])
        self.validate()
        for stamp in [110, 115, 120]:
            self.write(self.ready())
            self.state([(stamp, "settings changed revision=3")])
            with self.assertRaisesRegex(ValueError, "settings changed"):
                self.validate()

    def test_command_rejects_invalid_run_instead_of_printing_valid_cpu(self):
        self.write(self.ready() + [(115, "popover occlusion visible=false")])
        result = subprocess.run([sys.executable, "-B", str(SCRIPT), str(self.logs),
                                 str(self.launch), str(self.start), str(self.end)],
                                capture_output=True, text=True, check=False)
        self.assertEqual(result.returncode, 2)
        self.assertIn("invalid popover measurement", result.stderr)


if __name__ == "__main__":
    unittest.main()
