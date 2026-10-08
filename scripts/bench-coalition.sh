#!/usr/bin/env bash
# Coalition benchmark (architecture.md, Performance budget; D-067). Builds the packaged
# app with the `bench` feature under its own identifier (com.tryopendata.kelvo.bench, so the
# user's history and settings are never touched), launches it once per scenario, and
# sums CPU time over the app plus every process macOS holds it responsible for (its
# WebKit helpers), with the same membership rule as the `self.cpu` collector
# (crates/kelvo-collect/examples/coalition.rs).
#
# Scenarios (KELVO_BENCH_SCENARIO, src-tauri/src/bench.rs):
#   tray       nothing open, the app's resting state; the budget's number
#   popover    the popover shown
#   overview   the dashboard on /dashboard/overview
#   processes  the dashboard on /dashboard/processes
#
# Reports % of one core and phys_footprint per scenario and per process, the distance
# of tray-only to perf-budget.json's coalition.target, and fails (exit 1) if tray-only
# is over coalition.baseline by more than coalition.regressionPct in two runs. The
# target covers the backgrounded app only (D-088). The visible scenarios print their
# distance to coalition.visible.baseline plus visible.regressionPct; they fail only once
# visible.blocking is true, and print "no baseline" until one is recorded.
#
# Keep hands off while it runs: the popover hides when another app activates, and the
# dashboard stops streaming when it is covered (D-036), which would understate those
# scenarios. The scenario line says how many processes were counted.
#
# Usage: scripts/bench-coalition.sh [SECONDS]        (default 120 per scenario)
#   SCENARIOS="tray popover"   only these
#   NO_BUILD=1                 reuse the last bench build
#   DIST=/abs/path/dist        bundle this prebuilt frontend instead of building src/
#   APP=/abs/path/Kelvo.app    measure this bench bundle (with NO_BUILD=1), e.g. a copy
#                              of a previous build; one bench app runs at a time
#   OUT=dir                    keep the JSON there (default: a temp dir)
#   KEEP_SETTINGS=1            use the bench identity's settings.json; by default each
#                              launch starts from shipped defaults held in memory
#                              (KELVO_BENCH_DEFAULTS), so a setting left in the file
#                              cannot spoil the run (D-073)
#   MENU_BAR="items.cpu=graph" menu bar choices on top of the defaults (KELVO_BENCH_MENU_BAR, D-102)
#
# Each scenario also prints the tray's drawn frames per minute inside the measured window
# (scripts/tray-frames.py) and what one drawn frame costs: main-thread ms (main % × 600 /
# frames per minute) and whole app process ms; a scenario with no counted frames fails
# the run, since the tray pauses while the display is off. The Mac is held awake for the run
# (caffeinate -di): a dark display stops tray redraws and the dashboard's stream, and the
# measurer fails a run that spans a sleep.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SECS="${1:-120}"
WARMUP="${WARMUP:-30}"
SCENARIOS="${SCENARIOS:-tray popover overview processes}"
BUDGET="$ROOT/perf-budget.json"
APP="${APP:-$ROOT/target/release/bundle/macos/Kelvo.app}"
BIN="$APP/Contents/MacOS/kelvo"
ID="com.tryopendata.kelvo.bench"
OUT="${OUT:-$(mktemp -d)}"
mkdir -p "$OUT"
LOGS="$HOME/Library/Logs/$ID"

if [ "$(uname -s)" != "Darwin" ]; then
  echo "bench-coalition: macOS only" >&2
  exit 2
fi

# A closed lid with no external display turns the display off, which pauses the tray and
# the dashboard's stream: the run would measure an app with nothing drawing.
lid="$(ioreg -r -k AppleClamshellState -d 4)"
if [[ $lid == *'"AppleClamshellState" = Yes'* && $lid == *'"AppleClamshellCausesSleep" = Yes'* ]]; then
  echo "bench-coalition: the lid is closed with no external display; open it first" >&2
  exit 2
fi

if [ -z "${NO_BUILD:-}" ]; then
  config="{\"identifier\":\"$ID\"}"
  if [ -n "${DIST:-}" ]; then
    # A frontend built elsewhere (an absolute path), e.g. to hold the UI fixed while
    # comparing engine changes.
    config="{\"identifier\":\"$ID\",\"build\":{\"beforeBuildCommand\":\"\",\"frontendDist\":\"$DIST\"}}"
  fi
  (cd "$ROOT" && bun tauri build --bundles app --features bench --config "$config")
fi
caffeinate -di -w $$ &
(cd "$ROOT" && cargo build --quiet --release -p kelvo-collect --example coalition)
MEASURE="$ROOT/target/release/examples/coalition"
[ -x "$BIN" ] || { echo "bench-coalition: no app at $APP" >&2; exit 2; }

app_pid() { pgrep -f "^$BIN" | head -1 || true; }

stop_app() {
  local pid
  pid="$(app_pid)"
  [ -z "$pid" ] && return 0
  kill -TERM "$pid" 2>/dev/null || true
  for _ in $(seq 1 50); do
    kill -0 "$pid" 2>/dev/null || return 0
    sleep 0.2
  done
  kill -KILL "$pid" 2>/dev/null || true
}
trap stop_app EXIT

route_of() {
  case "$1" in
    tray) echo tray ;;
    popover) echo popover ;;
    overview) echo "dashboard:/dashboard/overview" ;;
    processes) echo "dashboard:/dashboard/processes" ;;
    *) echo "bench-coalition: unknown scenario $1" >&2; exit 2 ;;
  esac
}

field() { plutil -extract "$2" raw -o - "$1"; }

defaults=1
[ -n "${KEEP_SETTINGS:-}" ] && defaults=0

run_scenario() {
  local name="$1" pid t0
  stop_app
  open -n --env "KELVO_BENCH_SCENARIO=$(route_of "$name")" \
    --env "KELVO_BENCH_DEFAULTS=$defaults" --env "KELVO_BENCH_MENU_BAR=${MENU_BAR:-}" \
    --env "RUST_LOG=info,kelvo_lib::tray=debug" --env "KELVO_BENCH_TRAY_COUNTERS_MS=5000" \
    "$APP"
  for _ in $(seq 1 50); do
    pid="$(app_pid)"
    [ -n "$pid" ] && break
    sleep 0.2
  done
  [ -n "$pid" ] || { echo "bench-coalition: the app did not start" >&2; exit 2; }
  t0="$(date +%s)"
  "$MEASURE" --pid "$pid" --warmup "$WARMUP" --seconds "$SECS" > "$OUT/$name.json"
  stop_app
  python3 "$ROOT/scripts/tray-frames.py" "$OUT/$name.json" "$LOGS" \
    "$((t0 + WARMUP))" "$((t0 + WARMUP + SECS))"
  printf 'bench: %-10s %6.3f%% CPU  %6.1f MB footprint  (%s processes)\n' "$name" \
    "$(field "$OUT/$name.json" cpu_pct)" "$(field "$OUT/$name.json" footprint_mb)" \
    "$(field "$OUT/$name.json" members)"
  if python3 - "$OUT/$name.json" <<'EOF'
import json, sys
d = json.load(open(sys.argv[1]))
for n, v in sorted(d["processes"].items(), key=lambda kv: -kv[1]["cpu_pct"]):
    print(f"         {n:<36} {v['cpu_pct']:6.3f}%  {v['footprint_mb']:6.1f} MB")
for n, v in sorted(d["app_threads"].items(), key=lambda kv: -kv[1]):
    if v >= 0.001:
        print(f"           thread {n:<29} {v:6.3f}%")
fpm = d.get("frames_per_min")
if fpm:
    main = d["app_threads"].get("main", 0.0)
    app = d["processes"].get("kelvo", {}).get("cpu_pct", 0.0)
    print(f"         tray: {fpm:.1f} frames/min; per frame {main * 600 / fpm:.1f} ms main thread,"
          f" {app * 600 / fpm:.1f} ms whole app process")
else:
    print("         tray: no frame counters in the measured window")
    sys.exit(1)
EOF
  then :; else
    echo "bench-coalition: the tray drew nothing in the $name run (display off or asleep?); its numbers are not valid" >&2
    exit 2
  fi
}

echo "bench: $SECS s per scenario after $WARMUP s warm-up; load average $(sysctl -n vm.loadavg)"
for s in $SCENARIOS; do
  run_scenario "$s"
done
echo "bench: load average $(sysctl -n vm.loadavg); JSON in $OUT"

# Visible scenarios: a regression guard with no product target (D-088).
visible_failed=0
blocking="$(field "$BUDGET" coalition.visible.blocking 2>/dev/null || echo false)"
vregress="$(field "$BUDGET" coalition.visible.regressionPct 2>/dev/null || echo 30)"
for s in $SCENARIOS; do
  case "$s" in popover|overview|processes) ;; *) continue ;; esac
  cpu="$(field "$OUT/$s.json" cpu_pct)"
  vbase="$(field "$BUDGET" "coalition.visible.baseline.${s}CpuPct" 2>/dev/null || echo "")"
  if [ -z "$vbase" ]; then
    echo "bench: $s $cpu% (visible, no baseline)"
    continue
  fi
  vceil="$(awk -v b="$vbase" -v r="$vregress" 'BEGIN { printf "%.3f", b * (1 + r / 100) }')"
  if awk -v c="$cpu" -v m="$vceil" 'BEGIN { exit !(c > m) }'; then
    echo "bench: $s $cpu% is over its visible ceiling $vceil% (baseline $vbase% + $vregress%)$([ "$blocking" = true ] || echo ", advisory")"
    [ "$blocking" = true ] && visible_failed=1
  else
    echo "bench: $s $cpu% is under its visible ceiling $vceil%"
  fi
done

case " $SCENARIOS " in
  *" tray "*) ;;
  *) exit "$visible_failed" ;;
esac

target="$(field "$BUDGET" coalition.target.trayOnlyCpuPct)"
baseline="$(field "$BUDGET" coalition.baseline.trayOnlyCpuPct)"
regress="$(field "$BUDGET" coalition.regressionPct)"
tray="$(field "$OUT/tray.json" cpu_pct)"
ceiling="$(awk -v b="$baseline" -v r="$regress" 'BEGIN { printf "%.3f", b * (1 + r / 100) }')"
awk -v c="$tray" -v t="$target" 'BEGIN {
  d = c - t
  if (d > 0) printf "bench: tray-only %.3f%% is %.3f points (%.0f%%) over the %.2f%% target\n", c, d, d / t * 100, t
  else printf "bench: tray-only %.3f%% is %.3f points under the %.2f%% target\n", c, -d, t
}'
if awk -v c="$tray" -v m="$ceiling" 'BEGIN { exit !(c > m) }'; then
  echo "bench: tray-only $tray% is over the baseline $baseline% + $regress% ($ceiling%); measuring again"
  run_scenario tray
  tray="$(field "$OUT/tray.json" cpu_pct)"
  if awk -v c="$tray" -v m="$ceiling" 'BEGIN { exit !(c > m) }'; then
    echo "bench: FAIL, tray-only $tray% is over $ceiling% twice (perf-budget.json coalition.baseline)" >&2
    exit 1
  fi
fi
echo "bench: tray-only pass (baseline ceiling $ceiling%)"
exit "$visible_failed"
