#!/usr/bin/env python3
"""Run native Tauri restart acceptance without touching installed Kelvo or its data."""
import argparse
import json
import os
from pathlib import Path
import plistlib
import shutil
import signal
import subprocess
import tempfile
import time
import uuid


def stop_owned_processes(root: Path, executable: Path) -> None:
    for filename in ("first-pid.json", "second-pid.json"):
        path = root / filename
        if not path.exists():
            continue
        pid = json.loads(path.read_text())
        for sig in (signal.SIGTERM, signal.SIGKILL):
            command = subprocess.run(["ps", "-p", str(pid), "-o", "command="],
                                     capture_output=True, text=True).stdout.strip()
            if not command.startswith(str(executable)):
                break
            try:
                os.kill(pid, sig)
            except ProcessLookupError:
                break
            time.sleep(0.1)


def run_case(binary: Path, mode: str, arguments: list[str], output: Path) -> dict:
    with tempfile.TemporaryDirectory(prefix="kelvo-update-restart-") as temporary:
        root = Path(temporary).resolve()
        identifier = f"com.tryopendata.kelvo.acceptance.{uuid.uuid4().hex}"
        bundle = root / "Kelvo update acceptance.app"
        executable = bundle / "Contents/MacOS/verify_update_restart"
        executable.parent.mkdir(parents=True)
        shutil.copy2(binary, executable)
        with (bundle / "Contents/Info.plist").open("wb") as stream:
            plistlib.dump({"CFBundleExecutable": executable.name,
                          "CFBundleIdentifier": identifier,
                          "CFBundleName": "Kelvo update acceptance",
                          "CFBundlePackageType": "APPL",
                          "CFBundleVersion": "1",
                          "LSUIElement": True}, stream)
        subprocess.run(["codesign", "--force", "--sign", "-", str(bundle)], check=True, capture_output=True)
        env = os.environ.copy()
        env.update(KELVO_ACCEPTANCE_ROOT=str(root), KELVO_ACCEPTANCE_IDENTIFIER=identifier)
        log_path = output / f"{mode}.log"
        try:
            with log_path.open("w") as log:
                process = subprocess.Popen([str(executable), *arguments], env=env, stdout=log, stderr=log)
                deadline = time.monotonic() + 60
                while not (root / "done.json").exists():
                    if (root / "failure.json").exists():
                        raise RuntimeError((root / "failure.json").read_text())
                    if time.monotonic() > deadline:
                        process.terminate()
                        raise TimeoutError(f"{mode}: native restart did not finish; see {log_path}")
                    time.sleep(0.05)
                process.wait(timeout=10)
            if "panicked at" in log_path.read_text():
                raise RuntimeError(f"{mode}: native worker panicked; see {log_path}")
            result = json.loads((root / "verified.json").read_text())
            result["mode"] = mode
            result["first_exit"] = json.loads((root / "first-exit.json").read_text())
            result["first_request"] = json.loads((root / "first-request.json").read_text())
            result["second_exit"] = json.loads((root / "done.json").read_text())
            (output / f"{mode}.json").write_text(json.dumps(result, indent=2) + "\n")
            return result
        finally:
            stop_owned_processes(root, executable)
            paths = root / "paths.json"
            if paths.exists():
                for value in json.loads(paths.read_text()).values():
                    path = Path(value)
                    if path != root / identifier and not path.is_relative_to(root / identifier):
                        raise RuntimeError("refusing cleanup outside acceptance identity")
                    shutil.rmtree(path, ignore_errors=True)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path("target/debug/examples/verify_update_restart"))
    parser.add_argument("--output", type=Path, default=Path("test-results/update-restart"))
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    args.output.mkdir(parents=True, exist_ok=True)
    results = [run_case(binary, mode, flags, args.output) for mode, flags in [
        ("ordinary", []), ("background", ["--background"]), ("remote-setup", ["--remote-setup"]),
    ]]
    print(json.dumps({"passed": len(results), "cases": results,
                      "unverified": "A genuine macOS login-item launch event requires a login session; this test does not change registration."}, indent=2))


if __name__ == "__main__":
    main()
