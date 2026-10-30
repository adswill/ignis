// Final composite: full-screen triangle at screen resolution. Everything is read from the fx texture array
// (cell-resolution HDR layers packed by fx.wgsl, see the layer list there) plus a tileable noise texture.
//
// Fire view (0, and 6 as an alias): cinematic, "realistic but stylised":
//   * gas fields (emission, smoke/gas clouds, soot, flame front) B-spline (bicubic) upsampled -> no cell blocks
//   * flow-advected two-phase fBm noise adds sub-cell flame tongues and billowing smoke detail, only where
//     there is fire/smoke
//   * smoke lit by a dim sky light from above + the fire's own light, self-shadowed by smoke toward each light
//   * heat haze (refraction above hot gas) and blast-wave refraction (pressure gradient) warp the whole image
//   * walls: crisp cells, material albedo, edge bevels lit by sky and fire light, incandescent glow
//   * powders: crisp shaded grains; liquids: smooth (filtered-mask) surfaces with a highlight
//   * bloom (tight + wide), ACES filmic tone map, vignette, faint film grain
// Analytic views: B-spline-smoothed colour fields under crisp solids, display-space shoulder.

struct Params {
    w: u32, h: u32, nops: u32, view: u32,
    dt: f32, p_fuel: f32, fan_pow: f32, time: f32,
    ign_on: u32, frame: u32, srgb: u32, _p1: u32,
    p_o2: f32, p_air: f32, cam_x: f32, cam_y: f32,
    cam_w: f32, cam_h: f32, wall_dt: f32, _p3: f32,
}
struct FxParams { dt: f32, ph0: f32, ph1: f32, reset: u32, src: u32, stime: f32, _b: u32, _c: u32 }

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var<uniform> FP: FxParams;
@group(0) @binding(2) var TX: texture_2d_array<f32>;
@group(0) @binding(3) var NZT: texture_2d<f32>;
@group(0) @binding(4) var SL: sampler; // linear, clamp
@group(0) @binding(5) var SR: sampler; // linear, repeat
@group(0) @binding(6) var DTX: texture_2d<f32>; // detail.wgsl up-res field: r flame lum, g soot op, b cloud op, a haze
@group(0) @binding(7) var CTX: texture_2d<f32>; // detail.wgsl chemistry tracers: rgb emitter colour x conc, a white oxide smoke

const LA: i32 = 0; // gas emission, a = haze
const LB: i32 = 1; // cloud premult albedo, a = opacity
const LC: i32 = 2; // solid albedo, a = code
const LD: i32 = 3; // advected uv
const LE: i32 = 4; // solid emission, a = liquid mask
const LF: i32 = 5; // soot, burn, pdev, remaining
const LG: i32 = 6; // tight bloom, a = light dir x
const LH: i32 = 7; // wide bloom, a = light dir y
const LM: i32 = 8; // coverage masks: r liquid, g powder, b wall

struct VOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> }

@vertex
fn vs(@builtin(vertex_index) vi: u32) -> VOut {
    let p = vec2<f32>(f32((vi << 1u) & 2u), f32(vi & 2u));
    var o: VOut;
    o.pos = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2<f32>(p.x, 1.0 - p.y);
    return o;
}

fn wh() -> vec2<f32> { return vec2<f32>(f32(P.w), f32(P.h)); }

fn ld(layer: i32, c: vec2<i32>) -> vec4<f32> {
    let cc = clamp(c, vec2<i32>(0), vec2<i32>(i32(P.w) - 1, i32(P.h) - 1));
    return textureLoad(TX, cc, layer, 0);
}
fn bil(layer: i32, cf: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(TX, SL, cf / wh(), layer, 0.0);
}
// cubic B-spline filter from 4 bilinear taps (C2-smooth, slightly soft: ideal for gas fields)
fn bsp(layer: i32, cf: vec2<f32>) -> vec4<f32> {
    let p = cf - 0.5;
    let i = floor(p);
    let f = p - i;
    let f2 = f * f;
    let f3 = f2 * f;
    let w0 = (1.0 - 3.0 * f + 3.0 * f2 - f3) / 6.0;
    let w1 = (4.0 - 6.0 * f2 + 3.0 * f3) / 6.0;
    let w2 = (1.0 + 3.0 * f + 3.0 * f2 - 3.0 * f3) / 6.0;
    let w3 = f3 / 6.0;
    let g0 = w0 + w1;
    let g1 = w2 + w3;
    let c0 = i - 0.5 + w1 / g0;
    let c1 = i + 1.5 + w3 / g1;
    let r = wh();
    let a = textureSampleLevel(TX, SL, vec2<f32>(c0.x, c0.y) / r, layer, 0.0);
    let b = textureSampleLevel(TX, SL, vec2<f32>(c1.x, c0.y) / r, layer, 0.0);
    let c = textureSampleLevel(TX, SL, vec2<f32>(c0.x, c1.y) / r, layer, 0.0);
    let d = textureSampleLevel(TX, SL, vec2<f32>(c1.x, c1.y) / r, layer, 0.0);
    return (a * g0.x + b * g1.x) * g0.y + (c * g0.x + d * g1.x) * g1.y;
}
// noise texture: 64 cells per tile at scale 1, band-limited mips. NLOD = log2(noise texels per screen pixel at
// unit scale), set once per fragment from the zoom; nzs(p, s) samples p * s with the matching level.
var<private> NLOD: f32 = 0.0;
// extra LOD for flow-advected lookups: where the advected coordinates are stretched/sheared, the noise is
// compressed into stripes finer than a pixel (moire / cross-hatch); pick the band-limited mip that matches.
var<private> FBIAS: f32 = 0.0;
fn nzs(p: vec2<f32>, s: f32) -> vec4<f32> {
    return textureSampleLevel(NZT, SR, p * s / 64.0, max(NLOD + log2(s), 0.0));
}
fn nzsb(p: vec2<f32>, s: f32, b: f32) -> vec4<f32> {
    return textureSampleLevel(NZT, SR, p * s / 64.0, clamp(NLOD + log2(s) + b, 0.0, 3.0));
}
fn nz(p: vec2<f32>) -> vec4<f32> {
    return nzs(p, 1.0);
}

fn hash(x: u32, y: u32, f: u32) -> f32 {
    var h = x * 0x9E3779B1u ^ y * 0x85EBCA77u ^ f * 0xC2B2AE3Du;
    h ^= h >> 15u;
    h *= 0x2C1B3C6Du;
    h ^= h >> 12u;
    h *= 0x297A2D39u;
    h ^= h >> 15u;
    return f32(h & 0xFFFFu) / 65535.0;
}

fn lum(c: vec3<f32>) -> f32 { return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722)); }
fn to_lin(c: vec3<f32>) -> vec3<f32> { return pow(max(c, vec3<f32>(0.0)), vec3<f32>(2.2)); }


// perceptual "inferno" colormap (must match vis.wgsl)
fn inferno(t0: f32) -> vec3<f32> {
    let t = clamp(t0, 0.0, 1.0);
    let c0 = vec3<f32>(0.0002189403691192265, 0.001651004631001012, -0.01948089843709184);
    let c1 = vec3<f32>(0.1065134194856116, 0.5639564367884091, 3.932712388889277);
    let c2 = vec3<f32>(11.60249308247187, -3.972853965665698, -15.9423941062914);
    let c3 = vec3<f32>(-41.70399613139459, 17.43639888205313, 44.35414519872813);
    let c4 = vec3<f32>(77.162935699427, -33.40235894210092, -81.80730925738993);
    let c5 = vec3<f32>(-71.31942824499214, 32.62606426397723, 73.20951985803202);
    let c6 = vec3<f32>(25.13112622477341, -12.24266895238567, -23.07032500287172);
    return clamp(c0 + t * (c1 + t * (c2 + t * (c3 + t * (c4 + t * (c5 + t * c6))))), vec3<f32>(0.0), vec3<f32>(1.0));
}
fn pres_col(p: f32) -> vec3<f32> {
    let d = log2(max(p, 0.05));
    if (d < 0.0) { return mix(vec3<f32>(0.02, 0.02, 0.03), vec3<f32>(0.15, 0.45, 1.0), clamp(-d / 2.0, 0.0, 1.0)); }
    return inferno(d / 4.2) + vec3<f32>(0.02, 0.02, 0.03) * (1.0 - clamp(d, 0.0, 1.0));
}
// colour-bar legend for the analytic field views (pressure: 0.25..18 atm log scale, ticks at 0.25 1 2 4 8 16 atm and
// 1 atm marked brighter; flow: 0..5 cells/time, 5 ticks). Returns (rgb, coverage). Bar: right edge of the viewport.
fn legend(uv: vec2<f32>, view: u32) -> vec4<f32> {
    if (view != 1u && view != 3u) { return vec4<f32>(0.0); }
    let x0 = 0.962; let x1 = 0.978; let y0 = 0.60; let y1 = 0.94;
    let tick_x0 = x0 - 0.010;
    if (uv.x < tick_x0 - 0.002 || uv.x > x1 + 0.004 || uv.y < y0 - 0.012 || uv.y > y1 + 0.012) { return vec4<f32>(0.0); }
    let v = clamp((y1 - uv.y) / (y1 - y0), 0.0, 1.0);
    var col = vec3<f32>(0.0);
    var a = 0.0;
    // backing plate
    a = 0.55; col = vec3<f32>(0.02);
    if (uv.x >= x0 && uv.x <= x1 && uv.y >= y0 && uv.y <= y1) {
        if (view == 1u) { col = pres_col(exp2(-2.0 + v * 6.2)); } else { col = inferno(v); }
        a = 1.0;
    }
    // frame + ticks
    let fr = 0.0012;
    if ((abs(uv.x - x0) < fr || abs(uv.x - x1) < fr) && uv.y >= y0 && uv.y <= y1) { col = vec3<f32>(0.75); a = 1.0; }
    if ((abs(uv.y - y0) < fr || abs(uv.y - y1) < fr) && uv.x >= x0 && uv.x <= x1) { col = vec3<f32>(0.75); a = 1.0; }
    if (uv.x >= tick_x0 && uv.x < x0) {
        var nt = 5.0;
        if (view == 1u) { nt = 6.2; }
        for (var k = 0; k <= 6; k++) {
            var tv = f32(k) / 4.0;
            if (view == 1u) { tv = f32(k) / 6.2; }
            if (tv <= 1.0 && abs(uv.y - (y1 - tv * (y1 - y0))) < fr) {
                col = vec3<f32>(0.85);
                if (view == 1u && k == 2) { col = vec3<f32>(1.0, 1.0, 0.4); }
                a = 1.0;
            }
        }
    }
    return vec4<f32>(col, a);
}

// Display-space shoulder used by the analytic views.
fn tone(x: vec3<f32>) -> vec3<f32> {
    let v = max(x, vec3<f32>(0.0));
    let hi = 0.8 + 0.2 * (1.0 - exp(-(v - 0.8) / 0.2));
    return select(hi, v, v < vec3<f32>(0.8));
}

// ACES filmic (Narkowicz fit) plus a little cross-channel bleed of the overflow so very hot cores whiten
fn aces(x0: vec3<f32>) -> vec3<f32> {
    let over = max(x0 - vec3<f32>(1.2), vec3<f32>(0.0));
    let x = x0 + vec3<f32>((over.x + over.y + over.z) * 0.06);
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;
    return clamp((x * (a * x + b)) / (x * (c * x + d) + e), vec3<f32>(0.0), vec3<f32>(1.0));
}

// Filmic: blend of per-channel ACES (whitens hot cores) and a hue-preserving ACES on luminance (keeps deep
// oranges/reds orange instead of clipping them to flat white), so bright flame cores keep tonal gradation.
fn filmic(x: vec3<f32>) -> vec3<f32> {
    let a = aces(x);
    let L = max(lum(x), 1e-5);
    let Lt = aces(vec3<f32>(L)).x;
    var hp = x * (Lt / L);
    let mx = max(hp.x, max(hp.y, hp.z));
    hp = hp / max(mx, 1.0);
    // only the very brightest cores (L >> 1) drift toward the per-channel (whiter) curve
    let w = 0.18 + 0.5 * smoothstep(2.5, 14.0, L);
    return mix(hp, a, w);
}

// blackbody chromaticity (normalised to red = 1) for the trailing soot-glow of flame tongues
fn planck_col(tk: f32) -> vec3<f32> {
    let lam = vec3<f32>(0.625, 0.550, 0.465);
    let x = 14388.0 / (lam * max(tk, 500.0));
    let l5 = pow(lam.x / lam, vec3<f32>(5.0));
    let e = exp(max(x.x - x, vec3<f32>(-80.0)));
    return l5 * e * (1.0 - exp(-x.x)) / max(1.0 - exp(-x), vec3<f32>(1e-6));
}

fn kind_of(code: f32) -> u32 { return u32(code + 0.5) & 15u; }
fn mat_of_code(code: f32) -> u32 { return u32(code + 0.5) >> 4u; }
fn is_solid_kind(k: u32) -> bool { return k >= 1u && k <= 5u; }

// two-phase flow-advected noise (all 4 channels), contrast-preserving blend
fn flow_noise(cf: vec2<f32>, d: vec4<f32>, scale: f32) -> vec4<f32> {
    let w0 = 1.0 - abs(2.0 * FP.ph0 - 1.0);
    let w1 = 1.0 - w0;
    let n0 = nzsb(cf + d.xy, scale, FBIAS);
    let n1 = nzsb(cf + d.zw + vec2<f32>(23.7, 41.3) / scale, scale, FBIAS);
    let nn = sqrt(w0 * w0 + w1 * w1);
    return clamp((n0 * w0 + n1 * w1 - 0.5) / nn + 0.5, vec4<f32>(0.0), vec4<f32>(1.0));
}

// Inlets, vents and fans: small UI-like tiles (display space)
fn device_col(kind: u32, cx: i32, cy: i32, loc: vec2<f32>, sh: f32) -> vec3<f32> {
    if (kind == 6u) { return vec3<f32>(0.45, 0.8, 0.15) * sh; }
    if (kind == 7u) { return vec3<f32>(0.2, 0.45, 1.0) * sh; }
    if (kind == 8u) { return vec3<f32>(0.6, 0.8, 0.9) * sh; }
    if (kind == 9u) { return vec3<f32>(0.4, 0.25, 0.55) * select(0.6, 1.0, ((cx + cy) & 1) == 0); }
    let dr = kind - 10u;
    var along = f32(cx) + loc.x;
    var across = loc.y;
    if (dr == 1u) { along = f32(cy) + loc.y; across = loc.x; }
    if (dr == 2u) { along = -(f32(cx) + loc.x); }
    if (dr == 3u) { along = -(f32(cy) + loc.y); across = loc.x; }
    let ph = fract((along - abs(across - 0.5)) * 0.5 - P.time * 0.4);
    return vec3<f32>(0.95, 0.55, 0.15) * select(0.55, 1.0, ph < 0.5) * sh;
}

// light arriving at a point from the fire (bloom fields), and its rough direction (toward brighter)
struct Light { col: vec3<f32>, dir: vec2<f32>, bloom: vec3<f32> }
fn fire_light(cf: vec2<f32>) -> Light {
    var l: Light;
    let bt = bil(LG, cf);
    let bw = bil(LH, cf);
    l.col = bt.rgb * 0.7 + bw.rgb * 1.9;
    l.bloom = bt.rgb * 0.34 + bw.rgb * 0.20;
    let g = vec2<f32>(bt.a, bw.a);
    let gl = length(g);
    l.dir = select(vec2<f32>(0.0, 1.0), g / gl, gl > 1e-4);
    return l;
}

// dark lab backdrop: faint vertical gradient (so dark smoke can occlude something)
fn backdrop(uv: vec2<f32>, cf: vec2<f32>) -> vec3<f32> {
    // night-sky gradient with a faint, large-scale atmospheric mottling (no grid: reads as footage, not a canvas)
    let g = mix(vec3<f32>(0.0030, 0.0042, 0.0085), vec3<f32>(0.0200, 0.0175, 0.0170), pow(uv.y, 2.4));
    let m = nzs(cf * 0.05 + vec2<f32>(FP.stime * 0.004, 0.0), 1.0).r;
    return g * (0.85 + 0.3 * m);
}

// first cell among (self, the neighbours on the near sides) whose kind belongs to group g
// (1 walls/igniters, 3 powder, 4 liquid); returns (-1,-1) if none
fn kind_group(k: u32) -> u32 {
    if (k == 1u || k == 2u || k == 5u) { return 1u; }
    return k;
}
fn pick_cell(cell: vec2<i32>, loc: vec2<f32>, g: u32) -> vec2<i32> {
    if (kind_group(kind_of(ld(LC, cell).a)) == g) { return cell; }
    let dx = select(1, -1, loc.x < 0.5);
    let dy = select(1, -1, loc.y < 0.5);
    var cand = array<vec2<i32>, 5>(vec2<i32>(dx, 0), vec2<i32>(0, dy), vec2<i32>(dx, dy), vec2<i32>(-dx, 0), vec2<i32>(0, -dy));
    for (var k = 0; k < 5; k++) {
        let q = cell + cand[k];
        if (kind_group(kind_of(ld(LC, q).a)) == g) { return q; }
    }
    return vec2<i32>(-1, -1);
}

// ---------- Fire view surface shaders (linear HDR out) ----------

fn shade_wall(cf: vec2<f32>, cell: vec2<i32>, lt: Light) -> vec3<f32> {
    let cs = ld(LC, cell);
    let kind = kind_of(cs.a);
    let mat = mat_of_code(cs.a);
    let loc = cf - vec2<f32>(cell);
    let cell_n = hash(u32(cell.x), u32(cell.y), 7u);
    var alb = cs.rgb;
    // brushed-metal streaks for metals, mottling for ceramic / stone-like
    if (mat <= 6u && mat != 5u) {
        alb *= 0.86 + 0.28 * nzs(cf * vec2<f32>(0.07, 1.0), 5.0).g;
    } else {
        alb *= 0.88 + 0.24 * nzs(cf, 3.0).b;
    }
    // bevel normal from the smooth wall mask gradient (rounded, anti-aliased edges)
    let e = 0.6;
    let gx = bil(LM, cf + vec2<f32>(e, 0.0)).b - bil(LM, cf - vec2<f32>(e, 0.0)).b;
    let gy = bil(LM, cf + vec2<f32>(0.0, e)).b - bil(LM, cf - vec2<f32>(0.0, e)).b;
    let nrm = -vec2<f32>(gx, gy) * 1.6; // points out of the wall
    let nl = min(length(nrm), 1.0);
    let n2 = select(vec2<f32>(0.0), normalize(nrm) * nl, nl > 1e-3);
    let sky = 0.30 + 0.32 * clamp(-n2.y, 0.0, 1.0) - 0.16 * clamp(n2.y, 0.0, 1.0);
    let fdot = clamp(dot(n2, lt.dir), 0.0, 1.0);
    // walls are lit mostly by the (wall-occluded) tight bloom; the quarter-res wide bloom straddles thin walls and
    // would light the OUTSIDE of a closed chamber with the light inside it
    let wl = bil(LG, cf).rgb * 0.9 + bil(LH, cf).rgb * 0.55;
    let lit = vec3<f32>(sky) + wl * (0.9 + 1.6 * fdot);
    var spec = vec3<f32>(0.0);
    if (mat <= 6u && mat != 5u) { spec = wl * fdot * fdot * 0.8 + vec3<f32>(0.06) * clamp(-n2.y, 0.0, 1.0) * nl; }
    let em = ld(LE, cell).rgb;
    var c = alb * lit + spec + em * (0.95 + 0.1 * cell_n) + lt.bloom * 0.06;
    if (kind == 5u && P.ign_on != 0u) {
        c += vec3<f32>(1.2, 0.45, 0.10) * (0.8 + 0.2 * sin(FP.stime * 9.0 + cell_n * 6.0));
    }
    return c;
}

fn shade_powder(cf: vec2<f32>, cell: vec2<i32>, lt: Light) -> vec3<f32> {
    let cs = ld(LC, cell);
    let loc = cf - vec2<f32>(cell);
    // irregular packed grains: jittered centre and size per cell, dark interstitial gaps
    let h1 = hash(u32(cell.x), u32(cell.y), 41u);
    let h2 = hash(u32(cell.x), u32(cell.y), 43u);
    let ctr = vec2<f32>(0.5) + (vec2<f32>(h1, h2) - 0.5) * 0.45;
    let rad = 0.40 + 0.22 * hash(u32(cell.x), u32(cell.y), 47u);
    let q = (loc - ctr) / rad;
    let dd = length(q);
    let inside = 1.0 - smoothstep(0.85, 1.05, dd);
    // dome shading lit from above-left, plus the fire light
    let dome = clamp(0.85 - 0.30 * q.y - 0.12 * q.x - 0.15 * dd * dd, 0.4, 1.15);
    let gap = 0.68;
    let speck = 0.85 + 0.3 * nzs(cf, 5.0).g;
    let sh = mix(gap, dome, inside) * speck;
    let em = ld(LE, cell).rgb;
    let flick = 0.85 + 0.3 * hash(u32(cell.x), u32(cell.y), u32(FP.stime * 8.0));
    return cs.rgb * sh * (vec3<f32>(0.42) + lt.col * 1.5) + em * mix(0.7, 1.0, inside) * flick + lt.bloom * 0.25;
}

fn shade_liquid(cf: vec2<f32>, cell: vec2<i32>, lt: Light) -> vec3<f32> {
    let lc0 = ld(LC, cell).rgb;
    let lc = mix(vec3<f32>(lum(lc0)), lc0, 0.8) * 0.85; // slightly muted: liquids read as translucent, not paint
    let le = ld(LE, cell).rgb;
    // depth below the free surface (mask above), surface highlight, ripples / caustics
    let a1 = bil(LM, cf - vec2<f32>(0.0, 0.7)).r;
    let dep = bil(LM, cf - vec2<f32>(0.0, 2.0)).r + bil(LM, cf - vec2<f32>(0.0, 5.0)).r + bil(LM, cf - vec2<f32>(0.0, 10.0)).r;
    let surf = 1.0 - smoothstep(0.35, 0.6, a1);
    let rip = nzs(cf * vec2<f32>(0.4, 1.0) + vec2<f32>(FP.stime * 0.12, FP.stime * 0.017), 3.0).r;
    let caus = nzs(cf - vec2<f32>(FP.stime * 0.12, 0.0), 1.7).a;
    let deep = clamp(dep / 3.0, 0.0, 1.0);
    var col = lc * mix(1.05, 0.38, deep) * (0.85 + 0.3 * rip);
    col *= vec3<f32>(0.30) + lt.col * 1.4;
    // light filtering down: soft caustic shimmer near the top
    col += lc * caus * caus * 0.25 * (1.0 - deep) * (vec3<f32>(0.3) + lt.col);
    // surface: sky + fire reflection band
    col += (vec3<f32>(0.10, 0.11, 0.13) + lt.col * 0.9) * surf * (0.6 + 0.6 * rip);
    return col + le * (0.9 + 0.2 * rip) + lt.bloom * 0.25;
}

@fragment
fn fs(in: VOut) -> @location(0) vec4<f32> {
    // cells per screen pixel (derivative taken in uniform control flow, before any branch)
    let fw = abs(dpdx(in.uv.x)) * P.cam_w;
    NLOD = log2(max(fw * 4.0, 1e-3));
    var cf = vec2<f32>(P.cam_x, P.cam_y) + in.uv * vec2<f32>(P.cam_w, P.cam_h);
    if (cf.x < 0.0 || cf.y < 0.0 || cf.x >= f32(P.w) || cf.y >= f32(P.h)) {
        return vec4<f32>(select(vec3<f32>(0.01), vec3<f32>(0.0004), P.srgb != 0u), 1.0);
    }
    let fancy = P.view == 0u || P.view == 6u;
    let cf0 = cf;
    var c = vec3<f32>(0.0);
    if (fancy) {
        // ---------- refraction: heat haze + blast waves ----------
        let a0 = bil(LA, cf);
        {
            let ddx = bil(LD, cf + vec2<f32>(0.5, 0.0)) - bil(LD, cf - vec2<f32>(0.5, 0.0));
            let ddy = bil(LD, cf + vec2<f32>(0.0, 0.5)) - bil(LD, cf - vec2<f32>(0.0, 0.5));
            let st = max(max(length(ddx.xy), length(ddy.xy)), max(length(ddx.zw), length(ddy.zw)));
            FBIAS = log2(1.0 + st * 1.5);
        }
        var off = vec2<f32>(0.0);
        var shock = 0.0;
        if (a0.a > 0.01) {
            // heat shimmer: rotated (not grid-aligned), rising, slowly evolving noise; smooth at pixel scale
            let rp = vec2<f32>(0.866 * cf.x - 0.5 * cf.y, 0.5 * cf.x + 0.866 * cf.y);
            let h1 = nzsb(rp + vec2<f32>(0.0, FP.stime * 1.1), 0.9, 0.6);
            let h2 = nzsb(cf * 1.7 + vec2<f32>(FP.stime * 0.23, FP.stime * 1.6), 0.9, 0.6);
            off += ((h1.xy + h2.yx) * 0.5 - 0.5) * a0.a * 0.9;
        }
        let pc = bil(LF, cf).b;
        if (abs(pc) > 0.04) {
            let gx = bil(LF, cf + vec2<f32>(1.0, 0.0)).b - bil(LF, cf - vec2<f32>(1.0, 0.0)).b;
            let gy = bil(LF, cf + vec2<f32>(0.0, 1.0)).b - bil(LF, cf - vec2<f32>(0.0, 1.0)).b;
            let gpr = vec2<f32>(gx, gy);
            off -= gpr * 0.9 / (1.0 + length(gpr) * 0.25);
            shock = length(gpr) * smoothstep(0.04, 0.2, abs(pc));
        }
        cf = clamp(cf + off, vec2<f32>(0.0), wh() - 0.001);

        let cell = vec2<i32>(floor(cf));
        let loc = cf - floor(cf);
        let cs = ld(LC, cell);
        let kind = kind_of(cs.a);
        let lt = fire_light(cf);
        let cell_n = hash(u32(cell.x), u32(cell.y), 7u);
        // smooth anti-aliased coverage of walls / powders / liquids from the filtered masks
        let mk = bsp(LM, cf);
        let aa = max(fw * 0.9, 0.03);
        let cov = smoothstep(vec3<f32>(0.5 - aa), vec3<f32>(0.5 + aa), mk.rgb);
        let gas_w = (1.0 - cov.r) * (1.0 - cov.g) * (1.0 - cov.b);

        if (gas_w > 0.0) {
            // ---------- gas: volumetric fire, smoke and gas clouds ----------
            let bg = backdrop(in.uv, cf0) + lt.col * 0.06;
            // high-resolution detail field (wavelet-turbulence up-res driven by the sim velocity)
            let dh = max(textureSampleLevel(DTX, SL, cf / wh(), 0.0), vec4<f32>(0.0));
            // ---- fire: coarse chromaticity (blackbody / chemiluminescence colour from colorize) x detail luminance
            let em0 = bsp(LA, cf).rgb;
            let L0 = lum(em0);
            let chroma = (em0 + vec3<f32>(0.020, 0.006, 0.001)) / (L0 + 0.0128);
            let Ld = dh.r;
            // thinning tongues (detail < coarse) cool toward deep orange-red; dense cores whiten in the tone map
            let Lc = min(Ld, L0 * 1.6);
            let kE = clamp(Ld / max(L0, 0.02), 0.0, 1.0);
            let ff = bil(LF, cf);
            // cooling toward red only applies to glowing soot; clean exhaust / chemiluminescence keeps its hue
            let sootc = smoothstep(0.01, 0.10, max(dh.g, ff.r));
            let tint = mix(vec3<f32>(1.0), vec3<f32>(1.0, 0.70 + 0.30 * kE, 0.45 + 0.55 * kE), sootc);
            // glow carried downstream of the front (Ld above the local coarse emission): soot cooling as it rises,
            // blackbody colour from its own brightness: yellow-white bright roots -> orange -> deep red tips
            let Lx = max(Ld - Lc, 0.0);
            let tb = 1050.0 + 620.0 * log2(1.0 + Lx * 2.0);
            let bbc = mix(chroma * lum(planck_col(min(tb, 2600.0))), planck_col(min(tb, 2600.0)), sootc);
            var em = chroma * Lc * tint + bbc / max(lum(bbc), 1e-4) * Lx * smoothstep(0.0, 0.05, Lx);
            // ---- projected volume: in 3D the reaction sheet wraps around the hot gas body and is seen face-on,
            // so hot volumes glow through their whole interior (patchy, flickering), not only along the 2D slice's
            // outline. Light = local fire light (tight + wide bloom of the front), gated by hot gas (haze) and by
            // sooty cloud (glowing embers inside a dark smoke mass). Never applied in cold clean air.
            let opv = clamp(dh.b, 0.0, 0.995);
            let sootv = clamp(dh.g / max(dh.b, 1e-4), 0.0, 1.0);
            let hotv = smoothstep(0.05, 0.55, max(dh.a, a0.a));
            let pn = flow_noise(cf * 0.8, bil(LD, cf), 0.7);
            let pn2 = flow_noise(cf * 1.9 + vec2<f32>(0.0, FP.stime * 0.5), bil(LD, cf), 1.3);
            let pch = smoothstep(0.25, 0.85, pn.g * 0.8 + pn2.b * 0.2);
            // only sooty / fuel-rich bodies are optically thick; clean fast exhaust (rockets, jets) stays translucent
            let sooty = smoothstep(0.015, 0.08, max(dh.g, ff.r));
            // gated by the gas's own temperature (haze ~ T): only genuinely hot sooty volumes are filled; cool
            // sooty pyrolysis gas and smoke stay dark (no evenly orange-filled clouds).
            let hotv2 = smoothstep(0.10, 0.60, max(dh.a, a0.a));
            let gate = hotv2 * sooty * (0.45 + 0.55 * smoothstep(0.0, 0.3, Ld + L0)) + opv * sootv * 0.15 * hotv2;
            let hh = smoothstep(0.35, 0.95, max(dh.a, a0.a));
            let fillc = lt.col * mix(vec3<f32>(1.0, 0.36, 0.10), vec3<f32>(1.0, 0.80, 0.42), hh * hh);
            // fills hollows only: where the front itself is already bright nothing is added (no blown-out cores)
            let fillv = fillc * gate * (0.25 + 1.8 * pch) * 2.2;
            em += max(fillv - em * 0.9, vec3<f32>(0.0));
            // ---- flame colour by chemistry: advected emitter tracers recolour the flame (atomic lines / molecular
            // bands of Na, K, Cu, S, Mg...) and add their own line glow where the gas is hot
            let ct = max(textureSampleLevel(CTX, SL, cf / wh(), 0.0), vec4<f32>(0.0));
            let cconc = max(ct.r, max(ct.g, ct.b));
            if (cconc > 0.004) {
                let hue = ct.rgb / cconc;
                let hn = hue / max(lum(hue), 0.05);
                let wq = smoothstep(0.0, 0.6, cconc);
                let Le = lum(em);
                em = mix(em, hn * Le * 1.2, wq * 0.85) + hn * wq * 0.10 * hotv;
            }
            // ---- billowing structure: flow-advected multi-octave turbulence breaks the smooth plateau of fire bodies
            // into rolling lobes, dark gaps and ragged tongues; low parts shift toward deep orange-red (blackbody),
            // not pastel. Clean rocket exhaust is left alone (translucent, collimated).
            {
                let dd = bil(LD, cf);
                let t1 = flow_noise(cf * 0.9, dd, 2.1);
                let t2 = flow_noise(cf * 1.3 + vec2<f32>(0.0, FP.stime * 0.7), dd, 4.6);
                let t3 = flow_noise(cf + vec2<f32>(9.0, 3.0), dd, 9.5);
                let tv = 0.45 * t1.r + 0.35 * t2.g + 0.20 * t3.b;
                let md = clamp(0.12 + 1.9 * smoothstep(0.22, 0.78, tv), 0.1, 1.9);
                let gate_t = smoothstep(0.006, 0.06, max(max(dh.g, ff.r), 0.0)) * 0.85 + 0.15 * smoothstep(0.2, 1.0, hotv);
                let md2 = mix(1.0, md, gate_t);
                let warm = mix(vec3<f32>(1.0, 0.50, 0.22), vec3<f32>(1.0), clamp(md2, 0.0, 1.0));
                em = em * md2 * warm;
            }
            // ---- ember sparks: tiny incandescent specks rising out of hot sooty gas (wood / hydrocarbon fires)
            {
                let gs = 3.0;
                let gc = floor(cf / gs);
                var sp = vec3<f32>(0.0);
                for (var oy = 0; oy <= 1; oy++) {
                    let g2 = gc + vec2<f32>(0.0, f32(oy));
                    let h0 = hash(u32(g2.x + 4096.0), u32(g2.y + 4096.0), 311u);
                    if (h0 < 0.10) {
                        let h1 = hash(u32(g2.x + 4096.0), u32(g2.y + 4096.0), 313u);
                        let h2 = hash(u32(g2.x + 4096.0), u32(g2.y + 4096.0), 317u);
                        let ph = fract(FP.stime * (0.25 + 0.35 * h1) + h2);
                        let pos = (g2 + vec2<f32>(0.5 + 0.35 * sin(FP.stime * 3.0 * h1 + h2 * 6.28) * (0.4 + ph), 1.0 - ph * 1.6)) * gs;
                        let dsp = length(cf - pos);
                        let rad = 0.55 + 0.3 * h1;
                        let life = smoothstep(0.0, 0.1, ph) * (1.0 - ph) * (1.0 - ph);
                        let hotg = smoothstep(0.3, 0.8, max(dh.a, a0.a)) + smoothstep(0.04, 0.12, max(dh.g, ff.r)) * 0.5;
                        sp += vec3<f32>(1.0, 0.45, 0.12) * (1.0 - smoothstep(0.0, rad, dsp)) * life * min(hotg, 1.0) * 5.0;
                    }
                }
                em += sp * smoothstep(0.02, 0.12, max(max(dh.g, ff.r), 0.0));
            }
            // ---- smoke and gas clouds (Beer-Lambert opacity from the detail field)
            let cl = bsp(LB, cf);
            let op = clamp(dh.b, 0.0, 0.995);
            let sootf = clamp(dh.g / max(dh.b, 1e-4), 0.0, 1.0); // share of the cloud that is smoke
            let alb = cl.rgb / max(cl.a, 1e-4);
            // self-shadowing: optical depth toward the sky (up) and toward the fire, from the detail field
            let ws = 1.0 / wh();
            let tau_up = textureSampleLevel(DTX, SL, (cf + vec2<f32>(0.0, -2.0)) * ws, 0.0).b
                + 1.3 * bil(LB, cf + vec2<f32>(0.0, -7.0)).a;
            let tau_f = textureSampleLevel(DTX, SL, (cf + lt.dir * 1.5) * ws, 0.0).b + bil(LB, cf + lt.dir * 5.0).a;
            // relief from the detail density gradient: surfaces facing a light are brighter (billows, filaments)
            let e = 0.45;
            let b_up = textureSampleLevel(DTX, SL, (cf + vec2<f32>(0.0, -e)) * ws, 0.0).b;
            let b_dn = textureSampleLevel(DTX, SL, (cf + vec2<f32>(0.0, e)) * ws, 0.0).b;
            let b_tf = textureSampleLevel(DTX, SL, (cf + lt.dir * e) * ws, 0.0).b;
            let b_af = textureSampleLevel(DTX, SL, (cf - lt.dir * e) * ws, 0.0).b;
            let rel_sky = clamp(0.8 + 4.0 * (b_dn - b_up), 0.35, 1.8);
            let rel_f = clamp(0.8 + 4.0 * (b_af - b_tf), 0.3, 2.0);
            let sky = (vec3<f32>(0.46, 0.49, 0.56) * exp(-tau_up * 1.3) + vec3<f32>(0.04)) * rel_sky;
            // fire light reaching the smoke: strongly self-shadowed (a thick soot mass is lit only on the side facing
            // the flame, its interior stays dark grey-brown instead of an evenly orange-lit cloud)
            let fl = lt.col * 1.1 * exp(-tau_f * 2.2) * rel_f;
            // fire light scatters strongly in the hot smoke right above the flame (warm undersides)
            let inner = em0 * 0.25 * sootf;
            let self_lit = vec3<f32>(0.08) * (1.0 - sootf);
            let smoke = alb * (sky + fl + self_lit) + inner;
            // emission is partly hidden behind smoke that is in front of it (half the cloud lies in front)
            c = bg * (1.0 - op) + smoke * op + em * (1.0 - 0.25 * op * sootf) + lt.bloom * (1.0 - 0.55 * op);
            // white oxide smoke (Al2O3 / MgO / K2CO3 / NH4Cl): bright, sky- and fire-lit, self-shadowed from above
            if (ct.a > 0.003) {
                // soft-edged, translucent oxide smoke: 4 extra taps (+-2 cells) remove the grid-aligned sawtooth cut-off
                let wa = (ct.a * 2.0 + textureSampleLevel(CTX, SL, (cf + vec2<f32>(2.0, 0.0)) * ws, 0.0).a
                    + textureSampleLevel(CTX, SL, (cf - vec2<f32>(2.0, 0.0)) * ws, 0.0).a
                    + textureSampleLevel(CTX, SL, (cf + vec2<f32>(0.0, 2.0)) * ws, 0.0).a
                    + textureSampleLevel(CTX, SL, (cf - vec2<f32>(0.0, 2.0)) * ws, 0.0).a) * (1.0 / 6.0);
                var opw = 0.82 * (1.0 - exp(-wa * 1.6));
                let ta = textureSampleLevel(CTX, SL, (cf + vec2<f32>(0.0, -3.0)) * ws, 0.0).a;
                var wl = vec3<f32>(0.80, 0.80, 0.78) * ((vec3<f32>(0.42, 0.45, 0.52) * exp(-ta * 0.9) + vec3<f32>(0.05)) + lt.col * 1.3 * exp(-tau_f * 1.2));
                // oxide particles in hot gas are incandescent (the blinding white-orange core of an APCP/Al plume,
                // a burning magnesium ribbon's halo); they cool to white smoke within a few cells
                // (particles lag the gas: they keep the stagnation heat of the fast core after the gas itself has expanded and cooled)
                let spd9 = length(bil(9, cf).xy);
                let hw = smoothstep(0.25, 0.9, max(max(dh.a, a0.a), 0.85 * smoothstep(1.2, 4.5, spd9)));
                opw *= mix(0.5, 1.0, hw * hw);
                wl = wl * (1.0 - 0.7 * hw) + mix(vec3<f32>(1.0, 0.40, 0.10), vec3<f32>(1.0, 0.72, 0.42), hw * hw) * hw * 2.0;
                c = c * (1.0 - opw) + wl * opw + em * opw * 0.6;
            }
            if (kind >= 6u) {
                // inlets, vents, fans: crisp device tiles
                let sh = 0.9 + 0.2 * cell_n;
                c = to_lin(device_col(kind, cell.x, cell.y, loc, sh)) * (vec3<f32>(0.8) + lt.col * 2.0) + lt.bloom * 0.3;
            }
        }
        if (gas_w > 0.0 && shock > 0.0) {
            // blast-wave front: faint pale ring where the pressure gradient is steep (shadowgraph-like)
            c += vec3<f32>(0.55, 0.62, 0.78) * min(shock * 0.07, 0.4) * gas_w;
        }
        if (cov.r > 0.0) {
            let lcell = pick_cell(cell, loc, 4u);
            if (lcell.x >= 0) { c = mix(c, shade_liquid(cf, lcell, lt), cov.r); }
        }
        if (cov.g > 0.0) {
            let pcell = pick_cell(cell, loc, 3u);
            if (pcell.x >= 0) { c = mix(c, shade_powder(cf, pcell, lt), cov.g); }
        }
        if (cov.b > 0.0) {
            let wcell = pick_cell(cell, loc, 1u);
            if (wcell.x >= 0) { c = mix(c, shade_wall(cf, wcell, lt), cov.b); }
        }
        c = select(vec3<f32>(0.0), c, c == c); // drop NaNs
        // ---------- post: exposure, filmic tone map, vignette, grain ----------
        // logarithmic highlight compression (C1 at L = 1) before the filmic curve: 3000 K chambers and fireball
        // cores keep visible gradients instead of clipping to a flat cream fill
        {
            let L = lum(c);
            if (L > 1.0) { c *= (1.0 + 0.85 * log(1.0 + (L - 1.0) / 0.85)) / L; }
            // saturated cores keep a warm gradient instead of clipping to flat cream: overflow is tinted toward
            // the blackbody hue of the pixel (yellow-orange -> white only at the extreme core)
            let L2 = lum(c);
            if (L2 > 1.2) {
                let hue = c / max(L2, 1e-4);
                c = mix(c, hue * L2 * vec3<f32>(1.0, 0.74, 0.46), 0.8 * smoothstep(1.2, 2.6, L2));
            }
        }
        c = filmic(c * 1.05);
        let vg = in.uv - 0.5;
        c *= 1.0 - 0.28 * dot(vg, vg) * 2.0;
        c = pow(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(1.0 / 2.2)); // linear -> display
        let px = vec2<u32>(in.pos.xy);
        c += (hash(px.x, px.y, P.frame & 63u) - 0.5) * 0.012;
    } else {
        // ---------- analytic views ----------
        let cell = vec2<i32>(floor(cf));
        let loc = cf - floor(cf);
        let cs = ld(LC, cell);
        let kind = kind_of(cs.a);
        let cell_n = hash(u32(cell.x), u32(cell.y), 7u);
        let sh = 0.9 + 0.2 * cell_n;
        var gone = false;
        if (kind == 2u) {
            let rem = ld(LF, cell).a;
            let dd = max(abs(loc.x - 0.5), abs(loc.y - 0.5)) * 2.0;
            gone = dd > sqrt(max(rem, 0.0)) * (0.9 + 0.2 * cell_n) + 0.02;
        }
        if (kind == 0u || kind >= 6u || gone) {
            let cl = bsp(LB, cf);
            if (P.view == 4u) {
                // Gases view: strong colours as soft wisps over a dark backdrop
                let d = bil(LD, cf);
                let fn1 = flow_noise(cf, d, 1.15);
                let op = clamp(cl.a * (1.0 + (fn1.r - 0.5) * 0.9 * (1.1 - cl.a)), 0.0, 1.0);
                let alb = cl.rgb / max(cl.a, 1e-4);
                c = vec3<f32>(0.03, 0.03, 0.04) * (1.0 - op) + alb * op;
            } else {
                c = cl.rgb;
            }
            if (kind >= 6u && !gone) { c = device_col(kind, cell.x, cell.y, loc, sh); }
        } else if (kind == 3u || kind == 4u) {
            c = cs.rgb;
            if (kind == 3u) {
                let dd = length(loc - vec2<f32>(0.42, 0.40));
                c *= 1.05 - 0.4 * smoothstep(0.25, 0.75, dd);
            } else {
                let above = kind_of(ld(LC, cell + vec2<i32>(0, -1)).a);
                if (above != 4u && loc.y < 0.3) { c = c * 1.3 + 0.04; }
            }
        } else {
            c = cs.rgb;
            let ku = kind_of(ld(LC, cell + vec2<i32>(0, -1)).a);
            let kd = kind_of(ld(LC, cell + vec2<i32>(0, 1)).a);
            if (!is_solid_kind(ku) && loc.y < 0.2) { c *= 1.25; }
            if (!is_solid_kind(kd) && loc.y > 0.8) { c *= 0.75; }
        }
        // anti-aliased solid boundaries: blend gas field colour and wall colour by the filtered coverage mask
        {
            let mk = bsp(LM, cf);
            let aa = max(fw * 0.9, 0.03);
            let covb = smoothstep(0.5 - aa, 0.5 + aa, mk.b);
            if (covb > 0.0 && covb < 1.0) {
                let wc = pick_cell(cell, loc, 1u);
                if (wc.x >= 0) {
                    var wcol = ld(LC, wc).rgb;
                    let wn = vec2<f32>(bil(LM, cf - vec2<f32>(0.6, 0.0)).b - bil(LM, cf + vec2<f32>(0.6, 0.0)).b,
                                       bil(LM, cf - vec2<f32>(0.0, 0.6)).b - bil(LM, cf + vec2<f32>(0.0, 0.6)).b);
                    wcol = wcol * (1.0 + 0.35 * clamp(-wn.y, 0.0, 1.0) - 0.2 * clamp(wn.y, 0.0, 1.0));
                    let gcol = bsp(LB, cf).rgb;
                    c = mix(gcol, wcol, covb);
                }
            }
        }
        c = select(vec3<f32>(0.0), c, c == c);
        c = clamp(tone(c), vec3<f32>(0.0), vec3<f32>(1.0));
        let lg = legend(in.uv, P.view);
        c = mix(c, lg.rgb, lg.a);
    }
    if (P.srgb != 0u) { c = pow(max(c, vec3<f32>(1e-6)), vec3<f32>(2.2)); }
    return vec4<f32>(c, 1.0);
}
