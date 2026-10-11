#!/usr/bin/env python3
"""One-shot Unix-only, read-only Docker compatibility evidence; not an app adapter.

Requires Python 3 on macOS/Linux. Does not invoke Docker, launch an engine, fetch
logs or inspect containers. Output excludes names, IDs, labels and commands.
An explicit check can wake an idle runtime; this is not a background sleep probe.
"""

import argparse
import http.client
import json
from pathlib import Path
import re
import signal
import socket
import stat
import sys
import time


MAX_BYTES = 2 * 1024 * 1024
MAX_CONTAINERS = 10_000
CLIENT_MIN = (1, 40)
CLIENT_MAX = (1, 47)  # Deliberate downgrade exercise, not a Bollard version claim.


class ProbeError(ValueError):
    """A locally defined symbolic reason, never text copied from a payload."""


def api_version(value):
    if not isinstance(value, str) or not re.fullmatch(r"[0-9]{1,3}\.[0-9]{1,3}", value):
        raise ProbeError("invalid_api_version")
    return tuple(map(int, value.split(".")))


def deadline_expired(_signum, _frame):
    raise TimeoutError("probe_deadline")


def read_json(socket_path, endpoint):
    # No user-controlled request target or HTTP method, even when imported.
    if not re.fullmatch(r"/version|/v[0-9]{1,3}\.[0-9]{1,3}/(?:info|containers/json\?all=1)", endpoint):
        raise ProbeError("endpoint_not_allowlisted")
    connection = http.client.HTTPConnection("localhost", timeout=2)
    connection.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    connection.sock.settimeout(2)
    try:
        connection.sock.connect(str(socket_path))
        connection.request("GET", endpoint, headers={"Connection": "close"})
        response = connection.getresponse()
        if response.status != 200:
            raise ProbeError(f"http_status_{response.status}")
        payload = response.read(MAX_BYTES + 1)
        if len(payload) > MAX_BYTES:
            raise ProbeError("response_byte_cap")
        return json.loads(payload)
    finally:
        connection.close()


def probe(socket_path):
    started = time.monotonic()
    if not stat.S_ISSOCK(socket_path.stat().st_mode):
        raise ProbeError("not_a_unix_socket")
    version = read_json(socket_path, "/version")
    maximum = api_version(version.get("ApiVersion"))
    minimum = api_version(version.get("MinAPIVersion"))
    selected = min(CLIENT_MAX, maximum)
    if max(CLIENT_MIN, minimum) > selected:
        raise ProbeError("unsupported_api_range")
    prefix = "/v" + ".".join(map(str, selected))
    info = read_json(socket_path, prefix + "/info")
    inventory = read_json(socket_path, prefix + "/containers/json?all=1")
    if not isinstance(info, dict) or not isinstance(inventory, list):
        raise ProbeError("invalid_response_shape")
    if len(inventory) > MAX_CONTAINERS:
        raise ProbeError("inventory_count_cap")
    states = {}
    for item in inventory:
        if not isinstance(item, dict):
            raise ProbeError("invalid_inventory_entry")
        state = item.get("State")
        if state not in {"created", "running", "paused", "restarting", "removing", "exited", "dead"}:
            state = "unknown"
        states[state] = states.get(state, 0) + 1
    # All potentially sensitive payload fields are discarded without printing.
    result = {
        "status": "connected",
        "server_api_min": ".".join(map(str, minimum)),
        "server_api_max": ".".join(map(str, maximum)),
        "negotiated_probe_api": prefix[2:],
        "daemon_identity_present": isinstance(info.get("ID"), str) and bool(info["ID"]),
        "inventory_count": len(inventory),
        "inventory_states": states,
        "elapsed_ms": round((time.monotonic() - started) * 1000, 1),
    }
    for key in ("NCPU", "MemTotal", "Containers", "ContainersRunning", "ContainersStopped", "ContainersPaused"):
        if type(info.get(key)) is int and info[key] >= 0:
            result[key] = info[key]
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("socket", type=Path, help="Explicit local Unix socket path")
    args = parser.parse_args()
    signal.signal(signal.SIGALRM, deadline_expired)
    signal.setitimer(signal.ITIMER_REAL, 5)
    try:
        result = probe(args.socket.expanduser())
    except ProbeError as error:
        print(json.dumps({"status": "unavailable", "reason": str(error)}))
        return 1
    except (OSError, ValueError, TypeError, AttributeError, http.client.HTTPException, RecursionError) as error:
        # Never print exception text: parsers/OS errors can include payload data.
        print(json.dumps({"status": "unavailable", "error_type": type(error).__name__}))
        return 1
    finally:
        signal.setitimer(signal.ITIMER_REAL, 0)
    print(json.dumps(result, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
