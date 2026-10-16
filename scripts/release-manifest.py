#!/usr/bin/env python3
"""Validate release inputs and stage the exact assets consumed by downloads/updater."""
import argparse
import hashlib
import json
import re
import shutil
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def release_version(tag):
    if not re.fullmatch(r"v\d+\.\d+\.\d+", tag):
        raise ValueError("Release tag must be a stable vMAJOR.MINOR.PATCH version")
    version = tag[1:]
    for file in ("package.json", "src-tauri/tauri.conf.json"):
        if json.loads((ROOT / file).read_text())["version"] != version:
            raise ValueError(f"{file} does not match {tag}")
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--no-deps", "--format-version=1"], cwd=ROOT
    ))
    if next(p for p in metadata["packages"] if p["name"] == "kelvo")["version"] != version:
        raise ValueError(f"Cargo workspace version does not match {tag}")
    return version


def manifest(version, signature, notes):
    if not signature.strip():
        raise ValueError("Updater signature is empty")
    return {
        "version": version,
        "notes": notes,
        "platforms": {"darwin-aarch64": {
            "signature": signature.strip(),
            "url": f"https://github.com/tryopendata/kelvo/releases/download/v{version}/Kelvo.app.tar.gz",
        }},
    }


def stage(version, bundle, output):
    # Never mix an earlier release's files with this one.
    output.mkdir(parents=True, exist_ok=True)
    if any(output.iterdir()):
        raise ValueError(f"Release output must be empty: {output}")
    dmgs = list((bundle / "dmg").glob("*.dmg"))
    if len(dmgs) != 1:
        raise ValueError("Expected exactly one DMG")
    assets = {
        "Kelvo_aarch64.dmg": dmgs[0],
        "Kelvo.app.tar.gz": bundle / "macos/Kelvo.app.tar.gz",
        "Kelvo.app.tar.gz.sig": bundle / "macos/Kelvo.app.tar.gz.sig",
    }
    for name, source in assets.items():
        if not source.is_file() or source.stat().st_size == 0:
            raise ValueError(f"Missing or empty release asset: {source}")
        shutil.copy2(source, output / name)
    notes = (ROOT / "RELEASE_NOTES.md").read_text()
    data = manifest(version, (output / "Kelvo.app.tar.gz.sig").read_text(), notes)
    (output / "latest.json").write_text(json.dumps(data, indent=2) + "\n")
    checksums = []
    for path in sorted(output.iterdir()):
        checksums.append(f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.name}\n")
    (output / "SHA256SUMS").write_text("".join(checksums))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("tag")
    parser.add_argument("--stage", action="store_true")
    parser.add_argument("--bundle", type=Path, default=ROOT / "target/release/bundle")
    parser.add_argument("--output", type=Path, default=ROOT / "target/release-assets")
    args = parser.parse_args()
    version = release_version(args.tag)
    if args.stage:
        stage(version, args.bundle, args.output)
    print(f"Validated release v{version}")
