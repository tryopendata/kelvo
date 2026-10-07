#!/usr/bin/env bash
# Engine CPU gate (D-062, D-067, D-075). Builds the release engine (the `dump` example:
# real collectors, real store, no UI) and runs it for SECONDS, one run at a time:
#
#   - tray-only with the 1 s setting, the app's resting state: this run is the gate.
#     With no window open the engine ticks at the background's 2 s (D-094);
#   - then tray-only at 30 s, reported only, to show what the longest common interval
#     costs.
#
# The runs never overlap: each one's processes collector reads every process on the
# machine, the other engine included, and two engines compete for the same cores.
#
# CPU is getrusage(RUSAGE_SELF) user + system over the run after a 15 s warm-up, as
# percent of one core; it covers the engine, the collectors and the store writer. The
# run also prints where it went: each collector's thread CPU time and the engine core.
#
# A run that ticked fewer than 90% of the expected times measured something else (the
# engine backs off to 2 s on battery and in Low Power Mode, and a saturated machine
# misses ticks), so it is not a result: the script exits 2 without a verdict.
#
# perf-budget.json holds two numbers for it:
#   - engine.perf.baseline.trayOnlyCpuPct, the last measurement: a run more than
#     engine.perf.regressionPct over it is measured again, and fails if the second run
#     is over too (one noisy run is not a regression). The band is as wide as the
#     run-to-run noise of identical code on the dev machine (D-075), so it catches a
#     large regression, not a small one; compare a change against a parallel run of the
#     previous build for anything finer (.claude/rules/verification.md);
#   - coalition.target.trayOnlyCpuPct, the whole app's budget (architecture.md): the
#     engine is one part of it, and every run prints how much of it the engine uses.
# Lowering the baseline is a ratchet anyone can turn; raising it needs a decision entry.
# The whole app is measured by scripts/bench-coalition.sh.
#
# Usage: scripts/perf.sh [SECONDS]   (default 120)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SECS="${1:-120}"
WARMUP=15
BUDGET="$ROOT/perf-budget.json"

if [ "$SECS" -le $((WARMUP + 10)) ]; then
  echo "perf: SECONDS must be over $((WARMUP + 10)) (a ${WARMUP} s warm-up comes first)" >&2
  exit 2
fi

# Held awake for the run; dump fails a run that spans a sleep anyway.
caffeinate -i -w $$ &
cargo build --quiet --release -p kelvo-engine --example dump
BIN="$ROOT/target/release/examples/dump"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

field() { plutil -extract "$2" raw -o - "$1"; }

# run NAME INTERVAL_MS: one engine run, alone; leaves $TMP/NAME.json.
run() {
  local name="$1" interval="$2"
  if ! "$BIN" --perf --seconds "$SECS" --warmup "$WARMUP" --interval-ms "$interval" \
    --db "$TMP/$name.sqlite" > "$TMP/$name.json" 2> "$TMP/$name.log"; then
    echo "perf: the $name engine run failed" >&2
    tail -5 "$TMP/$name.log" >&2
    exit 2
  fi
  rm -f "$TMP/$name".sqlite*
  local frames expected tick
  frames="$(field "$TMP/$name.json" frames)"
  # The tick the engine ran at: with no window open it is the background's (D-094).
  tick="$(field "$TMP/$name.json" tick_ms 2>/dev/null || true)"
  if ! [[ $tick =~ ^[1-9][0-9]*$ ]]; then
    echo "perf: not measured: the $name run reported no tick (a stale dump build?)" >&2
    exit 2
  fi
  expected=$(((SECS - WARMUP) * 1000 / tick))
  # A run that missed ticks measured a different workload, not a cheaper one.
  if [ $((frames * 10)) -lt $((expected * 9)) ]; then
    echo "perf: not measured: the $name run ticked $frames of $expected times." >&2
    echo "perf: the engine backs off to 2 s on battery and in Low Power Mode, and a" >&2
    echo "perf: saturated machine misses ticks. $(pmset -g batt | head -1)" >&2
    echo "perf: load average $(sysctl -n vm.loadavg)" >&2
    exit 2
  fi
}

# gate_run: the gated tray-only run at the 1 s setting, with where its time went.
gate_run() {
  run 1s 1000
  printf 'perf: tray-only 1 s setting (%s ms tick):  %.3f%% CPU over %s frames\n' \
    "$(field "$TMP/1s.json" tick_ms)" "$(field "$TMP/1s.json" cpu_pct)" "$(field "$TMP/1s.json" frames)"
  python3 - "$TMP/1s.json" <<'EOF'
import json, sys
d = json.load(open(sys.argv[1]))
print(f"perf:   collectors {d['collectors_pct']:.3f}%, engine core {d['engine_core_pct']:.3f}%")
for k, v in sorted(d["collectors"].items(), key=lambda kv: -kv[1]["pct"]):
    if v["pct"] >= 0.001:
        print(f"perf:     {k:<14} {v['pct']:.3f}%  ({v['samples']} samples)")
EOF
  echo "perf: load average $(sysctl -n vm.loadavg)"
}

baseline="$(field "$BUDGET" engine.perf.baseline.trayOnlyCpuPct)"
regress="$(field "$BUDGET" engine.perf.regressionPct)"
target="$(field "$BUDGET" coalition.target.trayOnlyCpuPct)"
ceiling="$(awk -v b="$baseline" -v r="$regress" 'BEGIN { printf "%.3f", b * (1 + r / 100) }')"

gate_run
cpu="$(field "$TMP/1s.json" cpu_pct)"
awk -v c="$cpu" -v t="$target" 'BEGIN {
  printf "perf: the engine alone uses %.0f%% of the %.2f%% whole-app target", c / t * 100, t
  if (c >= t) printf " (%.3f points over before the shell and WebKit)", c - t
  printf "\n"
}'
verdict=pass
if awk -v c="$cpu" -v m="$ceiling" 'BEGIN { exit !(c > m) }'; then
  echo "perf: $cpu% is over the baseline $baseline% + $regress% ($ceiling%); measuring again"
  gate_run
  cpu="$(field "$TMP/1s.json" cpu_pct)"
  if awk -v c="$cpu" -v m="$ceiling" 'BEGIN { exit !(c > m) }'; then
    verdict=fail
  fi
fi

# Reported only, after the gate so it never runs beside a gated run.
run 30s 30000
printf 'perf: tray-only 30 s: %.3f%% CPU over %s frames (reported only)\n' \
  "$(field "$TMP/30s.json" cpu_pct)" "$(field "$TMP/30s.json" frames)"

if [ "$verdict" = fail ]; then
  echo "perf: FAIL, tray-only CPU $cpu% is over $ceiling% twice (perf-budget.json engine.perf.baseline)" >&2
  exit 1
fi
echo "perf: pass (baseline $baseline%, ceiling $ceiling%)"
