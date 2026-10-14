"""Probe the real Biome configuration with isolated authored-file fixtures.

Real files are important: Biome's stdin mode does not apply all path overrides in
this release. Each fixture has a unique directory, removed even after failure.
"""
import json
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
BIOME = ROOT / "node_modules/.bin/biome"
IMPORT_RULE = "style/noRestrictedImports"
FUNCTION_RULE = "complexity/noExcessiveLinesPerFunction"
FILE_RULE = "style/noExcessiveLinesPerFile"
COMPLEXITY_RULE = "complexity/noExcessiveCognitiveComplexity"


class BiomeRules(unittest.TestCase):
    def lint(self, source, *, directory="src/core", name="probe.ts", rule=IMPORT_RULE):
        self.assertTrue(BIOME.is_file(), "Run bun install before architecture fixtures")
        with tempfile.TemporaryDirectory(prefix="architecture-fixture-", dir=ROOT / directory) as temp:
            path = Path(temp) / name
            path.write_text(source)
            result = subprocess.run(
                [str(BIOME), "lint", f"--only={rule}", "--reporter=json", str(path)],
                cwd=ROOT, capture_output=True, text=True, check=False,
            )
            output = json.loads(result.stdout)
            self.assertEqual(output["summary"]["changed"] + output["summary"]["unchanged"], 1)
            diagnostics = output["diagnostics"]
            unexpected = [d for d in diagnostics if d["category"] != f"lint/{rule}"]
            self.assertEqual(unexpected, [], result.stdout + result.stderr)
            return result.returncode, diagnostics

    def assert_rejected(self, source, **kwargs):
        code, diagnostics = self.lint(source, **kwargs)
        self.assertNotEqual(code, 0)
        self.assertGreater(len(diagnostics), 0)

    def assert_allowed(self, source, **kwargs):
        code, diagnostics = self.lint(source, **kwargs)
        self.assertEqual((code, diagnostics), (0, []))

    def test_core_rejects_react_alias_relative_and_react_zustand(self):
        for module in ("react", "~/app", "../app/app", "../../app/app", "zustand"):
            with self.subTest(module=module):
                self.assert_rejected(f'import {{ value }} from "{module}";\n')
        self.assert_allowed('import { createStore } from "zustand/vanilla";\n')

    def test_widgets_reject_transport_generated_commands_and_app_hooks(self):
        for module, symbol in (("@core/transport", "Transport"), ("@core/generated/bindings", "commands"),
                               ("@core/generated/bindings", "events"), ("~/hooks/use-transport", "useTransport"),
                               ("../hooks/use-remote-hosts", "useRemoteHosts"),
                               ("../routes/dashboard/containers/_hooks/use-docker", "useDocker"),
                               ("../../core/transport", "Transport"), ("../../core/generated/bindings", "commands"),
                               ("../router", "router"), ("@core/app-transport", "appTransport"),
                               ("@core/mock-transport", "createMockTransport"), ("@core/mock/fixtures", "MOCK_HOST_ID")):
            with self.subTest(module=module, symbol=symbol):
                self.assert_rejected(f'import {{ {symbol} }} from "{module}";\n', directory="src/app/widgets")

    def test_app_reaches_tauri_only_through_transport(self):
        for module, symbol in (("@core/transport/native", "createNativeTransport"),
                               ("@core/transport/native-docker", "dockerCommands"),
                               ("@core/generated/bindings", "commands"), ("@core/generated/bindings", "events")):
            with self.subTest(module=module, symbol=symbol):
                self.assert_rejected(f'import {{ {symbol} }} from "{module}";\n', directory="src/app")
        for source in ('import type { Snapshot } from "@core/generated/bindings";\n',
                       'import { useTransport } from "@core/transport";\n'):
            with self.subTest(source=source):
                self.assert_allowed(source, directory="src/app")

    def test_widgets_allow_generated_types_constants_and_presentation_helpers(self):
        for source in ('import type { Snapshot } from "@core/generated/bindings";\n',
                       'import { HISTORY_PAGE_LIMIT } from "@core/generated/bindings";\n',
                       'import { cn } from "~/lib/utils";\n',
                       'import { enter } from "~/lib/motion/variants";\n',
                       'import type { LegendProps } from "../legend";\n'):
            with self.subTest(source=source):
                self.assert_allowed(source, directory="src/app/widgets")

    def test_browser_rejects_node_builtins_in_prefixed_and_bare_forms(self):
        for directory in ("src/core", "src/app", "src/app/widgets"):
            for module in ("node:fs", "fs", "fs/promises"):
                with self.subTest(directory=directory, module=module):
                    self.assert_rejected(f'import {{ readFile }} from "{module}";\n', directory=directory)

    def test_tests_keep_node_runtime_access(self):
        for directory in ("src/core", "src/app"):
            self.assert_allowed('import { readFile } from "node:fs";\n', directory=directory, name="probe.test.ts")

    @staticmethod
    def long_function(lines):
        return "export function accumulate(value: number) {\n" + "  value += 1;\n" * lines + "  return value;\n}\n"

    def test_function_limit_and_tsx_test_profile(self):
        self.assert_allowed(self.long_function(20), rule=FUNCTION_RULE)
        self.assert_rejected(self.long_function(110), rule=FUNCTION_RULE)
        for name in ("probe.tsx", "probe.test.ts"):
            self.assert_allowed(self.long_function(150), name=name, rule=FUNCTION_RULE)
            self.assert_rejected(self.long_function(210), name=name, rule=FUNCTION_RULE)

    def test_file_limit_and_test_gallery_profile(self):
        source = "\n".join(f"export const value{i} = {i};" for i in range(750))
        self.assert_rejected(source, rule=FILE_RULE)
        self.assert_allowed(source, name="probe.test.ts", rule=FILE_RULE)
        self.assert_allowed(source, directory="src/app/routes/dev/gallery", rule=FILE_RULE)
        self.assert_rejected(source + "\n" + "\n".join(f"export const extra{i} = {i};" for i in range(300)), name="probe.test.ts", rule=FILE_RULE)

    def test_cognitive_limit_is_independent_of_body_size(self):
        self.assert_allowed("export function choose(value: boolean) { return value ? 1 : 0; }", rule=COMPLEXITY_RULE)
        source = "export function choose(value: boolean) {\n" + "if (value) {\n" * 8 + "return 1;\n" + "}\n" * 8 + "return 0;\n}\n"
        self.assert_rejected(source, rule=COMPLEXITY_RULE)


if __name__ == "__main__":
    unittest.main()
