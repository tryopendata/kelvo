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
AGENTS = {
    "linux-aarch64": "kelvo-agent-aarch64-unknown-linux-musl",
    "linux-x86_64": "kelvo-agent-x86_64-unknown-linux-musl",
}


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


def protocol_range():
    source = (ROOT / "crates/kelvo-proto/src/lib.rs").read_text()
    def constant(name):
        match = re.search(rf"pub const {name}: u16 = (\d+);", source)
        if not match:
            raise ValueError(f"Cannot read protocol constant {name}")
        return int(match[1])
    return {"min": constant("MIN_COMPATIBLE"), "max": constant("PROTO_VERSION")}


def catalog(version, output, notes, protocol):
    artifacts = {}
    for target, name in {"darwin-aarch64": "Kelvo.app.tar.gz", **AGENTS}.items():
        path = output / name
        entry = {
            "url": f"https://github.com/tryopendata/kelvo/releases/download/v{version}/{name}",
            "size": path.stat().st_size,
            "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
        }
        if target == "darwin-aarch64":
            entry["signature"] = (output / "Kelvo.app.tar.gz.sig").read_text().strip()
        artifacts[target] = entry
    return {"schema_version": 1, "version": version, "notes": notes,
            "protocol": protocol, "artifacts": artifacts}


def checksums(output):
    # Run after signing, so the checksum list covers the detached signature too.
    required = ["release.json", "release.json.sig", "latest.json", "Kelvo.app.tar.gz",
                "Kelvo.app.tar.gz.sig", "Kelvo_aarch64.dmg", *AGENTS.values()]
    for name in required:
        if not (output / name).is_file() or (output / name).stat().st_size == 0:
            raise ValueError(f"Missing or empty release asset: {name}")
    lines = [f"{hashlib.sha256((output / name).read_bytes()).hexdigest()}  {name}\n"
             for name in sorted(required)]
    (output / "SHA256SUMS").write_text("".join(lines))


def stage(version, bundle, output, agents=None):
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
    agents = agents or ROOT / "src-tauri/binaries/agent"
    assets.update({name: agents / name for name in AGENTS.values()})
    for name, source in assets.items():
        if not source.is_file() or source.stat().st_size == 0:
            raise ValueError(f"Missing or empty release asset: {source}")
        shutil.copy2(source, output / name)
    notes = (ROOT / "RELEASE_NOTES.md").read_text()
    data = manifest(version, (output / "Kelvo.app.tar.gz.sig").read_text(), notes)
    (output / "latest.json").write_text(json.dumps(data, indent=2) + "\n")
    data = catalog(version, output, notes, protocol_range())
    (output / "release.json").write_text(json.dumps(data, indent=2) + "\n")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("tag")
    parser.add_argument("--stage", action="store_true")
    parser.add_argument("--checksums", action="store_true")
    parser.add_argument("--bundle", type=Path, default=ROOT / "target/release/bundle")
    parser.add_argument("--output", type=Path, default=ROOT / "target/release-assets")
    args = parser.parse_args()
    version = release_version(args.tag)
    if args.stage:
        stage(version, args.bundle, args.output)
    if args.checksums:
        checksums(args.output)
    print(f"Validated release v{version}")
