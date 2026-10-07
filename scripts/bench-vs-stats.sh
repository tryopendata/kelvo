#!/usr/bin/env bash
# Kelvo vs Stats (architecture.md, Performance budget: "at or below Stats"). Measures
# Kelvo's tray-only coalition with scripts/bench-coalition.sh, then Stats
# (/Applications/Stats.app) with the same coalition tool over the same span, one after
# the other. Stats is not installed by this script: without it the comparison is
# skipped and only Kelvo is measured.
#
# Stats runs with whatever modules its own settings enable; the comparison is only fair
# when they match Kelvo's menu bar (CPU, GPU, memory, temperature by default). If Stats
# was not running it is launched in the background for the measurement and quit after.
#
# Usage: scripts/bench-vs-stats.sh [SECONDS]   (default 600, the plan's 10 minutes)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SECS="${1:-600}"
WARMUP="${WARMUP:-30}"
STATS_APP="/Applications/Stats.app"
OUT="${OUT:-$(mktemp -d)}"
export OUT WARMUP

SCENARIOS=tray "$ROOT/scripts/bench-coalition.sh" "$SECS" || status=$?
status="${status:-0}"

if [ ! -d "$STATS_APP" ]; then
  echo "bench-vs-stats: skipped, $STATS_APP is not installed (nothing compared)"
  exit "$status"
fi

MEASURE="$ROOT/target/release/examples/coalition"
launched=""
pid="$(pgrep -x Stats | head -1 || true)"
if [ -z "$pid" ]; then
  open -g -a "$STATS_APP"
  launched=1
  for _ in $(seq 1 50); do
    pid="$(pgrep -x Stats | head -1 || true)"
    [ -n "$pid" ] && break
    sleep 0.2
  done
fi
[ -n "$pid" ] || { echo "bench-vs-stats: Stats did not start" >&2; exit 2; }
"$MEASURE" --pid "$pid" --warmup "$WARMUP" --seconds "$SECS" > "$OUT/stats.json"
if [ -n "$launched" ]; then
  osascript -e 'quit app "Stats"' >/dev/null 2>&1 || true
fi

field() { plutil -extract "$2" raw -o - "$1"; }
kelvo="$(field "$OUT/tray.json" cpu_pct)"
stats="$(field "$OUT/stats.json" cpu_pct)"
printf 'bench-vs-stats: Kelvo %.3f%% CPU %.1f MB, Stats %.3f%% CPU %.1f MB (%s s each)\n' \
  "$kelvo" "$(field "$OUT/tray.json" footprint_mb)" "$stats" "$(field "$OUT/stats.json" footprint_mb)" "$SECS"
if awk -v k="$kelvo" -v s="$stats" 'BEGIN { exit !(k > s) }'; then
  echo "bench-vs-stats: Kelvo uses more CPU than Stats"
fi
exit "$status"
