#!/usr/bin/env python3
"""Release metadata must fail closed rather than publish unusable updater files."""
import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location("release", Path(__file__).with_name("release-manifest.py"))
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class ReleaseTests(unittest.TestCase):
    def test_immutable_versioned_update_url_and_required_signature(self):
        data = release.manifest("0.1.0", "signature\n", "notes")
        platform = data["platforms"]["darwin-aarch64"]
        self.assertEqual(platform["url"], "https://github.com/tryopendata/kelvo/releases/download/v0.1.0/Kelvo.app.tar.gz")
        self.assertEqual(platform["signature"], "signature")
        with self.assertRaises(ValueError):
            release.manifest("0.1.0", "\n", "notes")

    def test_tag_cannot_inject_a_path_or_publish_a_prerelease_to_stable(self):
        for tag in ("main", "v1.0.0-beta.1", "v1.0.0/../../x", "v1.0"):
            with self.assertRaises(ValueError):
                release.release_version(tag)

    def test_stage_requires_all_assets_and_refuses_reuse(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            bundle = root / "bundle"
            (bundle / "dmg").mkdir(parents=True)
            (bundle / "macos").mkdir()
            (bundle / "dmg/Kelvo.dmg").write_bytes(b"dmg")
            (bundle / "macos/Kelvo.app.tar.gz").write_bytes(b"archive")
            (bundle / "macos/Kelvo.app.tar.gz.sig").write_text("signature")
            agents = root / "agents"
            agents.mkdir()
            for name in release.AGENTS.values():
                (agents / name).write_bytes(b"linux-agent")
            out = root / "out"
            release.stage("0.1.0", bundle, out, agents)
            self.assertEqual(json.loads((out / "latest.json").read_text())["version"], "0.1.0")
            catalog = json.loads((out / "release.json").read_text())
            self.assertEqual(set(catalog["artifacts"]), {"darwin-aarch64", *release.AGENTS})
            self.assertEqual(catalog["protocol"], release.protocol_range())
            with self.assertRaises(ValueError):
                release.checksums(out)
            (out / "release.json.sig").write_text("signature")
            release.checksums(out)
            self.assertEqual(len((out / "SHA256SUMS").read_text().splitlines()), 8)
            for target in release.AGENTS:
                entry = catalog["artifacts"][target]
                self.assertEqual(entry["size"], len(b"linux-agent"))
                self.assertEqual(entry["sha256"], release.hashlib.sha256(b"linux-agent").hexdigest())
            with self.assertRaises(ValueError):
                release.stage("0.1.0", bundle, out, agents)
            (bundle / "macos/Kelvo.app.tar.gz.sig").unlink()
            with self.assertRaises(ValueError):
                release.stage("0.1.0", bundle, root / "missing", agents)


if __name__ == "__main__":
    unittest.main()
