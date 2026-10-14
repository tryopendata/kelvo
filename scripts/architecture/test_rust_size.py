"""Fixtures exercise growth, stale ceilings, and production/test classification."""
from pathlib import Path
import tempfile
import unittest

from rust_size import check, production_lines, read_ceilings, source_paths


class ProductionLinesTests(unittest.TestCase):
    def test_blanks_are_free_but_comments_count(self):
        self.assertEqual(production_lines("// explanation\n\nfn one() {}\n"), 2)

    def test_inline_test_module_does_not_hide_later_production(self):
        source = "fn before() {}\n#[cfg(test)]\nmod tests {\n    fn case() {}\n}\nfn after() {}\n"
        self.assertEqual(production_lines(source), 2)

    def test_attributes_and_visibility_on_test_modules(self):
        self.assertEqual(production_lines("#[cfg(test)]\n#[allow(dead_code)]\npub(crate) mod tests {\n    fn case() {}\n}\n"), 0)

    def test_external_test_module_does_not_hide_the_file(self):
        self.assertEqual(production_lines("#[cfg(test)]\nmod tests;\nfn production() {}\n"), 1)

    def test_other_cfgs_are_not_exempt(self):
        self.assertEqual(production_lines('#[cfg(target_os = "linux")]\nmod platform {\n}\n'), 3)

    def test_malformed_test_module_fails_conservatively(self):
        with self.assertRaisesRegex(ValueError, "closing brace"):
            production_lines("#[cfg(test)]\nmod tests {\n")


class SizeGateTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.relative = "crates/example/src/lib.rs"
        self.file = self.root / self.relative
        self.file.parent.mkdir(parents=True)

    def write_lines(self, count):
        self.file.write_text("// authored line\n" * count)

    def test_new_file_at_limit_passes_then_growth_fails(self):
        self.write_lines(500)
        self.assertEqual(check(self.root, {}), ([], 1))
        self.write_lines(501)
        self.assertIn("exceed 500", check(self.root, {})[0][0])

    def test_legacy_ceiling_allows_existing_size_but_not_growth(self):
        self.write_lines(550)
        self.assertEqual(check(self.root, {self.relative: 550}), ([], 1))
        self.write_lines(551)
        self.assertIn("exceed 550", check(self.root, {self.relative: 550})[0][0])

    def test_cleaned_file_requires_removing_ceiling(self):
        self.write_lines(500)
        self.assertIn("obsolete ceiling", check(self.root, {self.relative: 550})[0][0])

    def test_deleted_file_requires_removing_ceiling(self):
        self.assertIn("no authored production file", check(self.root, {self.relative: 550})[0][0])

    def test_test_only_external_files_and_vendor_are_excluded(self):
        self.file.write_text("#[cfg(test)]\n#[allow(dead_code)]\nmod checks;\nfn production() {}\n")
        (self.file.parent / "checks.rs").write_text("// test\n" * 1000)
        vendor = self.root / "crates/kelvo-collect/src/macos/vendor/ffi.rs"
        vendor.parent.mkdir(parents=True)
        vendor.write_text("// vendor\n" * 1000)
        self.assertEqual(source_paths(self.root), [self.file])
        self.assertEqual(check(self.root, {}), ([], 1))

    def test_explicit_test_module_path_is_not_production(self):
        self.file.write_text('#[cfg(test)]\n#[path = "regressions.rs"]\nmod tests;\n')
        (self.file.parent / "regressions.rs").write_text("// test\n" * 1000)
        self.assertEqual(source_paths(self.root), [self.file])

    def test_ceilings_are_plain_text_and_reject_duplicate_or_small_values(self):
        path = self.root / "ceilings.txt"
        path.write_text(f"# explanation\n{self.relative} 550 # extraction owner\n")
        self.assertEqual(read_ceilings(path), {self.relative: 550})
        path.write_text(f"{self.relative} 550\n{self.relative} 600\n")
        with self.assertRaisesRegex(ValueError, "duplicate"):
            read_ceilings(path)
        path.write_text(f"{self.relative} 500\n")
        with self.assertRaisesRegex(ValueError, "invalid"):
            read_ceilings(path)


if __name__ == "__main__":
    unittest.main()
