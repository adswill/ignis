#!/usr/bin/env bash
# IGNIS profiling driver (read-only on the game's sources). Produces raw numbers for tools/PROFILE.md.
#
#   tools/profile.sh                 # everything below except xctrace
#   ONLY=gpu tools/profile.sh        # just the per-kernel GPU profiler (examples/gpu_profile.rs)
#   ONLY=bench|app|gpu               # pick one stage
#   SCENES=rocket,campfire STEPS=19 FRAMES=40 WARM=120 tools/profile.sh
#   APP_SCENE=rocket APP_SPEED=1 APP_FRAMES=600 tools/profile.sh   # real-app pass (opens the window ~15 s)
#   XCTRACE=1 tools/profile.sh       # also record a Metal System Trace of the headless bench (.trace, open in Instruments)
#   OUT=/abs/dir tools/profile.sh    # output folder (default: $TMPDIR/ignis_profile)
#
# Stages
#   gpu   : per-dispatch GPU timestamps (wgpu TIMESTAMP_QUERY at pass boundaries), real GpuSim throughput,
#           render-pass cost at several resolutions, calibration (bandwidth roof), memory accounting.
#   bench : the game's own --bench at STEPS=0/19/40 -> per-step and fixed per-frame cost (wall clock).
#   app   : the real eframe app via the COMBUSTOR_SHOT screenshot hook. Two runs (SHOT_FRAMES=a, b) ->
#           (t_b - t_a)/(b - a) = real frame time incl. egui + vsync. While the long run is up it samples
#           CPU %, Energy Impact (`top -stats power`), phys_footprint (`footprint`) and a 3 s CPU call-stack
#           profile (`sample`), summarised to the top egui / wgpu / Metal / app frames.
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${OUT:-${TMPDIR:-/tmp}/ignis_profile}"
mkdir -p "$OUT"
cd "$ROOT" || exit 1
ONLY="${ONLY:-all}"
BIN="$ROOT/target/release/ignis"
PROF="$ROOT/target/release/examples/gpu_profile"

retry_build() {
  # other teams edit the tree concurrently; retry a few times
  for i in 1 2 3 4 5 6 7 8 9 10; do
    if cargo build --release "$@" >"$OUT/build.log" 2>&1; then return 0; fi
    echo "build failed (attempt $i), retrying in 60 s: $(grep -m1 '^error' "$OUT/build.log")"
    sleep 60
  done
  return 1
}

now() { python3 -c 'import time; print(time.time())'; }
gpu_util() { ioreg -r -d 1 -c IOAccelerator | grep -o '"Device Utilization %"=[0-9]*' | head -1 | cut -d= -f2; }
others() { ps -Ao pid=,command= | grep -E "target/release/(ignis|combustor|examples/)" | grep -v -E "grep|zsh -c|bash -c|timeout " | grep -v "^ *$$ "; }
echo "GPU Device Utilization before start: $(gpu_util)% (should be ~0 for clean numbers)"
if [[ -n "$(others)" ]]; then echo "WARNING: other GPU users running, numbers will be inflated:"; others | cut -c1-110; fi

if [[ "$ONLY" == all || "$ONLY" == gpu ]]; then
  echo "== gpu: building examples/gpu_profile"
  retry_build --example gpu_profile || exit 1
  echo "== gpu: running (output -> $OUT/gpu_profile.md)"
  PROFILE_CSV="$OUT/gpu_profile.csv" "$PROF" 2>&1 | tee "$OUT/gpu_profile.md"
fi

if [[ "$ONLY" == all || "$ONLY" == bench ]]; then
  echo "== bench: building"
  retry_build || exit 1
  : >"$OUT/bench.txt"
  for sc in ${BENCH_SCENES:-empty rocket campfire chem}; do
    for st in 0 19 40; do
      t0=$(now)
      STEPS=$st EVERY=100000 "$BIN" --bench 200 "$sc" >/dev/null 2>&1
      t1=$(now)
      ms=$(python3 -c "print(f'{($t1-$t0)*1000/200:.3f}')")
      echo "bench $sc STEPS=$st: $ms ms/frame (200 frames, incl. startup + 1 readback)" | tee -a "$OUT/bench.txt"
    done
  done
  python3 - "$OUT/bench.txt" <<'PY'
import re,sys,collections
d=collections.defaultdict(dict)
for l in open(sys.argv[1]):
    m=re.match(r"bench (\S+) STEPS=(\d+): ([\d.]+)",l)
    if m: d[m[1]][int(m[2])]=float(m[3])
for sc,v in d.items():
    if 0 in v and 40 in v:
        print(f"  {sc}: per step {(v[40]-v[0])/40:.4f} ms, fixed {v[0]:.3f} ms/frame, 19 steps {v.get(19,0):.3f} ms/frame")
PY
fi

if [[ "$ONLY" == all || "$ONLY" == app ]]; then
  echo "== app: building"
  retry_build || exit 1
  SC="${APP_SCENE:-rocket}"; SPD="${APP_SPEED:-1}"; FB="${APP_FRAMES:-600}"; FA=60
  run_app() { # $1 frames -> seconds
    local t0 t1
    t0=$(now)
    COMBUSTOR_SHOT="$OUT/app_$1.ppm" SHOT_SCENE="$SC" SHOT_SPEED="$SPD" SHOT_FRAMES="$1" "$BIN" >"$OUT/app_$1.log" 2>&1 &
    local pid=$!
    if [[ "$1" == "$FB" ]]; then
      sleep 4
      if kill -0 $pid 2>/dev/null; then
        top -l 6 -s 1 -pid $pid -stats pid,cpu,power,mem,threads >"$OUT/app_top.txt" 2>&1 &
        footprint $pid >"$OUT/app_footprint.txt" 2>&1
        for k in 1 2 3 4 5; do echo "gpu util $(gpu_util)%"; sleep 0.5; done >"$OUT/app_gpu_util.txt"
        if [[ "${POWERMETRICS:-0}" == 1 ]]; then
          # needs passwordless sudo; fails quietly otherwise (run it yourself: sudo powermetrics --samplers gpu_power,cpu_power -i 1000 -n 5)
          sudo -n powermetrics --samplers gpu_power,cpu_power -i 1000 -n 3 >"$OUT/app_power.txt" 2>&1 || echo "powermetrics needs sudo" >"$OUT/app_power.txt"
        fi
        sample $pid 3 -mayDie -file "$OUT/app_sample.txt" >/dev/null 2>&1
        wait %2 2>/dev/null
      fi
    fi
    wait $pid
    t1=$(now)
    python3 -c "print($t1-$t0)"
  }
  ta=$(run_app $FA); tb=$(run_app $FB)
  python3 - "$ta" "$tb" "$FA" "$FB" "$SC" "$SPD" <<'PY' | tee "$OUT/app.txt"
import sys
ta,tb,fa,fb=float(sys.argv[1]),float(sys.argv[2]),int(sys.argv[3]),int(sys.argv[4])
ft=(tb-ta)*1000/(fb-fa)
print(f"app scene={sys.argv[5]} speed={sys.argv[6]}: {fa} frames {ta:.2f}s, {fb} frames {tb:.2f}s -> {ft:.2f} ms/frame = {1000/ft:.1f} fps (vsync on: 16.67 ms = keeping up)")
PY
  if [[ -f "$OUT/app_top.txt" ]]; then
    echo "-- top (CPU %, Energy Impact 'POWER', mem):"; grep -E "^\s*[0-9]+ " "$OUT/app_top.txt" | tail -5
  fi
  [[ -f "$OUT/app_gpu_util.txt" ]] && { echo "-- GPU utilisation while running:"; cat "$OUT/app_gpu_util.txt" | tr '\n' ' '; echo; }
  [[ -f "$OUT/app_power.txt" ]] && { echo "-- powermetrics:"; grep -E "GPU Power|CPU Power|Combined Power|GPU HW active" "$OUT/app_power.txt" | head -8; }
  if [[ -f "$OUT/app_footprint.txt" ]]; then
    echo "-- footprint:"; grep -E "phys_footprint|IOAccelerator|IOSurface|MALLOC" "$OUT/app_footprint.txt" | head -8
  fi
  if [[ -f "$OUT/app_sample.txt" ]]; then
    echo "-- sample: heaviest symbols (self samples, main thread + others)"
    python3 - "$OUT/app_sample.txt" <<'PY' | tee "$OUT/app_sample_summary.txt"
import re,sys,collections
txt=open(sys.argv[1],errors='replace').read()
# "Sort by top of stack" section lists self time per symbol
sec=txt.split("Sort by top of stack, same collapsed")[-1] if "Sort by top of stack" in txt else ""
rows=[]
for l in sec.splitlines()[1:60]:
    m=re.match(r"\s+(.*?)\s+\(in (.*?)\)\s.*?(\d+)\s*$",l)
    if m: rows.append((int(m[3]),m[1][:90],m[2]))
tot=sum(r[0] for r in rows) or 1
for n,s,lib in rows[:25]:
    print(f"  {n:5d} {100*n/tot:5.1f}%  {lib:22s} {s}")
# bucket by library/crate
b=collections.Counter()
for n,s,lib in rows:
    k=lib
    if lib.startswith("ignis") or lib.startswith("combustor"):
        k = "egui" if ("egui" in s or "epaint" in s or "emath" in s) else ("wgpu" if "wgpu" in s or "naga" in s else ("winit" if "winit" in s else "ignis(app)"))
    b[k]+=n
print("  by bucket: " + ", ".join(f"{k} {100*v/tot:.0f}%" for k,v in b.most_common(10)))
# main-thread view: how much of the main thread is blocked in nextDrawable (GPU-bound) vs egui/app work
cg=txt.split("Call graph:")[-1].split("Total number in stack")[0]
m0=re.search(r"(\d+) Thread_\S+\s+DispatchQueue_1: com.apple.main-thread", cg)
tot_m=int(m0[1]) if m0 else 1
main=cg[m0.end():].split("Thread_")[0] if m0 else ""
def cnt(pat):
    m=[int(x) for x in re.findall(r"(\d+) [^\n]*"+pat, main)]
    return max(m) if m else 0
dur=float(re.search(r"Duration:\s*([\d.]+)", txt)[1]) if re.search(r"Duration:\s*([\d.]+)", txt) else 3.0
print(f"  main thread samples: {tot_m} over {dur:.1f} s (1 ms interval)")
for lab,pat in [("blocked in CAMetalLayer nextDrawable (GPU-bound)","nextDrawable"),("App::update (egui UI build, app logic)","3App6update"),("egui tessellate","tessellate"),("egui_wgpu update_buffers (incl. GpuSim::frame encode)","update_buffers"),("GpuSim::frame encode","GpuSim5frame"),("queue submit","6submit")]:
    c=cnt(pat)
    print(f"  main thread: {100*c/tot_m:5.1f}%  ({c} ms in {dur:.1f} s)  {lab}")
PY
  fi
fi

if [[ "${XCTRACE:-0}" == 1 ]]; then
  echo "== xctrace: Metal System Trace of 60 bench frames (rocket)"
  rm -rf "$OUT/bench.trace"
  xcrun xctrace record --template 'Metal System Trace' --output "$OUT/bench.trace" --launch -- "$BIN" --bench 60 rocket >"$OUT/xctrace.log" 2>&1 \
    && echo "trace -> $OUT/bench.trace (open in Instruments: GPU timeline, per-encoder time, shader occupancy)" \
    || echo "xctrace failed, see $OUT/xctrace.log"
fi
echo "outputs in $OUT"
