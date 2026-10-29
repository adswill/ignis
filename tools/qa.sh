#!/usr/bin/env bash
# Ignis QA regression harness.
#   tools/qa.sh                 build (with retries) + run every bench scene + write tools/QA_REPORT.md
# Env:
#   QA_NO_BUILD=1               skip cargo build (use the existing binary)
#   QA_BUILD_TRIES=10           build attempts (others may be mid-edit)
#   QA_BUILD_WAIT=60            seconds between build attempts
#   QA_SCENES="jet rocket"      only run these scenes
#   QA_FAST=1                   halve frame counts (smoke run; thresholds that need long runs may fail)
#   QA_OUT=/path                image/log output dir (default: workflow scratch dir)
#   QA_REPORT=/path/report.md   report path (default tools/QA_REPORT.md)
#   QA_TIMEOUT=240              per-scene timeout, seconds
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
OUT="${QA_OUT:-${TMPDIR:-/tmp}/ignis-qa}"
mkdir -p "$OUT"
BUILD_LOG="$OUT/build.log"
TRIES="${QA_BUILD_TRIES:-10}"
WAIT="${QA_BUILD_WAIT:-60}"
BUILD_STATUS="skipped"
BUILD_ATTEMPTS=0
BUILD_SECS=0

if [ "${QA_NO_BUILD:-0}" != "1" ]; then
  BUILD_STATUS="failed"
  for i in $(seq 1 "$TRIES"); do
    BUILD_ATTEMPTS=$i
    t0=$(date +%s)
    echo "[qa] cargo build --release (attempt $i/$TRIES)"
    if cargo build --release >"$BUILD_LOG" 2>&1; then
      BUILD_SECS=$(( $(date +%s) - t0 ))
      BUILD_STATUS="ok"
      break
    fi
    BUILD_SECS=$(( $(date +%s) - t0 ))
    echo "[qa] build failed; errors:"
    grep -E "^(error|warning: unused)|-->" "$BUILD_LOG" | grep -E "^error|-->" | head -20
    if [ "$i" -lt "$TRIES" ]; then echo "[qa] retrying in ${WAIT}s"; sleep "$WAIT"; fi
  done
fi

if [ ! -x "$ROOT/target/release/ignis" ]; then
  echo "[qa] no binary at target/release/ignis"
fi

exec python3 -I "$ROOT/tools/qa_run.py" \
  --root "$ROOT" --out "$OUT" \
  --build-status "$BUILD_STATUS" --build-attempts "$BUILD_ATTEMPTS" --build-secs "$BUILD_SECS" \
  --build-log "$BUILD_LOG"
