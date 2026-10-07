#!/usr/bin/env bash
# Accuracy check: runs the kelvo-engine `dump` example and macmon side by side and
# compares window means of CPU cluster frequency and active residency, GPU frequency,
# GPU and system power, and CPU/GPU temperature. Fails when a mean is off by more than
# ±5% (power, frequency, residency) or ±2 °C (temperature).
#
# Usage: scripts/accuracy-vs-macmon.sh [seconds]   (default 600)
#
# Needs macmon (https://github.com/vladkens/macmon, `brew install macmon`). It does not
# install anything and does not need sudo. The macmon field names (`pcpu_usage` as
# [MHz, ratio], `gpu_power`, `sys_power`, `temp.cpu_temp_avg`, ...) follow `macmon pipe`
# 0.8 and were not checked against a live macmon: it was not installed when this was
# written.
#
# Definitions differ in places, so read a failure before blaming a collector:
# - macmon's pcpu/ecpu residency averages per-core residencies; Kelvo's
#   cpu.cluster.active is the cluster complex's HW active residency and its frequency is
#   weighted over active states only (D-043). Kelvo's P value is the mean of its P
#   clusters.
# - macmon's cpu_temp_avg/gpu_temp_avg average sensors; Kelvo's thermal.cpu/thermal.gpu
#   are the max of the chip's SMC key group.
# - On macOS 27 the PMP counters behind CPU/ANE/DRAM power refresh every ~5 minutes, so
#   Kelvo reports a gap for them (D-043); those rows are reported as skipped.
set -euo pipefail

SECONDS_TO_RUN="${1:-600}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

if ! command -v macmon >/dev/null 2>&1; then
  echo "accuracy-vs-macmon: macmon is not installed (brew install macmon); nothing compared." >&2
  exit 2
fi
if ! command -v python3 >/dev/null 2>&1; then
  echo "accuracy-vs-macmon: python3 is required to compare the outputs." >&2
  exit 2
fi

WORK="$(mktemp -d "${TMPDIR:-/tmp}/kelvo-accuracy.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

echo "chip:  $(sysctl -n machdep.cpu.brand_string)"
echo "model: $(sysctl -n hw.model)"
echo "macOS: $(sw_vers -productVersion) ($(sw_vers -buildVersion))"
echo "macmon: $(macmon --version 2>/dev/null || echo unknown)"
echo "window: ${SECONDS_TO_RUN} s"

cargo build --release -q -p kelvo-engine --example dump --manifest-path "$ROOT/Cargo.toml"
DUMP="$ROOT/target/release/examples/dump"

"$DUMP" --json --seconds "$SECONDS_TO_RUN" >"$WORK/kelvo.jsonl" 2>"$WORK/kelvo.log" &
KELVO_PID=$!
macmon pipe -i 1000 -s "$SECONDS_TO_RUN" >"$WORK/macmon.jsonl" 2>"$WORK/macmon.log" &
MACMON_PID=$!
wait "$KELVO_PID"
wait "$MACMON_PID" || true

python3 - "$WORK/kelvo.jsonl" "$WORK/macmon.jsonl" <<'PY'
import json, statistics, sys

def lines(path):
    out = []
    with open(path) as f:
        for line in f:
            line = line.strip()
            if line.startswith("{"):
                try:
                    out.append(json.loads(line))
                except json.JSONDecodeError:
                    pass
    return out

kelvo, macmon = lines(sys.argv[1]), lines(sys.argv[2])
if len(kelvo) < 10 or len(macmon) < 10:
    print(f"too few samples: kelvo {len(kelvo)}, macmon {len(macmon)}")
    sys.exit(1)

def mean(vals):
    vals = [v for v in vals if v is not None]
    return statistics.fmean(vals) if vals else None

def k_cluster(s, prefix, field):
    cl = (s.get("cpu") or {}).get("clusters") or []
    vals = [c.get(field) for c in cl if c.get("cluster", "").startswith(prefix)]
    vals = [v for v in vals if v is not None]
    return statistics.fmean(vals) if vals else None

def k(path):
    def get(s):
        cur = s
        for p in path:
            cur = (cur or {}).get(p)
        return cur
    return get

def m(path, idx=None, scale=1.0):
    def get(s):
        cur = s
        for p in path:
            cur = (cur or {}).get(p) if isinstance(cur, dict) else None
        if idx is not None:
            cur = cur[idx] if isinstance(cur, list) and len(cur) > idx else None
        return None if cur is None else cur * scale
    return get

# (name, kelvo getter, macmon getter, kind) where kind is "pct" (±5% relative) or "temp" (±2 °C).
rows = [
    ("P cluster freq (MHz)", lambda s: (k_cluster(s, "P", "freq_hz") or 0) / 1e6 or None, m(["pcpu_usage"], 0), "pct"),
    ("E cluster freq (MHz)", lambda s: (k_cluster(s, "E", "freq_hz") or 0) / 1e6 or None, m(["ecpu_usage"], 0), "pct"),
    ("P cluster active (%)", lambda s: k_cluster(s, "P", "active"), m(["pcpu_usage"], 1, 100.0), "pct"),
    ("E cluster active (%)", lambda s: k_cluster(s, "E", "active"), m(["ecpu_usage"], 1, 100.0), "pct"),
    ("GPU freq (MHz)", lambda s: (k(["gpu", "freq_hz"])(s) or 0) / 1e6 or None, m(["gpu_usage"], 0), "pct"),
    ("GPU power (W)", k(["power", "gpu"]), m(["gpu_power"]), "pct"),
    ("CPU power (W)", k(["power", "cpu"]), m(["cpu_power"]), "pct"),
    ("System power (W)", k(["power", "system"]), m(["sys_power"]), "pct"),
    ("CPU temp (C)", k(["sensors", "cpu_c"]), m(["temp", "cpu_temp_avg"]), "temp"),
    ("GPU temp (C)", k(["sensors", "gpu_c"]), m(["temp", "gpu_temp_avg"]), "temp"),
]

failed = False
print(f"{'metric':<24}{'kelvo':>12}{'macmon':>12}{'diff':>12}  result")
for name, kg, mg, kind in rows:
    kv = mean(kg(s) for s in kelvo)
    mv = mean(mg(s) for s in macmon)
    if kv is None or mv is None:
        print(f"{name:<24}{'-' if kv is None else f'{kv:.2f}':>12}{'-' if mv is None else f'{mv:.2f}':>12}{'':>12}  skipped (no data)")
        continue
    if kind == "temp":
        diff = kv - mv
        ok = abs(diff) <= 2.0
        shown = f"{diff:+.2f} C"
    else:
        # Relative to macmon, with a floor so near-idle values (0.01 W) do not dominate.
        base = max(abs(mv), 0.5)
        diff = (kv - mv) / base * 100.0
        ok = abs(diff) <= 5.0
        shown = f"{diff:+.1f}%"
    failed |= not ok
    print(f"{name:<24}{kv:>12.2f}{mv:>12.2f}{shown:>12}  {'ok' if ok else 'FAIL'}")

print(f"samples: kelvo {len(kelvo)}, macmon {len(macmon)}")
sys.exit(1 if failed else 0)
PY
