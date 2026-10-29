#!/usr/bin/env python3
"""Ignis brand asset generator.

Writes every SVG source for the Ignis identity (flame mark, wordmark, lockup,
macOS app icon, small-size icon, title art) into assets/branding/svg/.
Pure standard library; rasterising is done by ../build.sh with rsvg-convert.
Deterministic: all "random" embers use a fixed seed.
"""
import math
import os
import random
import sys

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "svg")

# ---------------------------------------------------------------- palette
C = {
    "soot": "#0B0A0C",       # deepest background
    "char": "#141519",       # panel / app background (matches app PANEL_BG-ish)
    "coal": "#2A1409",       # warm dark
    "ember": "#C2350E",      # deep red-orange
    "flame": "#FF8C28",      # primary accent (== app ACCENT 255,140,40)
    "gold": "#FFC83C",       # secondary accent (== app GOLD 255,200,60)
    "core": "#FFF3D2",       # white-hot core
    "blue": "#3C8CFF",       # premixed blue flame base
    "blue_deep": "#1F4FD8",
    "ash": "#8C919E",        # muted text (== app MUTED)
}

# ---------------------------------------------------------------- flame mark
# Unit flame: base centre at (0,0), tip near (0.06,-1). Width ~1.05.
def flame_path(s=1.0, dx=0.0, dy=0.0, lean=0.06, belly=0.52, tip=-1.0):
    def p(x, y):
        return f"{dx + x * s:.2f},{dy + y * s:.2f}"
    b = belly
    return (
        f"M{p(lean, tip)} "
        f"C{p(lean + 0.10, tip + 0.30)} {p(b, -0.52)} {p(b, -0.10)} "
        f"C{p(b, 0.18)} {p(b * 0.55, 0.34)} {p(0, 0.34)} "
        f"C{p(-b * 0.55, 0.34)} {p(-b, 0.18)} {p(-b, -0.10)} "
        f"C{p(-b, -0.34)} {p(-b * 0.70, -0.46)} {p(-b * 0.50, -0.62)} "
        # little flicking tongue on the left shoulder
        f"C{p(-b * 0.46, -0.46)} {p(-b * 0.20, -0.40)} {p(-b * 0.12, -0.50)} "
        f"C{p(-b * 0.02, -0.66)} {p(lean - 0.06, tip + 0.22)} {p(lean, tip)} Z"
    )


def flame_group(cx, bottom, h, gid="f", detail=True):
    """Layered flame whose lowest point sits at y=bottom; tip at bottom-1.34h."""
    by = bottom - 0.34 * h
    defs = f"""
    <linearGradient id="{gid}Outer" x1="0" y1="1" x2="0" y2="0">
      <stop offset="0" stop-color="{C['ember']}"/>
      <stop offset="0.45" stop-color="{C['flame']}"/>
      <stop offset="1" stop-color="#FF5A1E"/>
    </linearGradient>
    <linearGradient id="{gid}Mid" x1="0" y1="1" x2="0" y2="0">
      <stop offset="0" stop-color="#FFB02E"/>
      <stop offset="0.6" stop-color="{C['gold']}"/>
      <stop offset="1" stop-color="#FF9A2E"/>
    </linearGradient>
    <radialGradient id="{gid}Core" cx="0.5" cy="0.72" r="0.62">
      <stop offset="0" stop-color="#FFFFFF"/>
      <stop offset="0.45" stop-color="{C['core']}"/>
      <stop offset="1" stop-color="#FFE08A"/>
    </radialGradient>
    <linearGradient id="{gid}Blue" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="{C['blue']}" stop-opacity="0"/>
      <stop offset="0.62" stop-color="{C['blue']}" stop-opacity="0"/>
      <stop offset="0.88" stop-color="{C['blue']}" stop-opacity="0.55"/>
      <stop offset="1" stop-color="{C['blue_deep']}" stop-opacity="0.85"/>
    </linearGradient>"""
    outer = flame_path(h, cx, by, lean=0.07, belly=0.52)
    mid = flame_path(h * 0.74, cx + h * 0.015, by + h * 0.005, lean=0.05, belly=0.50)
    core = flame_path(h * 0.46, cx + h * 0.02, by + h * 0.01, lean=0.03, belly=0.46)
    # blue base: a shallow crescent hugging the bottom of the core
    bw = h * 0.17
    bt = by + h * 0.17
    blue = (f"M{cx - bw:.2f},{bt:.2f} C{cx - bw:.2f},{bt + h*0.12:.2f} {cx + bw:.2f},{bt + h*0.12:.2f} {cx + bw:.2f},{bt:.2f} "
            f"C{cx + bw*0.6:.2f},{bt + h*0.035:.2f} {cx - bw*0.6:.2f},{bt + h*0.035:.2f} {cx - bw:.2f},{bt:.2f} Z")
    body = f"""
    <path d="{outer}" fill="url(#{gid}Outer)"/>
    <path d="{mid}" fill="url(#{gid}Mid)"/>
    <path d="{core}" fill="url(#{gid}Core)"/>"""
    if detail:
        body += f'\n    <path d="{core}" fill="url(#{gid}Blue)"/>'
    return defs, body


# ---------------------------------------------------------------- wordmark
# Geometric custom letters on a 100-unit cap height, stroke weight W.
W = 17.0


def wordmark_shapes(x0=0.0, y0=0.0, scale=1.0, fill="url(#wmGrad)", flame_id="wmf"):
    """Return (defs, svg body, total width) for the IGNIS wordmark.

    The second I is a lit match: a shorter stem with a flame on top.
    """
    parts = []
    hw = W / 2
    x = 0.0
    gap = 24.0

    def rect(xx, yy, ww, hh):
        parts.append(f'<rect x="{xx:.2f}" y="{yy:.2f}" width="{ww:.2f}" height="{hh:.2f}"/>')

    def stroke(d):
        parts.append(f'<path d="{d}" fill="none" stroke-width="{W}" stroke-linecap="butt"/>')

    # I
    rect(x, 0, W, 100)
    x += W + gap
    # G : centre-line circle r, open at upper right, bar into the centre
    r = 50 - hw
    cx = x + r + hw
    a0 = math.radians(-42)
    sx, sy = cx + r * math.cos(a0), 50 + r * math.sin(a0)
    stroke(f"M{sx:.2f},{sy:.2f} A{r:.2f},{r:.2f} 0 1 0 {cx + r:.2f},50.00")
    rect(cx + 2, 50 - hw, r - 2 + hw, W)            # cross bar
    rect(cx + r - hw, 50 - hw, W, 22)               # short spur down
    x = cx + r + hw + gap
    # N
    nw = 82
    rect(x, 0, W, 100)
    rect(x + nw - W, 0, W, 100)
    d = W * 1.18
    parts.append(f'<polygon points="{x:.2f},0 {x + d:.2f},0 {x + nw:.2f},100 {x + nw - d:.2f},100"/>')
    x += nw + gap
    # I (match): stem from 34 to 100, flame above
    ix = x
    rect(x, 34, W, 66)
    x += W + gap
    # S : two stacked ellipses on the centre line
    rx, ry = 29.0, 25.0 - hw / 2
    scx = x + rx + hw
    tcy = hw + ry
    bcy = 100 - hw - ry
    mid_y = (tcy + ry + bcy - ry) / 2
    a = math.radians(-28)
    p0 = (scx + rx * math.cos(a), tcy + ry * math.sin(a))
    b = math.radians(152)
    p1 = (scx + rx * math.cos(b), bcy + ry * math.sin(b))
    stroke(f"M{p0[0]:.2f},{p0[1]:.2f} A{rx:.2f},{ry:.2f} 0 1 0 {scx:.2f},{tcy + ry:.2f} "
           f"L{scx:.2f},{bcy - ry:.2f} A{rx:.2f},{ry:.2f} 0 1 1 {p1[0]:.2f},{p1[1]:.2f}")
    x = scx + rx + hw
    total = x

    fdefs, fbody = flame_group(ix + W / 2, 36, 30, gid=flame_id, detail=False)
    defs = f"""
    <linearGradient id="wmGrad" gradientUnits="userSpaceOnUse" x1="0" y1="0" x2="0" y2="100">
      <stop offset="0" stop-color="#FFE7A8"/>
      <stop offset="0.45" stop-color="{C['gold']}"/>
      <stop offset="1" stop-color="{C['flame']}"/>
    </linearGradient>{fdefs}"""
    body = (f'<g transform="translate({x0:.2f},{y0:.2f}) scale({scale})">'
            f'<g fill="{fill}" stroke="{fill}">' + "".join(parts) + "</g>"
            f"{fbody}</g>")
    return defs, body, total


# ---------------------------------------------------------------- helpers
def svg(w, h, defs, body, bg=None):
    bgr = f'<rect width="{w}" height="{h}" fill="{bg}"/>' if bg else ""
    return (f'<?xml version="1.0" encoding="UTF-8"?>\n'
            f'<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">\n'
            f"<defs>{defs}\n</defs>\n{bgr}{body}\n</svg>\n")


def write(name, text):
    os.makedirs(OUT, exist_ok=True)
    with open(os.path.join(OUT, name), "w") as f:
        f.write(text)
    print("wrote", name)


def squircle_path(x, y, s, n=5.0, steps=256):
    """Superellipse (Apple-style continuous corner) approximated as a polygon."""
    cx, cy, r = x + s / 2, y + s / 2, s / 2
    pts = []
    for i in range(steps):
        t = 2 * math.pi * i / steps
        c, si = math.cos(t), math.sin(t)
        px = cx + r * math.copysign(abs(c) ** (2 / n), c)
        py = cy + r * math.copysign(abs(si) ** (2 / n), si)
        pts.append(f"{px:.2f},{py:.2f}")
    return "M" + " L".join(pts) + " Z"


EMBER_COLS = ["#FF4A12", "#FF7A1E", "#FFA62E", "#FFD24A", "#E2380C", "#9E2A0A"]
BED_COLS = ["#FFE08A", "#FFB02E", "#FF7A1E", "#E2450F", "#A52A0A", "#5A1A0A", "#2B1A14"]


def bed_col(heat):
    heat = max(0.0, min(1.0, heat))
    return BED_COLS[min(6, int((1.0 - heat) * 7))]


# ---------------------------------------------------------------- app icon
def app_icon(small=False):
    S = 1024
    body_x, body_s = 100, 824
    sq = squircle_path(body_x, body_x, body_s)
    rnd = random.Random(7)
    fdefs, fbody = flame_group(512, 730, 400, gid="ic", detail=True)
    defs = f"""
    <radialGradient id="bg" cx="0.5" cy="0.78" r="0.85">
      <stop offset="0" stop-color="#4A1C08"/>
      <stop offset="0.38" stop-color="#22110B"/>
      <stop offset="1" stop-color="{C['soot']}"/>
    </radialGradient>
    <radialGradient id="glow" cx="0.5" cy="0.5" r="0.5">
      <stop offset="0" stop-color="{C['flame']}" stop-opacity="0.55"/>
      <stop offset="0.5" stop-color="{C['ember']}" stop-opacity="0.18"/>
      <stop offset="1" stop-color="{C['ember']}" stop-opacity="0"/>
    </radialGradient>
    <linearGradient id="rim" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#FFFFFF" stop-opacity="0.22"/>
      <stop offset="0.5" stop-color="#FFFFFF" stop-opacity="0.03"/>
      <stop offset="1" stop-color="#FFB060" stop-opacity="0.12"/>
    </linearGradient>
    <clipPath id="clip"><path d="{sq}"/></clipPath>
    <filter id="shadow" x="-20%" y="-20%" width="140%" height="140%">
      <feGaussianBlur stdDeviation="14"/>
    </filter>
    <filter id="soft" x="-50%" y="-50%" width="200%" height="200%">
      <feGaussianBlur stdDeviation="6"/>
    </filter>{fdefs}"""
    b = []
    # macOS-style drop shadow inside the 1024 canvas
    b.append(f'<path d="{sq}" fill="#000" opacity="0.45" transform="translate(0,14)" filter="url(#shadow)"/>')
    b.append(f'<path d="{sq}" fill="url(#bg)"/>')
    b.append('<g clip-path="url(#clip)">')
    if not small:
        # simulation cell grid, fading toward the edges
        g = []
        step = 32
        for i in range(body_x, body_x + body_s + 1, step):
            g.append(f'M{i},{body_x}V{body_x + body_s}M{body_x},{i}H{body_x + body_s}')
        b.append(f'<path d="{"".join(g)}" stroke="#FF9A50" stroke-opacity="0.055" stroke-width="2" fill="none"/>')
    b.append('<ellipse cx="512" cy="560" rx="380" ry="360" fill="url(#glow)"/>')
    # burning powder bed: a row of glowing cells (falling-sand nod)
    cell = 32 if not small else 64
    ybed = 724
    for row in range(3 if not small else 2):
        y = ybed + row * cell
        for x in range(body_x, body_x + body_s, cell):
            d = abs((x + cell / 2) - 512) / 412.0
            heat = max(0.0, 1.0 - d * (1.15 + 0.25 * row)) + rnd.uniform(-0.12, 0.12)
            b.append(f'<rect x="{x + 2}" y="{y + 2}" width="{cell - 4}" height="{cell - 4}" rx="4" fill="{bed_col(heat)}"/>')
    # soft halo under the flame + the flame
    b.append(f'<ellipse cx="512" cy="712" rx="210" ry="40" fill="{C["gold"]}" opacity="0.35" filter="url(#soft)"/>')
    b.append(fbody)
    if not small:
        # rising ember particles (cells breaking free)
        for _ in range(18):
            ex = rnd.uniform(250, 780)
            ey = rnd.uniform(170, 560)
            if abs(ex - 512) < 150 and ey > 250:
                continue
            sz = rnd.choice([8, 10, 12, 14])
            col = rnd.choice(EMBER_COLS[:4])
            op = rnd.uniform(0.45, 0.95)
            b.append(f'<rect x="{ex:.1f}" y="{ey:.1f}" width="{sz}" height="{sz}" rx="2" fill="{col}" opacity="{op:.2f}"/>')
    b.append("</g>")
    # glassy rim
    b.append(f'<path d="{sq}" fill="none" stroke="url(#rim)" stroke-width="6"/>')
    return svg(S, S, defs, "\n".join(b))


# ---------------------------------------------------------------- mark
def mark_svg():
    S = 512
    fdefs, fbody = flame_group(256, 456, 300, gid="mk", detail=True)
    return svg(S, S, fdefs, fbody)


def wordmark_svg():
    defs, body, w = wordmark_shapes(10, 30, 1.0)
    W_ = int(math.ceil(w + 20))
    return svg(W_, 140, defs, body), W_


def lockup_svg():
    fdefs, fbody = flame_group(80, 150, 105, gid="lk", detail=True)
    wdefs, wbody, w = wordmark_shapes(160, 30, 1.0)
    W_ = int(math.ceil(160 + w + 24))
    sub = (f'<text x="{160 + 2}" y="160" font-family="Helvetica Neue, Helvetica, Arial, sans-serif" '
           f'font-size="17" letter-spacing="7.2" fill="{C["ash"]}">COMBUSTION SANDBOX</text>')
    return svg(W_, 180, fdefs + wdefs, fbody + wbody + sub), W_


# ---------------------------------------------------------------- title art
def title_svg(with_text=True):
    Wd, Hd = 1920, 720
    rnd = random.Random(42)
    defs = f"""
    <linearGradient id="sky" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#07070A"/>
      <stop offset="0.6" stop-color="#120B0A"/>
      <stop offset="1" stop-color="#2E1208"/>
    </linearGradient>
    <radialGradient id="heat" cx="0.5" cy="1.0" r="0.75">
      <stop offset="0" stop-color="{C['flame']}" stop-opacity="0.55"/>
      <stop offset="0.35" stop-color="{C['ember']}" stop-opacity="0.22"/>
      <stop offset="1" stop-color="{C['ember']}" stop-opacity="0"/>
    </radialGradient>
    <radialGradient id="vig" cx="0.5" cy="0.5" r="0.75">
      <stop offset="0.6" stop-color="#000" stop-opacity="0"/>
      <stop offset="1" stop-color="#000" stop-opacity="0.75"/>
    </radialGradient>
    <filter id="blur40" x="-50%" y="-50%" width="200%" height="200%"><feGaussianBlur stdDeviation="40"/></filter>
    <filter id="blur14" x="-50%" y="-50%" width="200%" height="200%"><feGaussianBlur stdDeviation="14"/></filter>
    <filter id="blur4" x="-50%" y="-50%" width="200%" height="200%"><feGaussianBlur stdDeviation="3"/></filter>"""
    b = [f'<rect width="{Wd}" height="{Hd}" fill="url(#sky)"/>']
    # faint simulation grid
    g = []
    for i in range(0, Wd + 1, 40):
        g.append(f"M{i},0V{Hd}")
    for j in range(0, Hd + 1, 40):
        g.append(f"M0,{j}H{Wd}")
    b.append(f'<path d="{"".join(g)}" stroke="#FF9A50" stroke-opacity="0.035" stroke-width="1" fill="none"/>')
    # smoke plumes
    for i in range(9):
        x = rnd.uniform(120, Wd - 120)
        y = rnd.uniform(60, 420)
        b.append(f'<ellipse cx="{x:.0f}" cy="{y:.0f}" rx="{rnd.uniform(160, 340):.0f}" ry="{rnd.uniform(60, 140):.0f}" '
                 f'fill="#3A3436" opacity="{rnd.uniform(0.10, 0.22):.2f}" filter="url(#blur40)"/>')
    b.append(f'<rect width="{Wd}" height="{Hd}" fill="url(#heat)"/>')
    # a field of flames rising from the bed
    flames = []
    for i in range(34):
        fx = rnd.uniform(-40, Wd + 40)
        d = abs(fx - Wd / 2) / (Wd / 2)
        fh = rnd.uniform(45, 105) * (0.75 + 0.6 * d)
        flames.append((fh, fx))
    flames.sort()
    for k, (fh, fx) in enumerate(flames):
        gid = f"tf{k}"
        dd, bb = flame_group(fx, Hd - 70, fh, gid=gid, detail=fh > 90)
        b.append(f"<defs>{dd}</defs><g opacity='{0.6 + 0.4 * min(1, fh / 150):.2f}'>{bb}</g>")
    # glowing bed of burning cells
    cell = 24
    for row in range(4):
        y = Hd - 76 + row * cell
        for x in range(0, Wd, cell):
            d = abs(x - Wd / 2) / (Wd / 2)
            heat = 1.0 - 0.75 * d - 0.2 * row + rnd.uniform(-0.2, 0.15)
            b.append(f'<rect x="{x + 1}" y="{y + 1}" width="{cell - 2}" height="{cell - 2}" rx="3" fill="{bed_col(heat)}"/>')
    # sparks
    for _ in range(140):
        x = rnd.uniform(0, Wd)
        y = rnd.uniform(40, Hd - 120)
        sz = rnd.choice([3, 4, 5, 6, 8])
        col = rnd.choice(EMBER_COLS[:4])
        b.append(f'<rect x="{x:.0f}" y="{y:.0f}" width="{sz}" height="{sz}" rx="1" fill="{col}" opacity="{rnd.uniform(0.25, 0.9):.2f}"/>')
    if with_text:
        _, _, w = wordmark_shapes(0, 0, 1.0)
        sc = 2.6
        x0 = (Wd - w * sc) / 2
        y0 = 120
        _, glow, _ = wordmark_shapes(x0, y0, sc, fill="#FF7A1E", flame_id="wg")
        _, word, _ = wordmark_shapes(x0, y0, sc, flame_id="ww")
        d2, _, _ = wordmark_shapes(0, 0, 1.0, flame_id="wg")
        d3, _, _ = wordmark_shapes(0, 0, 1.0, flame_id="ww")
        b.append(f"<defs>{d2}{d3}</defs>")
        b.append(f'<g filter="url(#blur14)" opacity="0.85">{glow}</g>')
        b.append(word)
        b.append(f'<text x="{Wd / 2}" y="{y0 + 100 * sc + 78}" text-anchor="middle" '
                 f'font-family="Helvetica Neue, Helvetica, Arial, sans-serif" font-size="30" '
                 f'letter-spacing="14" fill="#E8C9A8" opacity="0.85">REAL COMBUSTION  ·  REAL CHEMISTRY</text>')
    b.append(f'<rect width="{Wd}" height="{Hd}" fill="url(#vig)"/>')
    return svg(Wd, Hd, defs, "\n".join(b))


def main():
    write("app_icon.svg", app_icon(small=False))
    write("app_icon_small.svg", app_icon(small=True))
    write("mark.svg", mark_svg())
    s, _ = wordmark_svg()
    write("wordmark.svg", s)
    s, _ = lockup_svg()
    write("lockup.svg", s)
    write("title_art.svg", title_svg(True))
    write("title_bg.svg", title_svg(False))
    return 0


if __name__ == "__main__":
    sys.exit(main())
