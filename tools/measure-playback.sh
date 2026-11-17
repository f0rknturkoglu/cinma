#!/bin/bash
# Time-to-first-frame and seek latency for one stream, through the real app.
# Each run starts with an empty data dir (no torrent cache).
#
#   tools/measure-playback.sh <addon-manifest-url> <movie|series> <id> [runs]
#   e.g. tools/measure-playback.sh https://<addon>/manifest.json movie tt1254207 5
#
# Plays the FIRST stream the addon returns, waits for the first frame, then
# seeks to 50 %, 85 % and 20 % of the duration (regions not yet downloaded)
# and quits. Env: SEEKS=0.5,0.85,0.2  APP=<path to Cinma.app>
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ADDON="${1:?addon manifest url}"; TYPE="${2:?movie|series}"; ID="${3:?id}"; RUNS="${4:-5}"
SEEKS="${SEEKS:-0.5,0.85,0.2}"
APP="${APP:-$ROOT/build/xcode/Build/Products/Release/Cinma.app}"
WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT
median() { sort -n | awk '{ a[NR] = $1 } END { if (NR == 0) print "-"; else if (NR % 2) print a[(NR + 1) / 2]; else print (a[NR / 2] + a[NR / 2 + 1]) / 2 }'; }

for i in $(seq "$RUNS"); do
  CINMA_MEASURE=1 CINMA_DATA_DIR="$WORK/data$i" CINMA_DEV_ADDON="$ADDON" \
  CINMA_AUTOPLAY="$TYPE:$ID" CINMA_AUTOSEEK="$SEEKS" CINMA_AUTOQUIT=1 \
    "$APP/Contents/MacOS/Cinma" 2>"$WORK/run$i.log" >/dev/null &
  PID=$!
  for _ in $(seq 240); do kill -0 "$PID" 2>/dev/null || break; sleep 1; done
  kill "$PID" 2>/dev/null || true
  ttff=$(sed -n 's/^CINMA_TTFF_MS=//p' "$WORK/run$i.log")
  seeks=$(sed -n 's/^CINMA_SEEK_MS=//p' "$WORK/run$i.log" | tr '\n' ' ')
  echo "run $i: ttff=${ttff:-none} ms  seeks=[${seeks}] ms"
  [ -n "$ttff" ] && echo "$ttff" >> "$WORK/ttff"
  sed -n 's/^CINMA_SEEK_MS=//p' "$WORK/run$i.log" >> "$WORK/seek"
  grep -E "CINMA_DEV|error" "$WORK/run$i.log" | grep -v "seek to" | head -3 || true
done
echo "median ttff: $(median < "$WORK/ttff" 2>/dev/null || echo -) ms   median seek: $(median < "$WORK/seek" 2>/dev/null || echo -) ms"
