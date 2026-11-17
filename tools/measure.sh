#!/bin/bash
# Launch time + idle memory + idle CPU snapshot for the Release build.
#
#   tools/measure.sh              build Release, then measure
#   SKIP_BUILD=1 tools/measure.sh measure the existing build
#   RUNS=10 IDLE=10 tools/measure.sh
#
# Method: the binary is exec'd directly (not via LaunchServices/`open`), so
# times exclude LaunchServices overhead. Runs are warm (files in page cache);
# run 1 right after a build is the closest to cold. Each run uses a throwaway
# data dir. Times come from the app itself (LaunchMetrics.swift) and are
# measured from the kernel process start time.
#
# Memory: `rss` = ps RSS (includes shared dyld-cache pages, so it overstates
# what the app owns); `footprint` = phys_footprint (what Activity Monitor shows).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RUNS="${RUNS:-5}"
SETTLE="${SETTLE:-3}"   # seconds after core-ready before sampling
IDLE="${IDLE:-5}"       # idle CPU sampling window
APP="${APP:-$ROOT/build/xcode/Build/Products/Release/Cinma.app}"
BIN="$APP/Contents/MacOS/Cinma"
CSV="$ROOT/tools/measurements.csv"

if [ "${SKIP_BUILD:-0}" != 1 ]; then
  echo "Building Release…" >&2
  xcodebuild -project "$ROOT/app/Cinma.xcodeproj" -scheme Cinma -configuration Release \
    -destination 'platform=macOS,arch=arm64' -derivedDataPath "$ROOT/build/xcode" build -quiet >/dev/null
fi

cpu_seconds() { # ps "time" ([[h:]m]m:ss.cc) -> seconds
  ps -o time= -p "$1" | awk -F: '{ s = 0; for (i = 1; i <= NF; i++) s = s * 60 + $i; print s }'
}
median() { sort -n | awk '{ a[NR] = $1 } END { if (NR % 2) print a[(NR + 1) / 2]; else print (a[NR / 2] + a[NR / 2 + 1]) / 2 }'; }

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
: > "$WORK/first"; : > "$WORK/core"; : > "$WORK/rss"; : > "$WORK/fp"; : > "$WORK/cpu"

printf "%-4s %10s %10s %9s %13s %9s\n" run first_ms core_ms rss_mb footprint_mb idle_cpu%
for i in $(seq "$RUNS"); do
  LOG="$WORK/run$i.log"
  mkdir -p "$WORK/data$i"
  CINMA_MEASURE=1 CINMA_DATA_DIR="$WORK/data$i" "$BIN" 2>"$LOG" >/dev/null &
  PID=$!
  for _ in $(seq 200); do grep -q CINMA_CORE_READY_MS "$LOG" && break; sleep 0.05; done
  first=$(sed -n 's/^CINMA_FIRST_FRAME_MS=//p' "$LOG")
  core=$(sed -n 's/^CINMA_CORE_READY_MS=//p' "$LOG")
  if [ -z "$first" ] || [ -z "$core" ]; then
    echo "run $i: app did not report launch metrics" >&2; cat "$LOG" >&2
    kill "$PID" 2>/dev/null || true; exit 1
  fi
  sleep "$SETTLE"
  c0=$(cpu_seconds "$PID"); sleep "$IDLE"; c1=$(cpu_seconds "$PID")
  cpu=$(awk -v a="$c0" -v b="$c1" -v t="$IDLE" 'BEGIN { printf "%.2f", (b - a) / t * 100 }')
  rss=$(ps -o rss= -p "$PID" | awk '{ printf "%.1f", $1 / 1024 }')
  fp=$(footprint -p "$PID" 2>/dev/null | sed -n 's/.*Footprint: \([0-9.]*\) \([KMG]B\).*/\1 \2/p' | head -1 \
       | awk '{ v = $1; if ($2 == "KB") v /= 1024; if ($2 == "GB") v *= 1024; printf "%.1f", v }')
  kill "$PID"; wait "$PID" 2>/dev/null || true
  printf "%-4s %10s %10s %9s %13s %9s\n" "$i" "$first" "$core" "$rss" "${fp:-?}" "$cpu"
  echo "$first" >> "$WORK/first"; echo "$core" >> "$WORK/core"; echo "$rss" >> "$WORK/rss"
  echo "${fp:-0}" >> "$WORK/fp"; echo "$cpu" >> "$WORK/cpu"
done

m_first=$(median < "$WORK/first"); m_core=$(median < "$WORK/core")
m_rss=$(median < "$WORK/rss"); m_fp=$(median < "$WORK/fp"); m_cpu=$(median < "$WORK/cpu")
printf "%-4s %10s %10s %9s %13s %9s\n" med "$m_first" "$m_core" "$m_rss" "$m_fp" "$m_cpu"

[ -f "$CSV" ] || echo "date,commit,runs,first_frame_ms,core_ready_ms,rss_mb,footprint_mb,idle_cpu_pct,note" > "$CSV"
COMMIT=$(git -C "$ROOT" rev-parse --short HEAD 2>/dev/null || echo none)
echo "$(date +%Y-%m-%dT%H:%M:%S),$COMMIT,$RUNS,$m_first,$m_core,$m_rss,$m_fp,$m_cpu,${NOTE:-}" >> "$CSV"
echo "Appended medians to tools/measurements.csv" >&2
