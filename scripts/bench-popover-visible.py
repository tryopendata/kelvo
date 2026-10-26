#!/usr/bin/env python3
"""Reject a popover benchmark without continuous visibility in its measured window.

Usage: bench-popover-visible.py LOG_DIR LAUNCH_EPOCH START_EPOCH END_EPOCH
The launch boundary excludes evidence left by earlier launches in the same log.
"""

import glob
import re
import sys
from datetime import datetime
from pathlib import Path

LINE = re.compile(r"^(\S+Z)\s+.*\bkelvo_lib::(popover(?:::\w+)*|state): (.*)$")
PAUSED = re.compile(r"^sampling paused state host=(\S+) paused=(true|false)\b")
OCCLUSION = re.compile(r"\bpopover occlusion visible=(true|false)\b")


def validate(logs: Path, launch: float, start: float, end: float) -> None:
    if not launch <= start < end:
        raise ValueError("invalid launch or measurement window")
    events = []
    for path in sorted(glob.glob(str(logs / "kelvo.*.log"))):
        with open(path, errors="replace") as lines:
            for line in lines:
                match = LINE.match(line)
                if not match:
                    continue
                try:
                    stamp = datetime.fromisoformat(match[1].replace("Z", "+00:00")).timestamp()
                except ValueError:
                    continue
                if launch <= stamp <= end:
                    events.append((stamp, match[2], match[3]))
    created = opened = visible = False
    paused_hosts = set()
    for stamp, target, message in sorted(events, key=lambda event: event[0]):
        if stamp >= start and not (created and opened and visible):
            raise ValueError("popover was not confirmed open and visible before measurement")
        if stamp >= start and paused_hosts:
            raise ValueError("sampling was paused before measurement")
        if target == "state":
            if match := PAUSED.match(message):
                if stamp >= start:
                    raise ValueError("sampling pause state changed during measurement")
                if match[2] == "true":
                    paused_hosts.add(match[1])
                else:
                    paused_hosts.discard(match[1])
            elif stamp >= start and message.startswith("settings changed revision="):
                raise ValueError("settings changed during measurement")
            continue
        if message == "popover panel created":
            if stamp >= start:
                raise ValueError("popover panel recreated during measurement")
            created, opened, visible = True, False, False
        elif message.startswith("popover opened "):
            if stamp < start:
                opened = True
        elif match := OCCLUSION.search(message):
            visible = match[1] == "true"
            if stamp >= start and not visible:
                raise ValueError("popover became hidden or occluded during measurement")
    # A quiet, continuously visible panel need not emit anything during measurement.
    if paused_hosts:
        raise ValueError("sampling was paused before measurement")
    if not (created and opened and visible):
        raise ValueError("missing current-launch created, opened and visible evidence")


def main() -> None:
    try:
        logs, launch, start, end = sys.argv[1:]
        validate(Path(logs), float(launch), float(start), float(end))
    except (ValueError, OSError) as error:
        print(f"bench-coalition: invalid popover measurement: {error}", file=sys.stderr)
        sys.exit(2)


if __name__ == "__main__":
    main()
