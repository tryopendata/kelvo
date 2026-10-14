"""Exercise hook decisions, never execute submitted commands or send signals."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class HookTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="kelvo-hook-tests-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.hooks = self.root / ".claude/hooks"
        shutil.copytree(ROOT / ".claude/hooks", self.hooks)
        subprocess.run(["git", "init", "-q", str(self.root)], check=True)
        self.env = dict(os.environ, CLAUDE_CODE_SESSION_ID="hook-test")
        self.env["TMPDIR"] = str(self.root)
        self.marker = Path("/tmp/.wt-anchored-" + str(self.root).replace("/", "-") + "-hook-test")
        self.addCleanup(self.marker.unlink, missing_ok=True)

    def hook(self, name, payload=None, **env):
        return subprocess.run(
            ["bash", str(self.hooks / name)], input=json.dumps(payload or {}),
            text=True, capture_output=True, cwd=self.root,
            env=dict(self.env, **env), check=True,
        )

    def decision(self, hook, command):
        result = self.hook(hook, {"tool_name": "Bash", "tool_input": {"command": command}})
        return json.loads(result.stdout or "{}").get("hookSpecificOutput", {})

    def test_destructive_git_options_are_denied_and_never_approved(self):
        for command in [
            "git -C /tmp/example reset --hard",
            "git -c core.foo=bar clean -fd",
            "git --git-dir=/tmp/example push -f",
            "git --work-tree='/tmp/with space' --no-pager checkout .",
            "git -P -C '/tmp/with space' restore .",
            "env RUST_LOG=debug timeout 120 git -C /tmp/example stash clear",
            "bash -c 'git -C /tmp/example reset --hard'",
            "git -C/tmp/example -ccore.foo=bar reset --hard",
            "xargs -0 -I '{}' git --no-pager --git-dir=/tmp/example clean -fd",
        ]:
            with self.subTest(command=command):
                self.assertEqual(self.decision("pre-bash.sh", command).get("permissionDecision"), "deny")
                self.assertNotEqual(self.decision("auto-approve.sh", command).get("permissionDecision"), "allow")

    def test_development_commands_and_wrappers_stay_autonomous(self):
        commands = [
            "git status", "git -C '/tmp/with space' diff", "git push --force-with-lease",
            "bun install", "cargo test", "make check", "node -e 'console.log(1)'",
            "bash -c 'bun run test'", "sh -c 'bun run test'", "python3 -c 'print(1)'",
            "env RUST_LOG=debug cargo test", "env -u NODE_OPTIONS bun run test",
            "env --chdir='/tmp/with space' bun run test", "timeout -k 5 120 bun run test",
            "time -p cargo check", "nohup bun run test", "command -- git status",
            "xargs -0 -n 1 -P 2 git status", "xargs -I '{}' bun run test '{}'",
            "env A='two words' timeout --signal=TERM 120 command bun run test",
            "env -S 'bun run test'", "env --split-string='bun run test'",
            "xargs --max-args=1 --max-procs 2 bun run test",
            "timeout 30 rm -f /tmp/kelvo-build.json", "env", "env A=1", "time", "xargs -0",
        ]
        for command in commands:
            with self.subTest(command=command):
                self.assertEqual(self.decision("auto-approve.sh", command).get("permissionDecision"), "allow")

    def test_wrappers_classify_inner_command(self):
        result = self.decision("auto-approve.sh", "env python3 -c 'print(1)'")
        self.assertEqual(result.get("permissionDecision"), "allow")
        self.assertNotIn("Read-only", result.get("permissionDecisionReason", ""))
        for command in ["env unknown-tool", "timeout 3 unknown-tool", "command unknown-tool", "xargs unknown-tool"]:
            with self.subTest(command=command):
                self.assertNotEqual(self.decision("auto-approve.sh", command).get("permissionDecision"), "allow")

    def test_existing_denials_and_inert_rm_arguments(self):
        for command in ["rm -rf /", "env A=1 rm -rf ~", "timeout 2 git reset --hard", "git clean -fd", "bun publish"]:
            with self.subTest(command=command):
                self.assertEqual(self.decision("pre-bash.sh", command).get("permissionDecision"), "deny")
        for command in ["grep 'rm -rf ~' README.md", "git log --grep='rm -rf /'", "find scripts -name '*.pyc' -delete"]:
            with self.subTest(command=command):
                self.assertEqual(self.decision("auto-approve.sh", command).get("permissionDecision"), "allow")

    def test_reaping_is_checkout_scoped_and_leaves_active_or_intentional_work(self):
        sibling = self.root / "sibling"
        fixture = self.root / "ps.txt"
        fixture.write_text(f"""91001 1 05:00 node {self.root}/node_modules/vitest/vitest.mjs
91002 1 05:00 node {sibling}/node_modules/vitest/vitest.mjs
91003 1 00:30 node {self.root}/node_modules/vitest/vitest.mjs
91004 1 05:00 node {self.root}/node_modules/vite/bin/vite.js dev
91005 91006 05:00 node {self.root}/node_modules/vitest/vitest.mjs
91006 1 05:00 tmux
91007 1 05:00 node {self.root}-other/node_modules/vitest/vitest.mjs
91008 1 05:00 bun {self.root}/scripts/check.js
91009 91008 05:00 node {self.root}/node_modules/vitest/vitest.mjs
91010 1 05:00 bun {self.root}/scripts/check.js
91011 91010 05:00 node {self.root}/node_modules/vitest/vitest.mjs
91012 91010 05:00 node {sibling}/node_modules/vitest/vitest.mjs
""")
        result = self.hook("reap-orphans.sh", REAP_ORPHANS_PS_FILE=str(fixture),
                           REAP_ORPHANS_ROOTS=f"{self.root}\n{sibling}",
                           REAP_ORPHANS_SELF="92001", REAP_ORPHANS_DRY_RUN="1")
        self.assertIn("would reap", result.stderr)
        self.assertIn("91001", result.stderr)
        self.assertIn("left sibling", result.stderr)
        self.assertIn("91002", result.stderr)
        self.assertIn("91008", result.stderr)
        self.assertIn("left sibling-worktree orphan alone: 91010", result.stderr)
        own_line = next(line for line in result.stderr.splitlines() if "would reap" in line)
        self.assertNotIn("91002", own_line)
        self.assertNotIn("91010", own_line)
        for pid in ["91003", "91004", "91005", "91006", "91007"]:
            self.assertNotIn(pid, result.stderr)

    def test_reaping_checks_each_checkout_occurrence_in_one_command(self):
        parent = self.root.parent
        nested = self.root / "nested-worktree"
        tool = f"{self.root}/node_modules/vitest/vitest.mjs"
        fixture = self.root / "mixed-paths-ps.txt"
        fixture.write_text(f"""91101 1 05:00 node {tool}
91102 1 05:00 node {tool} --config {parent}/sibling-config.ts
91103 1 05:00 node {parent}/node_modules/vitest/vitest.mjs --config {self.root}/config.ts
91104 1 05:00 node {nested}/node_modules/vitest/vitest.mjs
91105 1 05:00 node {tool} --config {nested}/config.ts
""")
        result = self.hook("reap-orphans.sh", REAP_ORPHANS_PS_FILE=str(fixture),
                           REAP_ORPHANS_ROOTS=f"{parent}\n{self.root}\n{nested}",
                           REAP_ORPHANS_SELF="92001", REAP_ORPHANS_DRY_RUN="1")
        own_line = next(line for line in result.stderr.splitlines() if "would reap" in line)
        self.assertIn("91101", own_line, "an enclosing checkout prefix is not a separate path")
        for pid in ["91102", "91103", "91104", "91105"]:
            with self.subTest(pid=pid):
                self.assertIn(f"left sibling-worktree orphan alone: {pid}", result.stderr)
                self.assertNotIn(pid, own_line)

    def test_stop_formats_only_files_written_by_that_session(self):
        source = self.root / "src"
        source.mkdir()
        for name in ["mine.ts", "other.ts", "preexisting.ts"]:
            (source / name).write_text("export const value = 1;\n")
        subprocess.run(["git", "add", "src/preexisting.ts"], cwd=self.root, check=True)
        (source / "preexisting.ts").write_text("export const value=2;\n")
        binary = self.root / "bin"
        binary.mkdir()
        log = self.root / "formatter.log"
        stub = binary / "bunx"
        stub.write_text('#!/bin/bash\nprintf "%s\\n" "$@" >> "$FORMATTER_LOG"\n')
        stub.chmod(0o755)
        self.env.update(PATH=f"{binary}:{self.env['PATH']}", FORMATTER_LOG=str(log))
        for session, name in [("one", "mine.ts"), ("two", "other.ts")]:
            self.hook("lint-on-write.sh", {"tool_name": "Write", "session_id": session,
                      "tool_input": {"file_path": str(source / name)}})
        log.write_text("")
        self.hook("format-changed.sh", {"session_id": "one"})
        output = log.read_text()
        self.assertIn("mine.ts", output)
        self.assertNotIn("other.ts", output)
        self.assertNotIn("preexisting.ts", output)
        log.write_text("")
        self.hook("format-changed.sh", {})
        self.assertEqual(log.read_text(), "")


if __name__ == "__main__":
    unittest.main()
