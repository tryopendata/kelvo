#!/usr/bin/env python3
"""Real signed HTTPS update acceptance using an isolated fixture-key source build.

Pass old/new Linux binaries built from a COPY of the source with only the update
public key replaced by an ephemeral test key and workspace version advanced.
Production source, signing keys, GitHub, and real remote hosts are never changed.
The HTTPS fixture uses the production GitHub URL and certificate validation; only
this disposable container resolves github.com to its own loopback server.
"""
import argparse
import hashlib
import json
from pathlib import Path
import tempfile

from systemd import Host, architecture, build_image, run
import systemd


TLS = r'''
set -eu
mkdir -p /srv/kelvo-qa
cd /srv/kelvo-qa
openssl req -x509 -newkey rsa:2048 -nodes -days 1 -keyout ca.key -out ca.crt -subj /CN=KelvoUpdateTestCA
openssl req -newkey rsa:2048 -nodes -keyout server.key -out server.csr -subj /CN=github.com
printf 'subjectAltName=DNS:github.com\nbasicConstraints=CA:FALSE\nkeyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n' > server.ext
openssl x509 -req -in server.csr -CA ca.crt -CAkey ca.key -CAcreateserial -out server.crt -days 1 -extfile server.ext
cp ca.crt /usr/local/share/ca-certificates/kelvo-update-qa.crt
update-ca-certificates
printf '\n127.0.0.1 github.com\n' >> /etc/hosts
'''
SERVER = '''import http.server, ssl, os
os.chdir('/srv/kelvo-qa')
server=http.server.HTTPServer(('127.0.0.1',443),http.server.SimpleHTTPRequestHandler)
context=ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
context.load_cert_chain('server.crt','server.key')
server.socket=context.wrap_socket(server.socket,server_side=True)
server.serve_forever()
'''


def write(host, path, text):
    host.root("python3", "-c", "import pathlib,sys; p=pathlib.Path(sys.argv[1]); p.parent.mkdir(parents=True,exist_ok=True); p.write_text(sys.argv[2])", path, text)


def catalog(host, binary, version, key):
    target = "linux-aarch64" if architecture(binary) == "arm64" else "linux-x86_64"
    artifact = f"/tryopendata/kelvo/releases/download/v{version}/kelvo-agent"
    host.root("mkdir", "-p", str(Path("/srv/kelvo-qa" + artifact).parent))
    run("docker", "cp", str(binary), f"{host.name}:/srv/kelvo-qa{artifact}")
    release = {"schema_version": 1, "version": version, "notes": "Isolated systemd acceptance", "protocol": {"min": 1, "max": 1}, "artifacts": {target: {"url": f"https://github.com{artifact}", "size": binary.stat().st_size, "sha256": hashlib.file_digest(binary.open("rb"), "sha256").hexdigest()}}}
    with tempfile.TemporaryDirectory(prefix="kelvo-qa-manifest-") as directory:
        manifest = Path(directory) / "release.json"
        manifest.write_text(json.dumps(release))
        run("bun", "tauri", "signer", "sign", "--private-key-path", str(key), "--password", "", str(manifest))
        destination = "/srv/kelvo-qa/tryopendata/kelvo/releases/latest/download"
        host.root("mkdir", "-p", destination)
        for path in [manifest, manifest.with_suffix(".json.sig")]:
            run("docker", "cp", str(path), f"{host.name}:{destination}/{path.name}")


def kill_updater_at(host, phase):
    # Watch the real journal and kill only this container's updater PID. Prepared
    # lasts through private health; Committed is deliberately caught before the
    # public service resumes. No production fault-injection hooks are involved.
    code = r'''import json, os, pathlib, signal, subprocess, sys, time
unit=['systemctl']+json.loads(sys.argv[1]); journal=pathlib.Path(sys.argv[2]); phase=sys.argv[3]
subprocess.run(unit+['start','--no-block','kelvo-agent-update.service'],check=True)
end=time.monotonic()+60; pid=None
while time.monotonic()<end:
    if pid is None:
        value=subprocess.check_output(unit+['show','-p','MainPID','--value','kelvo-agent-update.service'],text=True).strip()
        if value and value != '0': pid=int(value)
    try:
        current=json.loads(journal.read_text())
        if pid and current['phase']==phase:
            os.kill(pid,signal.SIGKILL); print('killed '+phase); sys.exit(0)
    except FileNotFoundError: pass
    except json.JSONDecodeError: pass
    if phase != 'Committed': time.sleep(0.001)
raise RuntimeError('did not catch journal phase '+phase)
'''
    host.exec("python3", "-c", code, json.dumps([] if host.flags else ["--user"]), f"{host.data}/update/transaction.json", phase)
    host.wait(lambda: host.systemctl("is-active", "--quiet", "kelvo-agent-update.service", check=False).returncode != 0)


def assert_trial_constraints(host):
    host.agent("update", "--request")
    host.wait(lambda: host.systemctl("is-active", "--quiet", "kelvo-agent-trial.service", check=False).returncode == 0)
    result = host.systemctl("show", "kelvo-agent-trial.service", "-p", "NoNewPrivileges", "-p", "MemoryHigh", "-p", "MemoryMax", "-p", "MemorySwapMax", "-p", "ProtectSystem", "-p", "ProtectHome").stdout
    properties = dict(line.split("=", 1) for line in result.splitlines())
    assert properties["NoNewPrivileges"] == "yes", properties
    assert properties["MemoryHigh"] == "134217728" and properties["MemoryMax"] == "268435456" and properties["MemorySwapMax"] == "0", properties
    if host.layout == "system":
        assert properties["ProtectSystem"] == "strict" and properties["ProtectHome"] == "read-only", properties
    host.wait(lambda: host.systemctl("is-active", "--quiet", "kelvo-agent-update.service", check=False).returncode != 0)


def fault_trials(host, before):
    for fault in ["qa-sandbox", "qa-shutdown"]:
        host.exec("touch", f"{host.data}/{fault}")
        if fault == "qa-shutdown":
            assert_trial_constraints(host)
        else:
            assert host.systemctl("start", "kelvo-agent-update.service", check=False).returncode != 0
        restored = host.ready()
        assert restored["daemon_version"] == before["daemon_version"] and restored["epoch"] == before["epoch"], restored
        result = json.loads(host.agent("update-status", "--json").stdout)
        assert result["status"] == "failed", result
        assert host.systemctl("is-active", "--quiet", "kelvo-agent-trial.service", check=False).returncode != 0
        host.exec("rm", f"{host.data}/{fault}")
        host.exec("rm", "-f", f"{host.data}/update/failed-version")


def exercise(old, new, version, key, layout, faults):
    host = Host(old, layout)
    try:
        host.root("sh", "-c", TLS)
        write(host, "/srv/kelvo-qa/server.py", SERVER)
        host.root("systemd-run", "--unit", "kelvo-qa-https", "python3", "/srv/kelvo-qa/server.py")
        host.exec("chmod", "0750", host.binary)
        host.agent("install-unit")
        host.configure(False)
        host.systemctl("daemon-reload")
        host.systemctl("enable", "--now", "kelvo-agent.service", "kelvo-agent-update.timer")
        before = host.ready()
        catalog(host, new, version, key)
        # Check-only reports availability without changing or restarting the daemon.
        host.agent("configure", '{"general":{"check_updates":true,"auto_install_updates":false}}')
        host.systemctl("start", "kelvo-agent-update.service")
        check = json.loads(host.agent("update-status", "--json").stdout)
        assert check["status"] == "waiting" and check["version"] == version, check
        assert host.status()["daemon_version"] == before["daemon_version"]
        host.agent("configure", '{"general":{"auto_install_updates":true}}')
        if faults:
            fault_trials(host, before)
        # Stop the actual updater at opposite sides of the commit boundary.
        phase = "Prepared" if layout == "system" else "Committed"
        kill_updater_at(host, phase)
        resumed = host.ready()
        if phase == "Prepared":
            assert resumed["daemon_version"] == before["daemon_version"], resumed
            # This one-shot request exits immediately. The host service continues
            # after the requesting process has gone, just as after SSH disconnects.
            host.agent("update", "--request")
            host.wait(lambda: json.loads(host.agent("update-status", "--json").stdout)["status"] == "updated")
        after = host.ready()
        assert after["daemon_version"] == version and after["host_id"] == before["host_id"] and after["epoch"] == before["epoch"], after
        mode = host.exec("stat", "-c", "%a", host.binary).stdout.strip()
        assert mode == "750", f"installed permissions changed to {mode}"
        result = json.loads(host.agent("update-status", "--json").stdout)
        assert result["status"] == "updated", result
        # A correctly signed manifest that relabels an executable must fail private
        # daemon health, roll back, and hold the candidate on the next scheduled run.
        catalog(host, new, "99.0.0", key)
        assert host.systemctl("start", "kelvo-agent-update.service", check=False).returncode != 0
        restored = host.ready()
        assert restored["daemon_version"] == version and restored["host_id"] == before["host_id"] and restored["epoch"] == before["epoch"]
        failed = json.loads(host.agent("update-status", "--json").stdout)
        assert failed["status"] == "failed" and failed["version"] == "99.0.0", failed
        host.systemctl("start", "kelvo-agent-update.service")
        held = json.loads(host.agent("update-status", "--json").stdout)
        assert held["status"] == "failed" and "retry" in held["message"], held
        failures = ", sandbox + post-health shutdown failure rollback" if faults else ""
        print(f"PASS {architecture(old)} {layout}: signed HTTPS check-only, automatic {version} installation, private trial health, real updater SIGKILL recovery, identity/history preservation, relabeled-version rollback, failed candidate hold{failures}", flush=True)
    except Exception:
        print(host.root("journalctl", "--no-pager", "-n", "150", check=False).stdout)
        raise
    finally:
        host.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--old", type=Path, required=True)
    parser.add_argument("--new", type=Path, required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--test-key", type=Path, required=True)
    parser.add_argument("--fault-fixtures", action="store_true", help="Binaries built by build_fixtures.py; exercise sandbox and post-health shutdown failure")
    parser.add_argument("--emulator-dir", type=Path)
    args = parser.parse_args()
    systemd.EMULATOR_DIRECTORY = args.emulator_dir
    build_image(architecture(args.old))
    for layout in ["system", "user"]:
        exercise(args.old.resolve(), args.new.resolve(), args.version, args.test_key.resolve(), layout, args.fault_fixtures)


if __name__ == "__main__":
    main()
