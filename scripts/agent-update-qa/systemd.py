#!/usr/bin/env python3
"""Exercise real systemd services in disposable Docker containers, without releases.

Usage: python3 scripts/agent-update-qa/systemd.py [--binary PATH ...]
Defaults to both bundled Linux binaries; arm64 Docker can emulate the x86 binary.
The image needs privileged container cgroups, never mounts the host's systemd tree.
"""
import argparse
import json
from pathlib import Path
import subprocess
import time
import uuid

ROOT = Path(__file__).resolve().parents[2]
IMAGE = "kelvo-agent-update-systemd-test"
EMULATOR_DIRECTORY = None


def run(*args, check=True):
    result = subprocess.run(args, capture_output=True, text=True)
    if check and result.returncode:
        raise RuntimeError(f"{' '.join(args)}\n{result.stdout}\n{result.stderr}")
    return result


def architecture(binary):
    with binary.open("rb") as source:
        header = source.read(20)
    machine = int.from_bytes(header[18:20], "little")
    return {62: "amd64", 183: "arm64"}[machine]


def build_image(arch):
    if arch == "amd64" and EMULATOR_DIRECTORY:
        arch = "arm64"
    run("docker", "build", "--platform", f"linux/{arch}", "-t", f"{IMAGE}-{arch}", str(Path(__file__).parent))


class Host:
    def __init__(self, binary, layout):
        self.name = f"kelvo-update-qa-{uuid.uuid4().hex[:8]}"
        self.layout = layout
        self.flags = ["--system"] if layout == "system" else []
        self.binary = "/usr/local/bin/kelvo-agent" if self.flags else "/home/kelvo/.local/bin/kelvo-agent"
        self.data = "/var/lib/kelvo-agent" if self.flags else "/home/kelvo/.local/state/kelvo-agent"
        self.units = "/etc/systemd/system" if self.flags else "/home/kelvo/.config/systemd/user"
        arch = architecture(binary)
        emulated = arch == "amd64" and EMULATOR_DIRECTORY
        container_arch = "arm64" if emulated else arch
        run("docker", "run", "-d", "--name", self.name, "--platform", f"linux/{container_arch}", "--privileged", "--cgroupns=private", "--tmpfs", "/run", "--tmpfs", "/run/lock", f"{IMAGE}-{container_arch}")
        if emulated:
            for executable in ["binfmt", "qemu-x86_64"]:
                run("docker", "cp", str(EMULATOR_DIRECTORY / executable), f"{self.name}:/usr/bin/{executable}")
            self.root("env", "QEMU_PRESERVE_ARGV0=1", "binfmt", "--install", "amd64")
        self.root("mkdir", "-p", str(Path(self.binary).parent))
        run("docker", "cp", str(binary), f"{self.name}:{self.binary}")
        self.root("chmod", "0755", self.binary)
        if not self.flags:
            self.root("chown", "-R", "kelvo:kelvo", "/home/kelvo/.local")
        self.wait(lambda: self.root("systemctl", "is-active", "user@1000.service", check=False).returncode == 0)

    def root(self, *args, check=True):
        return run("docker", "exec", self.name, *args, check=check)

    def exec(self, *args, check=True):
        prefix = [] if self.flags else ["--user", "1000", "-e", "HOME=/home/kelvo", "-e", "XDG_RUNTIME_DIR=/run/user/1000"]
        return run("docker", "exec", *prefix, self.name, *args, check=check)

    def agent(self, *args, check=True):
        return self.exec(self.binary, *args, *self.flags, check=check)

    def systemctl(self, *args, check=True):
        user = [] if self.flags else ["--user"]
        return self.exec("systemctl", *user, *args, check=check)

    @staticmethod
    def wait(condition):
        deadline = time.monotonic() + 45
        while time.monotonic() < deadline:
            if condition():
                return
            time.sleep(0.1)
        raise AssertionError("condition did not become true within 45 seconds")

    def status(self):
        return json.loads(self.agent("status", "--json").stdout)

    def ready(self):
        self.wait(lambda: self.status()["running"])
        return self.status()

    def configure(self, checks):
        self.agent("configure", json.dumps({"general": {"check_updates": checks}}))

    def prepare_interrupted(self, phase):
        self.systemctl("stop", "kelvo-agent.service")
        code = r'''
import json, pathlib, shutil, sqlite3, sys
root, binary, units, phase = map(str, sys.argv[1:])
data = pathlib.Path(root); backup = data / 'update/backup'; backup.mkdir(parents=True, exist_ok=True)
for name in ['history.sqlite','history.sqlite-wal','history.sqlite-shm','host-id','settings.json']:
    source=data/name
    if source.exists(): shutil.copy2(source, backup/name)
shutil.copy2(binary, backup/'kelvo-agent')
(backup/'units').mkdir(exist_ok=True)
for name in ['kelvo-agent.service','kelvo-agent-update.service','kelvo-agent-update.timer','kelvo-agent-recovery.service']:
    source=pathlib.Path(units)/name
    if source.exists(): shutil.copy2(source, backup/'units'/name)
(data/'update/transaction.json').write_text(json.dumps({'phase':phase,'version':'99.0.0','binary':binary}))
db=sqlite3.connect(data/'history.sqlite'); db.execute("UPDATE meta SET value=CAST(value AS INTEGER)+10000000 WHERE key='next_seq'"); db.commit(); db.close()
'''
        self.exec("python3", "-c", code, self.data, self.binary, self.units, phase)

    def sequence(self):
        return int(self.exec("python3", "-c", "import sqlite3,sys; print(sqlite3.connect(sys.argv[1]).execute(\"SELECT value FROM meta WHERE key='next_seq'\").fetchone()[0])", f"{self.data}/history.sqlite").stdout)

    def close(self):
        run("docker", "rm", "-f", self.name, check=False)


def exercise(binary, layout):
    host = Host(binary, layout)
    try:
        host.agent("install-unit")
        host.configure(False)
        host.systemctl("daemon-reload")
        host.systemctl("enable", "--now", "kelvo-agent.service", "kelvo-agent-update.timer")
        initial = host.ready()
        host.systemctl("start", "kelvo-agent-update.service")
        status = json.loads(host.agent("update-status", "--json").stdout)
        assert status["check_updates"] is False and status["status"] == "idle"
        host.systemctl("stop", "kelvo-agent.service")
        host.configure(True)
        host.systemctl("start", "kelvo-agent-update.service")
        status = json.loads(host.agent("update-status", "--json").stdout)
        assert status["status"] == "deferred" and not host.status()["running"]
        host.configure(False)
        host.systemctl("start", "kelvo-agent.service")
        host.ready()
        host.exec("python3", "-c", "import pathlib,sys; pathlib.Path(sys.argv[1]).write_text('invalid')", f"{host.data}/settings.json")
        assert host.systemctl("start", "kelvo-agent-update.service", check=False).returncode != 0
        assert host.status()["running"], "invalid preferences must not stop collection"
        host.configure(False)
        host.prepare_interrupted("Prepared")
        # The actual binary refuses public serving before recovery. This protects
        # controllers from observing sequence numbers a failed trial may roll back.
        assert host.agent("run", check=False).returncode != 0
        host.systemctl("start", "kelvo-agent.service")
        restored = host.ready()
        assert restored["host_id"] == initial["host_id"] and restored["epoch"] == initial["epoch"]
        assert host.sequence() < 10000000, "precommit recovery must restore history"
        host.prepare_interrupted("Committed")
        host.systemctl("start", "kelvo-agent.service")
        committed = host.ready()
        assert committed["host_id"] == initial["host_id"] and committed["epoch"] == initial["epoch"]
        assert host.sequence() >= 10000000, "postcommit recovery must never restore history"
        if layout == "user":
            # Real EACCES as uid 1000, independent of the test runner's uid. Other
            # cleanup must continue and JSON stdout must survive warning stderr.
            host.exec("chmod", "0500", host.units)
            report = json.loads(host.agent("uninstall", "--purge").stdout)
            failed = [step for step in report["steps"] if step["outcome"]["outcome"] == "failed"]
            assert len(failed) == 4 and all("Permission denied" in step["outcome"]["detail"] for step in failed), report
        else:
            report = json.loads(host.agent("uninstall", "--purge").stdout)
            assert all(step["outcome"]["outcome"] != "failed" for step in report["steps"]), report
        assert host.systemctl("is-active", "kelvo-agent-update.timer", check=False).returncode != 0
        assert host.exec("test", "-e", host.data, check=False).returncode != 0
        assert host.exec("test", "-e", host.binary, check=False).returncode != 0
        print(f"PASS {binary.name} {layout}: timer, opt-out, stopped collector, invalid settings, private trial gate, rollback, committed recovery, uninstall", flush=True)
    except Exception:
        print(host.root("journalctl", "--no-pager", "-n", "100", check=False).stdout)
        raise
    finally:
        host.close()


def main():
    global EMULATOR_DIRECTORY
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, action="append")
    parser.add_argument("--emulator-dir", type=Path, help="Run x86 agents in native arm64 systemd with container-local binfmt and qemu-x86_64 from this directory")
    args = parser.parse_args()
    EMULATOR_DIRECTORY = args.emulator_dir
    binaries = args.binary or [ROOT / "src-tauri/binaries/agent" / f"kelvo-agent-{target}-unknown-linux-musl" for target in ["aarch64", "x86_64"]]
    for arch in sorted({architecture(binary) for binary in binaries}):
        build_image(arch)
    for binary in binaries:
        for layout in ["system", "user"]:
            exercise(binary.resolve(), layout)


if __name__ == "__main__":
    main()
