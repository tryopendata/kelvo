#!/usr/bin/env bash
# Performance mode saving (D-088). Measures the coalition CPU (app + WebKit helpers, the
# same tool as bench-coalition.sh) with Performance mode off and on, and checks each
# case's median saving against perf-budget.json coalition.performanceMode.minSavingPp.
#
# Only the difference within a pair means anything: load on the machine moves both
# halves together (D-077). So:
#   tray, tray-battery   off and on run at the same time, as two bundles with their own
#                        identifiers (A off, B on, swapped every other pair so neither
#                        bundle is always the "on" one).
#   overview             off and on run one after the other (two dashboards would cover
#                        each other and stop streaming, D-036), alternating which goes
#                        first.
# tray-battery has the engine see the Mac on battery with the slowdown on
# (KELVO_BENCH_BATTERY), so the backed-off case measures on AC. Neither run touches the
# bench identities' settings files: KELVO_BENCH_PERFORMANCE is applied in memory.
#
# Only overview runs by default. With no window open the background already holds the
# 2 s tick, 30 s processes, 10 s temperatures and the 4 s tray redraw (D-094), so the
# tray cases should save about nothing; run them by name to check that still holds.
#
# Keep hands off while it runs, on AC. Each pair prints its own line; drop a run where a
# window flashed on screen.
#
# Usage: scripts/bench-perf-mode.sh [SECONDS]      (default 120 per run)
#   CASES="tray tray-battery overview"   these instead (default: overview)
#   PAIRS=6                              pairs per case (default: performanceMode.pairs)
#   NO_BUILD=1                           reuse the last bench build
#   OUT=dir                              keep the JSON there (default: a temp dir)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SECS="${1:-120}"
WARMUP="${WARMUP:-30}"
CASES="${CASES:-overview}"
BUDGET="$ROOT/perf-budget.json"
OUT="${OUT:-$(mktemp -d)}"
mkdir -p "$OUT"
ID_A="com.tryopendata.kelvo.bench"
ID_B="com.tryopendata.kelvo.bench2"
APP_A="$ROOT/target/release/bundle/macos/Kelvo.app"
APP_B="$ROOT/target/release/bundle/macos/Kelvo-bench2.app"

if [ "$(uname -s)" != "Darwin" ]; then
  echo "bench-perf-mode: macOS only" >&2
  exit 2
fi

# A closed lid with no external display turns the display off, which pauses the tray and
# the dashboard's stream: the run would measure an app with nothing drawing.
lid="$(ioreg -r -k AppleClamshellState -d 4)"
if [[ $lid == *'"AppleClamshellState" = Yes'* && $lid == *'"AppleClamshellCausesSleep" = Yes'* ]]; then
  echo "bench-perf-mode: the lid is closed with no external display; open it first" >&2
  exit 2
fi

field() { plutil -extract "$2" raw -o - "$1"; }
PAIRS="${PAIRS:-$(field "$BUDGET" coalition.performanceMode.pairs 2>/dev/null || echo 6)}"

# Low Power Mode would put the "off" half in Performance mode too, and battery would
# make the AC case the backed-off one (tray-battery forces back-off on AC instead).
# Older macOS reports `lowpowermode 1`; newer reports `powermode 1` (2 is High Power).
if pmset -g | awk '$1 == "lowpowermode" || $1 == "powermode" { if ($2 == 1) found = 1 } END { exit !found }'; then
  echo "bench-perf-mode: macOS Low Power Mode is on; turn it off first" >&2
  exit 2
fi
if ! pmset -g batt | grep -q "AC Power"; then
  echo "bench-perf-mode: on battery; plug in first" >&2
  exit 2
fi

# Two builds, one per identifier. Tauri compiles the identifier into the binary (data
# directory, logs, single-instance socket), so a copy with an edited Info.plist would
# still be A: its launch hands over to the running A and exits. B is built first and
# moved aside, then A; the second build only recompiles the app crate. Only the tray
# cases run B.
parallel=""
[[ " $CASES " == *" tray"* ]] && parallel=1
build() {
  (cd "$ROOT" && bun tauri build --bundles app --features bench --config "{\"identifier\":\"$1\"}")
}
if [ -z "${NO_BUILD:-}" ]; then
  if [ -n "$parallel" ]; then
    build "$ID_B"
    rm -rf "$APP_B"
    mv "$APP_A" "$APP_B"
  fi
  build "$ID_A"
fi
# Held awake for the run: a dark display stops tray redraws and the dashboard's stream.
caffeinate -di -w $$ &
(cd "$ROOT" && cargo build --quiet --release -p kelvo-collect --example coalition)
MEASURE="$ROOT/target/release/examples/coalition"
for app in "$APP_A" ${parallel:+"$APP_B"}; do
  [ -x "$app/Contents/MacOS/kelvo" ] || { echo "bench-perf-mode: no app at $app" >&2; exit 2; }
done
if [ -n "$parallel" ] && ! grep -aq "$ID_B" "$APP_B/Contents/MacOS/kelvo"; then
  echo "bench-perf-mode: $APP_B was not built as $ID_B; rebuild without NO_BUILD" >&2
  exit 2
fi

pid_of() { pgrep -f "^$1/Contents/MacOS/kelvo" | head -1 || true; }

stop() {
  local pid
  pid="$(pid_of "$1")"
  [ -z "$pid" ] && return 0
  kill -TERM "$pid" 2>/dev/null || true
  for _ in $(seq 1 50); do
    kill -0 "$pid" 2>/dev/null || return 0
    sleep 0.2
  done
  kill -KILL "$pid" 2>/dev/null || true
}
stop_all() { stop "$APP_A"; stop "$APP_B"; }
trap stop_all EXIT

# launch APP SCENARIO PERFORMANCE BATTERY -> pid
launch() {
  local app="$1" pid
  # Shipped defaults in memory: the two identities' settings files cannot differ.
  open -n --env "KELVO_BENCH_SCENARIO=$2" --env "KELVO_BENCH_PERFORMANCE=$3" \
    --env "KELVO_BENCH_BATTERY=$4" --env "KELVO_BENCH_DEFAULTS=1" "$app"
  for _ in $(seq 1 50); do
    pid="$(pid_of "$app")"
    [ -n "$pid" ] && { echo "$pid"; return 0; }
    sleep 0.2
  done
  echo "bench-perf-mode: $app did not start" >&2
  exit 2
}

cpu() { field "$1" cpu_pct; }

# One parallel pair: A and B at once, one off and one on.
parallel_pair() {
  local case="$1" i="$2" battery=0 off_app on_app off_pid on_pid
  [ "$case" = "tray-battery" ] && battery=1
  if [ $((i % 2)) -eq 0 ]; then off_app="$APP_A"; on_app="$APP_B"; else off_app="$APP_B"; on_app="$APP_A"; fi
  stop_all
  off_pid="$(launch "$off_app" tray 0 "$battery")"
  on_pid="$(launch "$on_app" tray 1 "$battery")"
  "$MEASURE" --pid "$off_pid" --warmup "$WARMUP" --seconds "$SECS" > "$OUT/$case-$i-off.json" &
  local m1=$!
  "$MEASURE" --pid "$on_pid" --warmup "$WARMUP" --seconds "$SECS" > "$OUT/$case-$i-on.json" &
  local m2=$!
  wait "$m1" "$m2"
  stop_all
}

# One sequential pair on the dashboard, alternating which half runs first.
sequential_pair() {
  local case="$1" i="$2" first second pid
  if [ $((i % 2)) -eq 0 ]; then first=off; second=on; else first=on; second=off; fi
  for half in "$first" "$second"; do
    stop_all
    pid="$(launch "$APP_A" "dashboard:/dashboard/overview" "$([ "$half" = on ] && echo 1 || echo 0)" 0)"
    "$MEASURE" --pid "$pid" --warmup "$WARMUP" --seconds "$SECS" > "$OUT/$case-$i-$half.json"
  done
  stop_all
}

budget_key() {
  case "$1" in
    tray) echo trayAc ;;
    tray-battery) echo trayBackedOff ;;
    overview) echo overview ;;
  esac
}

echo "bench-perf-mode: $PAIRS pairs per case, $SECS s per run after $WARMUP s warm-up; load average $(sysctl -n vm.loadavg)"
failed=0
for case in $CASES; do
  case "$case" in tray|tray-battery|overview) ;; *) echo "bench-perf-mode: unknown case $case" >&2; exit 2 ;; esac
  savings=()
  for i in $(seq 1 "$PAIRS"); do
    if [ "$case" = overview ]; then sequential_pair "$case" "$i"; else parallel_pair "$case" "$i"; fi
    off="$(cpu "$OUT/$case-$i-off.json")"
    on="$(cpu "$OUT/$case-$i-on.json")"
    d="$(awk -v a="$off" -v b="$on" 'BEGIN { printf "%.3f", a - b }')"
    savings+=("$d")
    printf 'bench-perf-mode: %-13s pair %d  off %6.3f%%  on %6.3f%%  saving %6.3f pp\n' "$case" "$i" "$off" "$on" "$d"
  done
  stats="$(printf '%s\n' "${savings[@]}" | sort -g | awk '{ v[NR] = $1 } END {
    m = (NR % 2) ? v[(NR + 1) / 2] : (v[NR / 2] + v[NR / 2 + 1]) / 2
    printf "%.3f %.3f %.3f", m, v[1], v[NR] }')"
  read -r median lo hi <<< "$stats"
  min="$(field "$BUDGET" "coalition.performanceMode.minSavingPp.$(budget_key "$case")" 2>/dev/null || echo "")"
  if [ -z "$min" ]; then
    echo "bench-perf-mode: $case median saving $median pp (range $lo to $hi); no minimum set"
  elif awk -v m="$median" -v t="$min" 'BEGIN { exit !(m < t) }'; then
    echo "bench-perf-mode: FAIL, $case median saving $median pp (range $lo to $hi) is under $min pp" >&2
    failed=1
  else
    echo "bench-perf-mode: $case median saving $median pp (range $lo to $hi), minimum $min pp"
  fi
done
echo "bench-perf-mode: load average $(sysctl -n vm.loadavg); JSON in $OUT"
exit "$failed"
