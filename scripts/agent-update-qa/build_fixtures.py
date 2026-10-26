#!/usr/bin/env python3
"""Build disposable signing-key fixtures with test-only candidate failure behaviors.

Only the copied source is modified. No fault hooks or test key enter production.
"""
import argparse
from pathlib import Path
import re
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[2]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--target", choices=["aarch64-unknown-linux-musl", "x86_64-unknown-linux-musl"], required=True)
    args = parser.parse_args()
    qa = args.directory.resolve()
    assert not qa.is_relative_to(ROOT) and not ROOT.is_relative_to(qa)
    qa.mkdir(parents=True, exist_ok=True)
    source = qa / "source"
    source.mkdir(exist_ok=True)
    subprocess.run(["rsync", "-a", "--exclude=.git", "--exclude=target", "--exclude=node_modules", "--exclude=dist", "--exclude=src-tauri/binaries/agent", str(ROOT) + "/", str(source) + "/"], check=True)
    key = qa / "test-key"
    if not key.exists():
        subprocess.run(["bun", "tauri", "signer", "generate", "--ci", "--password", "", "--write-keys", str(key)], cwd=ROOT, check=True, stdout=subprocess.DEVNULL)
    shutil.copyfile(str(key) + ".pub", source / "crates/kelvo-update/public-key.txt")
    daemon = source / "crates/kelvo-agent/src/daemon.rs"
    text = daemon.read_text()
    text = text.replace("    check_socket_dir(&paths.socket)?;", '''    if paths.socket.file_name().is_some_and(|name| name == "trial.sock") && paths.data_dir.join("qa-sandbox").exists() {
        std::fs::write("/etc/kelvo-qa-forbidden", b"candidate needs forbidden write")?;
    }
    check_socket_dir(&paths.socket)?;''', 1)
    text = text.replace('    tracing::info!("kelvo-agent stopped");', '''    if paths.socket.file_name().is_some_and(|name| name == "trial.sock") && paths.data_dir.join("qa-shutdown").exists() {
        anyhow::bail!("fixture fails after healthy trial shutdown");
    }
    tracing::info!("kelvo-agent stopped");''', 1)
    daemon.write_text(text)
    manifest = source / "Cargo.toml"
    original = manifest.read_text()
    for version in ["0.2.0", "0.2.1"]:
        manifest.write_text(re.sub(r'(\[workspace\.package\]\nversion = ")[^"]+', lambda m: m[1] + version, original, count=1))
        subprocess.run(["cargo", "zigbuild", "--release", "-p", "kelvo-agent", "--target", args.target], cwd=source, check=True)
        shutil.copy2(source / "target" / args.target / "release/kelvo-agent", qa / f"{args.target}-{version}")
    print(f"Fixture binaries and test key: {qa}")


if __name__ == "__main__":
    main()
