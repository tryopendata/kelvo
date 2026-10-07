#!/usr/bin/env python3
"""Tray frames drawn per minute inside a bench window (scripts/bench-coalition.sh).

Usage: tray-frames.py RESULT.json LOG_DIR START_EPOCH END_EPOCH

Reads the "tray frames" counter lines a bench build logs every
KELVO_BENCH_TRAY_COUNTERS_MS (src-tauri/src/bench.rs) from LOG_DIR/kelvo.*.log and adds
`frames_per_min` to RESULT.json: drawn frames (summed over items) between the first and
the last line inside [START, END], per minute of the time between those two lines. With
fewer than two lines in the window the key is left out.
"""

import glob
import json
import re
import sys
from datetime import datetime

LINE = re.compile(r"^(\S+?)Z .*tray frames.* drawn=(\d+)")


def main() -> None:
    path, logs = sys.argv[1], sys.argv[2]
    start, end = float(sys.argv[3]), float(sys.argv[4])
    points = []
    for f in sorted(glob.glob(f"{logs}/kelvo.*.log"))[-2:]:
        with open(f, errors="replace") as lines:
            for line in lines:
                m = LINE.match(line)
                if not m:
                    continue
                # Microseconds at most: fromisoformat takes six fraction digits.
                stamp = m.group(1)[:26] + "+00:00"
                t = datetime.fromisoformat(stamp).timestamp()
                if start <= t <= end:
                    points.append((t, int(m.group(2))))
    with open(path) as f:
        result = json.load(f)
    points.sort()
    if len(points) >= 2 and points[-1][0] > points[0][0]:
        (t0, n0), (t1, n1) = points[0], points[-1]
        result["frames_per_min"] = (n1 - n0) / (t1 - t0) * 60
    with open(path, "w") as f:
        json.dump(result, f)


if __name__ == "__main__":
    main()
