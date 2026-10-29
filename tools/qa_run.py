#!/usr/bin/env python3
"""Ignis QA runner: runs every bench scene, parses stats, checks assertions, writes a Markdown report.

Called by tools/qa.sh (which handles the cargo build). Can also be run directly:
    python3 -I tools/qa_run.py --root . --out /tmp/qa
Scene list is discovered from src/main.rs (the `"name" => scene::Scene::...` match arms, plus the scene.rs
SHOWCASE names when main.rs routes through `Scene::by_name`). Scenes with no entry in CHECKS below still get the
generic checks (exit code, no NaN/bad cells, sane ranges, image not blank). Scenes in CHECKS that the bench
cannot reach are reported as SKIP.
"""
import argparse
import math
import os
import re
import subprocess
import sys
import time

# ----------------------------------------------------------------------------------------------------------------
# Ownership (ARCHITECTURE.md "Ownership")
OWNER_OF_FILE = {
    "vis.wgsl": "vfx", "render.wgsl": "vfx", "particles.wgsl": "vfx", "fx.wgsl": "vfx", "vfx.rs": "vfx",
    "matter.wgsl": "chem", "elements.rs": "chem", "scene.rs": "chem",
    "sim.wgsl": "phys", "main.rs": "phys", "gpu.rs": "phys (compute) / vfx (render)",
    "app.rs": "ui", "missions.rs": "ui",
}

MOLTEN = 24          # matter element id: molten metal
AL, STEEL, TI, INCONEL, WOOD, PROPELLANT = 1, 0, 3, 4, 7, 8  # wall material ids
WALL_T, MATTER_T = 1, 8

# ----------------------------------------------------------------------------------------------------------------
# helpers over a parsed run `r`


def first(r, key):
    return r["samples"][0][key] if r["samples"] else None


def last(r, key):
    return r["samples"][-1][key] if r["samples"] else None


def late(r, key, frac=0.25, fn=max):
    """fn() of `key` over the samples in the last `frac` of the run (robust to frame-to-frame flicker)."""
    s = r["samples"]
    if not s:
        return None
    f0 = s[-1]["frame"] * (1.0 - frac)
    vals = [x[key] for x in s if x["frame"] >= f0]
    return fn(vals) if vals else None


def peak(r, key):
    return max((x[key] for x in r["samples"]), default=None)


def tiles(r, idx, tile, mat):
    t = r["tiles"]
    if not t:
        return None
    return t[idx].get((tile, mat), 0)


def tiles_max(r, tile, mat):
    return max((t.get((tile, mat), 0) for t in r["tiles"]), default=None)


def thrust_mag(sample):
    return math.hypot(sample["tx"], sample["ty"])


def late_thrust(r, frac=0.25):
    s = r["samples"]
    if not s:
        return None
    f0 = s[-1]["frame"] * (1.0 - frac)
    return max((thrust_mag(x) for x in s if x["frame"] >= f0), default=None)


def ratio(a, b):
    if a is None or b is None or b == 0:
        return None
    return a / b


# A check = (name, owner, fn(run) -> (ok: bool|None, observed: str)). ok None = could not evaluate (FAIL).

def chk_walls_intact(minfrac, owner="phys"):
    def f(r):
        q = ratio(last(r, "walls"), first(r, "walls"))
        return (q is not None and q >= minfrac, f"walls {last(r, 'walls')}/{first(r, 'walls')} = {q*100 if q else float('nan'):.1f}%")
    return (f"walls >= {minfrac*100:.0f}% intact", owner, f)


def chk_late_thrust(gt):
    def f(r):
        v = late_thrust(r)
        return (v is not None and v > gt, f"late |thrust| max {v if v is not None else float('nan'):.2f}")
    return (f"|thrust| > {gt} (last 25% of run)", "phys", f)


def chk_late_burn(gt, owner="phys"):
    def f(r):
        v = late(r, "burn")
        return (v is not None and v > gt, f"late burn max {v if v is not None else float('nan'):.3f}")
    return (f"burn > {gt} (last 25% of run)", owner, f)


def chk_peak_burn(gt, owner="phys"):
    def f(r):
        v = peak(r, "burn")
        return (v is not None and v > gt, f"peak burn {v if v is not None else float('nan'):.3f}")
    return (f"peak burn > {gt}", owner, f)


def chk_peak_p(gt, owner="phys"):
    def f(r):
        v = peak(r, "pmax")
        return (v is not None and v > gt, f"peak pmax {v if v is not None else float('nan'):.2f}")
    return (f"peak pmax > {gt} atm", owner, f)


def chk_mat_drop(mat, maxfrac, label, owner="phys/chem"):
    def f(r):
        a, b = tiles(r, 0, WALL_T, mat), tiles(r, -1, WALL_T, mat)
        q = ratio(b, a)
        return (q is not None and q <= maxfrac, f"{label} walls {b}/{a}" + (f" = {q*100:.1f}%" if q is not None else " (no TILES data)"))
    return (f"{label} vessel destroyed (<= {maxfrac*100:.0f}% left)", owner, f)


def chk_mat_kept(mat, minfrac, label, owner="phys/chem"):
    def f(r):
        a, b = tiles(r, 0, WALL_T, mat), tiles(r, -1, WALL_T, mat)
        q = ratio(b, a)
        return (q is not None and q >= minfrac, f"{label} walls {b}/{a}" + (f" = {q*100:.1f}%" if q is not None else " (no TILES data)"))
    return (f"{label} intact (>= {minfrac*100:.0f}% left)", owner, f)


def chk_mat_any_drop(mat, label, owner="phys/chem"):
    def f(r):
        a, b = tiles(r, 0, WALL_T, mat), tiles(r, -1, WALL_T, mat)
        return (a is not None and b is not None and b < a, f"{label} walls {a} -> {b}")
    return (f"{label} breached (count drops)", owner, f)


def chk_molten(owner="chem"):
    def f(r):
        v = tiles_max(r, MATTER_T, MOLTEN)
        return (v is not None and v > 0, f"max molten-metal (8,{MOLTEN}) tiles {v}")
    return (f"molten metal (element {MOLTEN}) appears", owner, f)


def chk_melt(owner="phys/chem"):
    def f(r):
        a, b = tiles(r, 0, WALL_T, AL), tiles(r, -1, WALL_T, AL)
        m = tiles_max(r, MATTER_T, MOLTEN)
        ok = (m is not None and m > 0) or (a is not None and b is not None and b < a)
        return (ok, f"Al walls {a} -> {b}, max molten {m}")
    return ("aluminium plate melts (Al drops or molten metal appears)", owner, f)


def chk_front_advances(dx):
    def f(r):
        fr = r["fronts"]
        if len(fr) < 2:
            return (None, f"only {len(fr)} front samples")
        # frame-0 sample is taken before the strip is lit; compare against the earliest of the first two samples
        base = min(fr[:2])
        return (fr[-1] - base >= dx and 20 < fr[-1] < 490, f"front x samples {fr[0]}, {fr[1]} ... {fr[-1]} (max {max(fr)})")
    return (f"flame front advances >= {dx} cells", "phys", f)


def chk_quiet():
    def f(r):
        t, p, v = peak(r, "tmax"), peak(r, "pmax"), peak(r, "vmax")
        ok = t is not None and t < 1.1 and p < 1.2 and v < 0.1
        return (ok, f"peak Tmax {t:.3f} pmax {p:.3f} vmax {v:.3f}" if t is not None else "no samples")
    return ("empty box stays quiescent (T<1.1, p<1.2, v<0.1)", "phys", f)


# Per-scene config: frames, env, checks. Frame counts are at default 20 steps/frame.
CHECKS = {
    "jet":      dict(frames=400, env={}, checks=[chk_walls_intact(0.99), chk_late_thrust(20), chk_late_burn(0.2)]),
    "rocket":   dict(frames=300, env={}, checks=[chk_walls_intact(0.98), chk_late_thrust(250), chk_peak_p(8)]),
    "vessels":  dict(frames=400, env={"TILES": "1"}, checks=[chk_mat_drop(AL, 0.7, "aluminium"), chk_mat_kept(TI, 0.98, "titanium")]),
    "demo":     dict(frames=300, env={"TILES": "1"}, checks=[chk_mat_any_drop(TI, "titanium vault")]),
    "flare":    dict(frames=400, env={}, checks=[chk_late_burn(0.05)]),
    "flame":    dict(frames=300, env={"FRONT": "1"}, checks=[chk_front_advances(20)]),
    "fireball": dict(frames=200, env={}, checks=[chk_peak_burn(0.05)]),
    "campfire": dict(frames=400, env={"TILES": "1"}, checks=[chk_peak_burn(0.01)]),
    "srm":      dict(frames=300, env={"TILES": "1"}, checks=[chk_mat_kept(STEEL, 0.98, "steel case", "phys"), chk_mat_kept(INCONEL, 0.98, "inconel nozzle", "phys"),
                                                    chk_mat_any_drop(PROPELLANT, "propellant grain", "phys"), chk_peak_p(5)]),
    "chem":     dict(frames=400, env={"TILES": "1"}, checks=[chk_molten()]),
    "melt":     dict(frames=400, env={"TILES": "1"}, checks=[chk_melt()]),
    "empty":    dict(frames=100, env={}, checks=[chk_quiet()]),
    # scene.rs SHOWCASE (only reachable once main.rs routes unknown names through Scene::by_name)
    "thermite": dict(frames=400, env={"TILES": "1"}, checks=[chk_molten(), chk_mat_any_drop(STEEL, "steel plate", "chem")]),
    "kitchen":  dict(frames=400, env={}, checks=[chk_peak_burn(0.01, "chem")]),
    "silo":     dict(frames=300, env={}, checks=[chk_peak_p(5, "chem")]),  # dust explosion: matter burning is not in the gas burn stat
    "explosives": dict(frames=300, env={"TILES": "1"}, checks=[chk_peak_p(5, "chem")]),
}
DEFAULT_FRAMES = 200

# ----------------------------------------------------------------------------------------------------------------

RE_FRAME = re.compile(r"frame\s+(\d+)\s+wall\s+([-\d.naNinf]+)s\s+Tmax\s+(\S+)\s+pmax\s+(\S+)\s+vmax\s+(\S+)\s+walls\s+(\d+)\s+bad\s+(\d+)")
RE_STATS = re.compile(r"stats: wall heat\s+(\S+)\s+stress\s+(\S+)\s+burn\s+(\S+)\s+thrust\s+([^,\s]+),(\S+)")
RE_TILES = re.compile(r"tiles \{(.*)\}")
RE_PAIR = re.compile(r"\((\d+),\s*(\d+)\):\s*(\d+)")
RE_FRONT = re.compile(r"front t=\S+ x=(\d+)")
RE_TOTAL = re.compile(r"(\d+) frames x\d+ steps in ([\d.]+)s")
RE_NAN = re.compile(r"(?<![A-Za-z])(NaN|nan|inf|-inf)(?![A-Za-z])")


def fnum(s):
    try:
        return float(s)
    except ValueError:
        return float("nan")


def parse(out):
    r = dict(samples=[], tiles=[], fronts=[], sim_secs=None, nan_lines=[])
    pending = None
    for line in out.splitlines():
        m = RE_STATS.search(line)
        if m:
            pending = dict(heat=fnum(m[1]), stress=fnum(m[2]), burn=fnum(m[3]), tx=fnum(m[4]), ty=fnum(m[5]))
            continue
        m = RE_FRAME.search(line)
        if m:
            s = dict(frame=int(m[1]), tmax=fnum(m[3]), pmax=fnum(m[4]), vmax=fnum(m[5]), walls=int(m[6]), bad=int(m[7]))
            s.update(pending or dict(heat=float("nan"), stress=float("nan"), burn=float("nan"), tx=float("nan"), ty=float("nan")))
            pending = None
            r["samples"].append(s)
            continue
        m = RE_TILES.search(line)
        if m:
            r["tiles"].append({(int(a), int(b)): int(c) for a, b, c in RE_PAIR.findall(m[1])})
            continue
        m = RE_FRONT.search(line)
        if m:
            r["fronts"].append(int(m[1]))
            continue
        m = RE_TOTAL.search(line)
        if m:
            r["sim_secs"] = float(m[2])
        if ("stats" in line or "frame" in line or "w(" in line) and RE_NAN.search(line):
            r["nan_lines"].append(line.strip())
    for s in r["samples"]:
        if any(isinstance(v, float) and not math.isfinite(v) for v in s.values()):
            r["nan_lines"].append(f"frame {s['frame']}: non-finite value in {s}")
    return r


def image_stats(ppm):
    """(mean luminance 0-255, fraction of pixels brighter than 8) from a binary P6 file, subsampled."""
    with open(ppm, "rb") as f:
        data = f.read()
    parts, pos = [], 0
    while len(parts) < 4:
        while data[pos:pos + 1].isspace():
            pos += 1
        if data[pos:pos + 1] == b"#":
            pos = data.index(b"\n", pos) + 1
            continue
        e = pos
        while not data[e:e + 1].isspace():
            e += 1
        parts.append(data[pos:e])
        pos = e
    pos += 1
    w, h = int(parts[1]), int(parts[2])
    px = data[pos:pos + w * h * 3]
    n = tot = lit = 0
    for i in range(0, len(px) - 2, 3 * 97):
        l = (px[i] * 3 + px[i + 1] * 6 + px[i + 2]) / 10
        tot += l
        lit += l > 8
        n += 1
    return (tot / n if n else 0.0, lit / n if n else 0.0, w, h)


def discover(root):
    main = open(os.path.join(root, "src/main.rs"), encoding="utf-8").read()
    names = re.findall(r'"([a-z_0-9]+)"\s*=>\s*scene::Scene::', main)
    showcase = []
    for fn in sorted(os.listdir(os.path.join(root, "src"))):
        if not fn.endswith(".rs"):
            continue
        sc = open(os.path.join(root, "src", fn), encoding="utf-8", errors="replace").read()
        for m in re.finditer(r"pub (?:const|static) SHOWCASE[^=]*=\s*(?:&)?\[(.*?)\n\];", sc, re.S):
            showcase += [n for n in re.findall(r'^\s*\("([a-z_0-9]+)"', m[1], re.M) if n not in showcase]
    by_name = "by_name" in main
    # the fallback arm `_ => turbojet` makes "jet" reachable
    if "turbojet" in main and "jet" not in names:
        names.insert(0, "jet")
    runnable = list(dict.fromkeys(names + (showcase if by_name else [])))
    unreachable = [s for s in showcase if s not in runnable]
    return runnable, unreachable, by_name


def owner_from_text(text):
    owners = []
    for fn, team in OWNER_OF_FILE.items():
        if fn in text and team not in owners:
            owners.append(team)
    return ", ".join(owners) if owners else "phys (bench/runtime)"


def run_scene(root, out, name, cfg, fast, timeout):
    exe = os.path.join(root, "target/release/ignis")
    frames = cfg["frames"] // 2 if fast else cfg["frames"]
    ppm = os.path.join(out, f"{name}.ppm")
    png = os.path.join(out, f"{name}.png")
    for p in (ppm, png):
        if os.path.exists(p):
            os.remove(p)
    env = dict(os.environ)
    env.update(cfg.get("env", {}))
    t0 = time.time()
    try:
        p = subprocess.run([exe, "--bench", str(frames), name, ppm, "0"], cwd=root, env=env,
                           capture_output=True, text=True, timeout=timeout)
        code, so, se = p.returncode, p.stdout, p.stderr
    except subprocess.TimeoutExpired as e:
        code, so, se = "timeout", (e.stdout or b"").decode() if isinstance(e.stdout, bytes) else (e.stdout or ""), "timeout"
    wall = time.time() - t0
    with open(os.path.join(out, f"{name}.log"), "w") as f:
        f.write(f"$ {' '.join(f'{k}={v}' for k, v in cfg.get('env', {}).items())} ignis --bench {frames} {name} {ppm} 0\n")
        f.write(so)
        f.write("\n--- stderr ---\n")
        f.write(se)
    r = parse(so)
    r.update(name=name, frames=frames, code=code, wall=wall, stderr=se, png=None, img=None)
    if os.path.exists(ppm):
        try:
            r["img"] = image_stats(ppm)
        except Exception as e:  # noqa
            r["img"] = None
            r["img_err"] = str(e)
        subprocess.run(["sips", "-s", "format", "png", ppm, "--out", png], capture_output=True)
        subprocess.run(["sips", "-Z", "1200", png], capture_output=True)
        if os.path.exists(png):
            r["png"] = png
            os.remove(ppm)
    return r


def generic_checks(r):
    res = []
    crashed = r["code"] != 0
    panic = "\n".join(l for l in r["stderr"].splitlines() if "panic" in l or "rror" in l or "wgsl" in l)[:600]
    res.append(("process exits cleanly", owner_from_text(r["stderr"]) if crashed else "phys",
                not crashed, f"exit {r['code']}" + (f": {panic}" if crashed and panic else "")))
    res.append(("stats printed", "phys", bool(r["samples"]), f"{len(r['samples'])} samples"))
    bad = max((s["bad"] for s in r["samples"]), default=None)
    res.append(("no bad (NaN) gas cells", "phys", bad == 0, f"max bad {bad}"))
    res.append(("no NaN/inf in printed stats", "phys", not r["nan_lines"],
                (r["nan_lines"][0][:200] if r["nan_lines"] else "none")))
    t, p, v = peak(r, "tmax"), peak(r, "pmax"), peak(r, "vmax")
    sane = t is not None and math.isfinite(t) and 0 < t <= 20 and 0 < p < 500 and v < 50
    res.append(("sane ranges (0<Tmax<=20, 0<pmax<500, vmax<50)", "phys", sane,
                f"peak Tmax {t} pmax {p} vmax {v}"))
    img = r.get("img")
    ok_img = bool(img and img[1] > 0.02)
    res.append(("Fire-view screenshot rendered, not blank", "vfx", ok_img,
                (f"mean lum {img[0]:.1f}, lit {img[1]*100:.1f}%, {img[2]}x{img[3]}" if img else r.get("img_err", "no image"))))
    return res


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--root", default=os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
    ap.add_argument("--out", required=True)
    ap.add_argument("--build-status", default="skipped")
    ap.add_argument("--build-attempts", default="0")
    ap.add_argument("--build-secs", default="0")
    ap.add_argument("--build-log", default="")
    a = ap.parse_args()
    root, out = a.root, a.out
    os.makedirs(out, exist_ok=True)
    fast = os.environ.get("QA_FAST") == "1"
    timeout = int(os.environ.get("QA_TIMEOUT", "240"))
    only = os.environ.get("QA_SCENES", "").split()
    report = os.environ.get("QA_REPORT", os.path.join(root, "tools/QA_REPORT.md"))

    runnable, unreachable, by_name = discover(root)
    exe = os.path.join(root, "target/release/ignis")
    have_exe = os.path.exists(exe)
    exe_age = time.time() - os.path.getmtime(exe) if have_exe else None

    build_errs = []
    if a.build_status == "failed" and a.build_log and os.path.exists(a.build_log):
        log = open(a.build_log, errors="replace").read().splitlines()
        for i, l in enumerate(log):
            if l.startswith("error"):
                loc = next((x.strip() for x in log[i + 1:i + 4] if "-->" in x), "")
                build_errs.append((l.strip(), loc, owner_from_text(loc)))

    results = []
    t_all = time.time()
    if have_exe:
        for name in runnable:
            if only and name not in only:
                continue
            cfg = CHECKS.get(name, dict(frames=DEFAULT_FRAMES, env={}, checks=[]))
            print(f"[qa] {name}: {cfg['frames'] // 2 if fast else cfg['frames']} frames ...", flush=True)
            r = run_scene(root, out, name, cfg, fast, timeout)
            checks = generic_checks(r)
            for cname, owner, fn in cfg["checks"]:
                try:
                    ok, obs = fn(r)
                except Exception as e:  # noqa
                    ok, obs = None, f"check error: {e}"
                checks.append((cname, owner, bool(ok), obs))
            r["checks"] = checks
            nf = sum(1 for c in checks if not c[2])
            print(f"[qa]   {'PASS' if nf == 0 else f'FAIL ({nf})'} in {r['wall']:.1f}s", flush=True)
            for c in checks:
                if not c[2]:
                    print(f"[qa]     FAIL {c[0]} [{c[1]}]: {c[3]}", flush=True)
            results.append(r)
    total = time.time() - t_all
    skipped = [n for n in CHECKS if n not in runnable and (not only or n in only)]

    # ---------------------------------------------------------------- report
    npass = sum(1 for r in results for c in r["checks"] if c[2])
    nfail = sum(1 for r in results for c in r["checks"] if not c[2])
    L = []
    L.append("# Ignis QA report\n")
    L.append(f"Generated {time.strftime('%Y-%m-%d %H:%M:%S')} by `tools/qa.sh`" + (" (QA_FAST=1: halved frames)" if fast else "") + ".\n")
    overall = "PASS" if nfail == 0 and a.build_status != "failed" and results else "FAIL"
    L.append(f"**Overall: {overall}** - {npass} checks passed, {nfail} failed, {len(results)} scenes run, "
             f"{len(skipped)} configured scenes skipped. Scene time {total:.1f}s.\n")
    L.append("## Build\n")
    L.append(f"- status: **{a.build_status}** after {a.build_attempts} attempt(s), last attempt {a.build_secs}s")
    if have_exe:
        L.append(f"- binary: `target/release/ignis`, built {exe_age/60:.1f} min before the runs"
                 + (" (**STALE: build failed, results are for the previous binary**)" if a.build_status == "failed" else ""))
    else:
        L.append("- binary: **missing**, no scenes run")
    for e, loc, ow in build_errs[:15]:
        L.append(f"- `{e}` at `{loc}` -> owner **{ow}**")
    L.append("")
    L.append("## Summary\n")
    L.append("| scene | frames | result | failed checks | wall time | sim time | image |")
    L.append("|---|---|---|---|---|---|---|")
    for r in results:
        f = [c for c in r["checks"] if not c[2]]
        L.append(f"| {r['name']} | {r['frames']} | {'PASS' if not f else 'FAIL'} | {', '.join(c[0] for c in f) or '-'} | "
                 f"{r['wall']:.1f}s | {('%.2fs' % r['sim_secs']) if r['sim_secs'] else '-'} | "
                 f"{('`' + r['png'] + '`') if r['png'] else 'none'} |")
    for n in skipped:
        L.append(f"| {n} | - | SKIP | not reachable from `--bench` (main.rs) | - | - | - |")
    L.append("")
    if unreachable:
        L.append(f"Scenes defined in scene.rs `SHOWCASE` but not reachable from `--bench` "
                 f"(main.rs {'has' if by_name else 'has no'} `Scene::by_name` fallback; unknown names silently run `jet`): "
                 + ", ".join(f"`{u}`" for u in unreachable) + " -> owner **phys** (main.rs bench).\n")
    fails = [(r["name"], c) for r in results for c in r["checks"] if not c[2]]
    if fails:
        L.append("## Failures by owning team\n")
        teams = {}
        for n, c in fails:
            teams.setdefault(c[1], []).append((n, c))
        for t, items in sorted(teams.items()):
            L.append(f"### {t}\n")
            for n, c in items:
                L.append(f"- **{n}**: {c[0]} - observed: {c[3]}")
            L.append("")
    L.append("## Per-scene details\n")
    for r in results:
        L.append(f"### {r['name']}\n")
        L.append(f"Log: `{os.path.join(out, r['name'] + '.log')}`" + (f"  \nImage: `{r['png']}`" if r["png"] else ""))
        L.append("")
        L.append("| check | owner | result | observed |")
        L.append("|---|---|---|---|")
        for c in r["checks"]:
            L.append(f"| {c[0]} | {c[1]} | {'PASS' if c[2] else '**FAIL**'} | {str(c[3]).replace('|', '/')} |")
        s = r["samples"]
        if s:
            fs, ls = s[0], s[-1]
            L.append("")
            L.append(f"First/last sample: frame {fs['frame']}->{ls['frame']}, walls {fs['walls']}->{ls['walls']}, "
                     f"Tmax {ls['tmax']:.2f} (peak {peak(r, 'tmax'):.2f}), pmax {ls['pmax']:.2f} (peak {peak(r, 'pmax'):.2f}), "
                     f"vmax {ls['vmax']:.2f}, burn {ls['burn']:.3f} (peak {peak(r, 'burn'):.3f}), "
                     f"thrust {ls['tx']:.2f},{ls['ty']:.2f}")
        if r["tiles"]:
            L.append(f"  \nTiles first: `{dict(sorted(r['tiles'][0].items()))}`  \nTiles last: `{dict(sorted(r['tiles'][-1].items()))}`")
        if r["fronts"]:
            L.append(f"  \nFlame front x: {r['fronts']}")
        L.append("")
    with open(report, "w") as f:
        f.write("\n".join(L) + "\n")
    print(f"[qa] {overall}: {npass} pass, {nfail} fail. Report: {report}")
    sys.exit(0 if overall == "PASS" else 1)


if __name__ == "__main__":
    main()
