"""Run the gate wrapper with real Git and Make in disposable checkouts."""

import os
from pathlib import Path
import subprocess
import tempfile
import time
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "pre-push-check.sh"


class PrePushTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="kelvo verification ")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.env = {k: v for k, v in os.environ.items() if not k.startswith("GIT_")}
        subprocess.run(["git", "init", "-q"], cwd=self.root, env=self.env, check=True)
        (self.root / ".gitignore").write_text("target/\n")
        self.write_makefile()
        subprocess.run(["git", "add", "."], cwd=self.root, env=self.env, check=True)
        subprocess.run(
            ["git", "-c", "core.hooksPath=/dev/null", "-c", "user.name=Test",
             "-c", "user.email=test@example.com", "commit", "-qm", "fixture"],
            cwd=self.root, env=self.env, check=True,
        )

    def write_makefile(self, fail=False):
        (self.root / "Makefile").write_text(
            "check:\n\t@echo checked\n\t@echo diagnostic >&2\n"
            "bindings-check:\n\t@echo bindings\n"
            + ("\t@exit 17\n" if fail else "")
            + "e2e-perf:\n\t@echo performance\n"
        )

    def run_gate(self):
        nested = self.root / "nested"
        nested.mkdir(exist_ok=True)
        result = subprocess.run(
            ["bash", str(SCRIPT)], cwd=nested, env=self.env,
            capture_output=True, text=True, timeout=20,
        )
        runs = list((self.root / "target/qa/pre-push").iterdir())
        self.assertEqual(len(runs), 1)
        return result, runs[0]

    def test_success_retains_each_stage_output_and_revision(self):
        result, evidence = self.run_gate()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("checked", (evidence / "check.log").read_text())
        self.assertIn("diagnostic", (evidence / "check.log").read_text())
        self.assertIn("performance", (evidence / "e2e-perf.log").read_text())
        revision = subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=self.root, env=self.env, text=True,
        )
        self.assertEqual((evidence / "revision.txt").read_text(), revision)
        self.assertEqual((evidence / "working-tree.txt").read_text(), "")
        self.assertEqual((evidence / "result.txt").read_text(), "0\n")
        self.assertEqual((evidence / "stages.tsv").read_text(),
                         "check\t0\nbindings-check\t0\ne2e-perf\t0\n")
        self.assertIn(str(evidence), result.stdout)

    def test_failure_survives_tee_and_stops_later_stages(self):
        self.write_makefile(fail=True)
        result, evidence = self.run_gate()
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertIn("Error 17", (evidence / "bindings-check.log").read_text())
        self.assertFalse((evidence / "e2e-perf.log").exists())
        self.assertEqual((evidence / "result.txt").read_text(), "2\n")
        self.assertEqual((evidence / "stages.tsv").read_text(),
                         "check\t0\nbindings-check\t2\n")
        self.assertIn("Makefile", (evidence / "working-tree.txt").read_text())

    def test_output_is_inspectable_before_the_gate_finishes(self):
        with (self.root / "Makefile").open("a") as makefile:
            makefile.write("\ncheck: wait-for-test\nwait-for-test:\n"
                           "\t@echo waiting\n"
                           "\t@while [ ! -f release ]; do sleep 0.01; done\n")
        process = subprocess.Popen(
            ["bash", str(SCRIPT)], cwd=self.root, env=self.env,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
        )
        try:
            deadline = time.monotonic() + 10
            while time.monotonic() < deadline:
                logs = list(self.root.glob("target/qa/pre-push/*/check.log"))
                if logs and "waiting" in logs[0].read_text():
                    break
                time.sleep(0.01)
            else:
                self.fail("stage output was not visible while Make was running")
            evidence = logs[0].parent
            self.assertIsNone(process.poll())
            self.assertEqual((evidence / "current-stage.txt").read_text(), "check\n")
            self.assertFalse((evidence / "result.txt").exists())
        finally:
            (self.root / "release").touch()
            process.communicate(timeout=10)
        self.assertEqual(process.returncode, 0)


if __name__ == "__main__":
    unittest.main()
