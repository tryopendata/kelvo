"""Exercise generation against disposable checkouts; never edit live sources."""

import importlib.util
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

SCRIPT_DIR = Path(__file__).resolve().parent
REPO = SCRIPT_DIR.parents[1]
spec = importlib.util.spec_from_file_location("agent_config_sync", SCRIPT_DIR / "sync.py")
sync = importlib.util.module_from_spec(spec)
spec.loader.exec_module(sync)


class SyncTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / "checkout"
        self.root.mkdir()
        shutil.copy2(REPO / "CLAUDE.md", self.root / "CLAUDE.md")
        for kind in ("rules", "skills", "commands"):
            shutil.copytree(REPO / ".claude" / kind, self.root / ".claude" / kind)
        (self.root / "scripts/agent-config").mkdir(parents=True)
        for name in ("sync.py", "shared.txt"):
            shutil.copy2(SCRIPT_DIR / name, self.root / "scripts/agent-config" / name)

    def generate(self):
        sync.apply(sync.plan_changes(self.root, sync.desired_state(self.root)))

    def run_cli(self, *args, cwd=None, root=None):
        root = root or self.root
        return subprocess.run(
            [sys.executable, "-B", str(root / "scripts/agent-config/sync.py"), *args],
            cwd=cwd or root, capture_output=True, text=True,
            env={**os.environ, "GIT_DIR": "/irrelevant/pre-commit/git-dir"},
        )

    def snapshot(self):
        return {str(p.relative_to(self.root)): (
            os.readlink(p) if p.is_symlink() else p.read_bytes(), p.lstat().st_mtime_ns
        ) for p in self.root.rglob("*") if p.is_symlink() or p.is_file()}

    def test_complete_guidance_and_relative_links(self):
        self.assert_complete_guidance()

    def assert_complete_guidance(self):
        self.generate()
        agents = (self.root / "AGENTS.md").read_text()
        self.assertIn((self.root / "CLAUDE.md").read_text().rstrip(), agents)
        manifest = sync.read_manifest(self.root / "scripts/agent-config/shared.txt")
        rules = [self.root / ".claude/rules" / rel for rel in manifest["rule"]]
        self.assertEqual(
            {p.name for p in (self.root / ".agents/rules").glob("*.md")},
            {rel.replace("/", "-") for rel in manifest["rule"]},
        )
        for rule in rules:
            self.assertIn(f"]({rule.relative_to(self.root).as_posix()})", agents)
        commands = list((self.root / ".claude/commands").glob("*.md"))
        self.assertTrue(commands)
        for command in commands:
            self.assertIn(f"]({command.relative_to(self.root).as_posix()})", agents)
        link = self.root / ".agents/skills/frontend-design-slop"
        self.assertTrue(link.is_symlink())
        self.assertEqual(os.readlink(link), "../../.claude/skills/frontend-design-slop")
        self.assertEqual((link / "SKILL.md").read_bytes(),
                         (self.root / ".claude/skills/frontend-design-slop/SKILL.md").read_bytes())

    def test_generation_is_idempotent_and_preserves_sources(self):
        before = self.snapshot()
        self.generate()
        for path, value in before.items():
            self.assertEqual(self.snapshot()[path], value, path)
        generated = self.snapshot()
        self.generate()
        self.assertEqual(self.snapshot(), generated)
        self.assertEqual(self.run_cli("--check").returncode, 0)

    def test_new_guidance_keeps_completeness_check_valid(self):
        (self.root / ".claude/rules/new.md").write_text("# New shared rule\n")
        manifest = self.root / "scripts/agent-config/shared.txt"
        manifest.write_text(manifest.read_text() + "rule:new.md\n")
        (self.root / ".claude/commands/kelvo:new.md").write_text(
            "---\ndescription: New workflow\n---\n\nFollow the new workflow.\n")
        # An unshared source should not need a generated rule or routing row.
        (self.root / ".claude/rules/private.md").write_text("# Unshared rule\n")
        self.assert_complete_guidance()
        self.assert_rule_activation()
        self.assertEqual(self.run_cli("--check").returncode, 0)

    def test_missing_outputs_check_is_read_only(self):
        before = self.snapshot()
        result = self.run_cli("--check")
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("make agent-config", result.stderr)
        self.assertEqual(self.snapshot(), before)

    def test_source_edits_are_detected_and_propagated(self):
        self.generate()
        cases = [
            ("CLAUDE.md", "\nNew canonical instruction.\n", "AGENTS.md"),
            (".claude/rules/rust.md", "\nNew Rust rule.\n", ".agents/rules/rust.md"),
        ]
        for source, addition, output in cases:
            with self.subTest(source=source):
                path = self.root / source
                path.write_text(path.read_text() + addition)
                before = self.snapshot()
                self.assertEqual(self.run_cli("--check").returncode, 1)
                self.assertEqual(self.snapshot(), before)
                self.generate()
                self.assertIn(addition, (self.root / output).read_text())
        command = self.root / ".claude/commands/kelvo:test.md"
        command.write_text(command.read_text().replace("Run tests (scoped by default)", "Updated test workflow"))
        self.assertEqual(self.run_cli("--check").returncode, 1)
        self.generate()
        self.assertIn("Updated test workflow", (self.root / "AGENTS.md").read_text())

    def test_rule_activation_and_body_are_preserved(self):
        self.assert_rule_activation()
        self.assertEqual(sync.antigravity_globs(["src/**/*.{ts,tsx}"]),
                         ["**/src/**/*.ts", "**/src/**/*.tsx"])

    def assert_rule_activation(self):
        self.generate()
        manifest = sync.read_manifest(self.root / "scripts/agent-config/shared.txt")
        for rel in manifest["rule"]:
            source = self.root / ".claude/rules" / rel
            original, body = sync.read_source(source)
            generated = self.root / ".agents/rules" / rel.replace("/", "-")
            metadata, generated_body = sync.read_source(generated)
            self.assertTrue(generated_body.endswith(body))
            paths = sync.rule_paths(original)
            self.assertEqual(metadata["trigger"], "glob" if paths else "always_on")
            if paths:
                self.assertEqual(metadata["globs"], ",".join(sync.antigravity_globs(paths)))
            else:
                self.assertNotIn("globs", metadata)

    def test_always_on_rule_has_no_globs(self):
        (self.root / ".claude/rules/rust.md").write_text("# Rust\n\nApplies everywhere.\n")
        self.generate()
        metadata, _ = sync.read_source(self.root / ".agents/rules/rust.md")
        self.assertEqual(metadata["trigger"], "always_on")
        self.assertNotIn("globs", metadata)
        self.assertIn("| All tasks | [rust.md]", (self.root / "AGENTS.md").read_text())

    def test_repair_broken_managed_skill_link(self):
        self.generate()
        link = self.root / ".agents/skills/frontend-design-slop"
        link.unlink()
        link.symlink_to("../../.claude/skills/missing")
        before = self.snapshot()
        self.assertEqual(self.run_cli("--check").returncode, 1)
        self.assertEqual(self.snapshot(), before)
        self.generate()
        self.assertTrue((link / "SKILL.md").is_file())

    def test_skill_source_edits_flow_through_link(self):
        self.generate()
        source = self.root / ".claude/skills/frontend-design-slop/SKILL.md"
        source.write_text(source.read_text() + "\nNew design instruction.\n")
        self.assertIn("New design instruction.",
                      (self.root / ".agents/skills/frontend-design-slop/SKILL.md").read_text())
        self.assertEqual(self.run_cli("--check").returncode, 0)

    def test_missing_skill_source_fails_without_writes(self):
        self.generate()
        (self.root / ".claude/skills/frontend-design-slop/SKILL.md").unlink()
        before = self.snapshot()
        self.assertEqual(self.run_cli("--check").returncode, 2)
        self.assertEqual(self.snapshot(), before)

    def test_cleanup_only_removes_managed_outputs(self):
        self.generate()
        manual = self.root / ".agents/rules/manual.md"
        manual.write_text("# Leave this alone\n")
        manual_dir = self.root / ".agents/skills/manual"
        manual_dir.mkdir()
        (manual_dir / "SKILL.md").write_text("Hand-written skill.\n")
        manifest = self.root / "scripts/agent-config/shared.txt"
        manifest.write_text(manifest.read_text().replace("rule:rust.md\n", "").replace(
            "skill:frontend-design-slop\n", ""))
        self.assertEqual(self.run_cli("--check").returncode, 1)
        self.generate()
        self.assertFalse((self.root / ".agents/rules/rust.md").exists())
        self.assertFalse((self.root / ".agents/skills/frontend-design-slop").is_symlink())
        self.assertEqual(manual.read_text(), "# Leave this alone\n")
        self.assertTrue((manual_dir / "SKILL.md").is_file())

    def test_unmanaged_collision_prevents_all_writes(self):
        (self.root / "AGENTS.md").write_text("# My instructions\n")
        before = self.snapshot()
        result = self.run_cli()
        self.assertEqual(result.returncode, 2)
        self.assertIn("unmanaged", result.stderr)
        self.assertEqual(self.snapshot(), before)

    def test_flattened_rule_collision_prevents_all_writes(self):
        (self.root / ".claude/rules/frontend-react.md").write_text("# Duplicate\n")
        manifest = self.root / "scripts/agent-config/shared.txt"
        manifest.write_text(manifest.read_text() + "rule:frontend-react.md\n")
        before = self.snapshot()
        result = self.run_cli()
        self.assertEqual(result.returncode, 2)
        self.assertIn("collision", result.stderr)
        self.assertEqual(self.snapshot(), before)

    def test_invalid_frontmatter_fails_without_writes(self):
        rule = self.root / ".claude/rules/rust.md"
        for text in ("---\npaths:\n  - x\n", '---\npaths: "unclosed\n---\nBody\n',
                     "---\npaths: []\n---\nBody\n", "---\npaths: [1]\n---\nBody\n",
                     "---\npaths:\n  nested: x\n---\nBody\n"):
            with self.subTest(text=text):
                rule.write_text(text)
                before = self.snapshot()
                self.assertEqual(self.run_cli().returncode, 2)
                self.assertEqual(self.snapshot(), before)

    def test_manifest_rejects_duplicates_and_path_escape(self):
        path = self.root / "scripts/agent-config/shared.txt"
        for text in ("rule:../outside.md\n", "skill:/absolute\n", "skill:a\nskill:a\n"):
            with self.subTest(text=text):
                path.write_text(text)
                with self.assertRaises(sync.ConfigError):
                    sync.read_manifest(path)

    def test_symlinked_output_directory_is_refused(self):
        outside = Path(self.temp.name) / "outside"
        outside.mkdir()
        (self.root / ".agents").symlink_to(outside, target_is_directory=True)
        result = self.run_cli()
        self.assertEqual(result.returncode, 2)
        self.assertIn("symlink", result.stderr)
        self.assertEqual(list(outside.iterdir()), [])
        self.assertFalse((self.root / "AGENTS.md").exists())

    def test_portability_and_nested_discovery_smoke(self):
        self.generate()
        moved = Path(self.temp.name) / "linked-worktree"
        self.root.rename(moved)
        (moved / ".git").write_text("gitdir: /some/main/checkout/.git/worktrees/linked\n")
        nested = moved / "src/app/routes"
        nested.mkdir(parents=True)
        for cwd in (moved, nested, Path(self.temp.name)):
            with self.subTest(cwd=cwd):
                result = self.run_cli("--check", cwd=cwd, root=moved)
                self.assertEqual(result.returncode, 0, result.stderr)
        # Discovery from a nested session reaches the same root instruction and skill.
        ancestors = [nested, *list(nested.parents)[:3]]
        self.assertEqual([p / "AGENTS.md" for p in ancestors if (p / "AGENTS.md").is_file()],
                         [moved / "AGENTS.md"])
        self.assertTrue((moved / ".agents/skills/frontend-design-slop/SKILL.md").is_file())


if __name__ == "__main__":
    unittest.main()
