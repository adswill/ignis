// High-resolution render-only detail simulation (wavelet-turbulence-style up-res, Kim et al. 2008, simplified).
//
// A DS x finer 2D field (DS = 4 -> 2048 x 1152 for the 512 x 288 sim) of four scalars:
//   r  flame luminance  (anchored to lum(L0 gas emission))
//   g  soot opacity     (anchored to L5.r)
//   b  cloud opacity    (anchored to L1.a: smoke + steam + gas tints)
//   a  heat-haze weight (anchored to L0.a)
// Each frame (only while the sim advances):
//   1. semi-Lagrangian RK2 backtrace with the bilinear coarse velocity (L9.xy) + synthetic small-scale turbulence
//      (curl of two drifting noise octaves, amplitude from local coarse speed, vorticity and heat)
//   2. re-anchor: add the mismatch between the coarse field and a low-pass of the advected detail
//      (keeps sub-cell filaments, removes any drift at cell scale), plus a slow direct relaxation
//   3. clamp against the local coarse value: detail can only exist where the physics has smoke/fire
//      (no layers or slabs in clean air)
// render.wgsl reads the result with linear filtering.

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
@group(0) @binding(4) var SL: sampler; // linear clamp
@group(0) @binding(5) var SR: sampler; // linear repeat
@group(0) @binding(6) var DSRC: texture_2d<f32>;
@group(0) @binding(7) var DDST: texture_storage_2d<rgba16float, write>;
// chemistry colour tracers (same resolution): rgb = flame-emitter colour x concentration, a = white oxide smoke
@group(0) @binding(8) var CSRC: texture_2d<f32>;
@group(0) @binding(9) var CDST: texture_storage_2d<rgba16float, write>;

const LA: i32 = 0;
const LB: i32 = 1;
const LF: i32 = 5;
const LV: i32 = 9;
const LK: i32 = 10;

fn wh() -> vec2<f32> { return vec2<f32>(f32(P.w), f32(P.h)); }
fn bil(layer: i32, cf: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(TX, SL, cf / wh(), layer, 0.0);
}
fn lum(c: vec3<f32>) -> f32 { return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722)); }

fn anchor(cf: vec2<f32>) -> vec4<f32> {
    let a = bil(LA, cf);
    let b = bil(LB, cf);
    let f = bil(LF, cf);
    return vec4<f32>(min(lum(a.rgb), 64.0), f.r, b.a, a.a);
}
// cubic B-spline (C2) lookup from 4 bilinear taps. The re-anchoring target must not have the bilinear kinks at
// every coarse cell centre: subtracting a low-pass from a piecewise-linear field injects a ripple at exactly the
// grid frequency, which the turbulence then bends into wavy moire stripes around every flame.
fn bspl(layer: i32, cf: vec2<f32>) -> vec4<f32> {
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
    let c0 = (i - 0.5 + w1 / g0) / wh();
    let c1 = (i + 1.5 + w3 / g1) / wh();
    let a = textureSampleLevel(TX, SL, c0, layer, 0.0);
    let b = textureSampleLevel(TX, SL, vec2<f32>(c1.x, c0.y), layer, 0.0);
    let c = textureSampleLevel(TX, SL, vec2<f32>(c0.x, c1.y), layer, 0.0);
    let d = textureSampleLevel(TX, SL, c1, layer, 0.0);
    return (a * g0.x + b * g1.x) * g0.y + (c * g0.x + d * g1.x) * g1.y;
}
fn anchor_smooth(cf: vec2<f32>) -> vec4<f32> {
    let a = bspl(LA, cf);
    let b = bspl(LB, cf);
    let f = bspl(LF, cf);
    return max(vec4<f32>(min(lum(a.rgb), 64.0), f.r, b.a, a.a), vec4<f32>(0.0));
}

// noise potential: two octaves drifting in different directions (evolving, not just scrolling)
fn psi(p: vec2<f32>) -> f32 {
    let t = FP.stime;
    // band-limited mips: wavelengths ~2-18 cells (octave a) and ~1.9-8 cells (octave b); never texel-scale
    let a = textureSampleLevel(NZT, SR, (p * 0.9 + vec2<f32>(0.07 * t, 0.45 * t)) / 64.0, 1.0).r;
    let b = textureSampleLevel(NZT, SR, (p * 2.1 + vec2<f32>(-0.31 * t, 0.85 * t) + 17.3) / 64.0, 2.0).g;
    return a + 0.35 * b;
}
// divergence-free turbulent velocity (cells / time unit, before amplitude scaling)
fn curl(p: vec2<f32>) -> vec2<f32> {
    let e = 0.6;
    let dx = psi(p + vec2<f32>(e, 0.0)) - psi(p - vec2<f32>(e, 0.0));
    let dy = psi(p + vec2<f32>(0.0, e)) - psi(p - vec2<f32>(0.0, e));
    return vec2<f32>(dy, -dx) / (2.0 * e) * 11.0;
}

fn vel(p: vec2<f32>, amp: f32) -> vec2<f32> {
    return bil(LV, p).xy + curl(p) * amp;
}

fn dsrc(p: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(DSRC, SL, p / wh(), 0.0);
}

@compute @workgroup_size(8, 8)
fn detail(@builtin(global_invocation_id) g: vec3<u32>) {
    let dims = textureDimensions(DDST);
    if (g.x >= dims.x || g.y >= dims.y) { return; }
    let ds = f32(dims.x) / f32(P.w);
    let cf = (vec2<f32>(g.xy) + 0.5) / ds; // coarse cell coordinates
    let c = anchor_smooth(cf);
    if ((FP.reset & 4u) != 0u) {
        textureStore(DDST, vec2<i32>(g.xy), c);
        textureStore(CDST, vec2<i32>(g.xy), vec4<f32>(0.0));
        return;
    }
    let dt = min(FP.dt, 2.0);
    let v0 = bil(LV, cf);
    // turbulence amplitude: grows with the local coarse speed, vorticity and heat (flames flicker even when slow)
    let heat = clamp(c.r / 1.5, 0.0, 1.0);
    let smoke = clamp(c.b * 2.0, 0.0, 1.0);
    // (speed term saturates: the potential core of a fast jet is smooth; its shear layer carries vorticity instead)
    let spd = length(v0.xy);
    let amp = min(0.10 + 0.20 * min(spd, 1.5) + 0.7 * v0.z + 0.30 * heat + 0.12 * smoke, 1.3);
    // RK2 backtrace; the turbulence term is evaluated once (it varies slowly at the step length)
    let ut = curl(cf) * amp;
    let mid = cf - 0.5 * dt * (v0.xy + ut);
    let src = cf - dt * (bil(LV, mid).xy + ut);
    // (bilinear semi-Lagrangian lookup; turbulence is band-limited to >= ~2 cells so no texel-scale mazes form)

    // low-pass of the advected detail at the coarse-cell scale
    // separable tent (1,2,1)/4 per axis at +-0.5 cell: its frequency response (1 + cos)/2 is never negative. The old
    // 4-tap diagonal box had response -1 at exactly one coarse cell (4 texels), so `h += (c - hl) * k1` AMPLIFIED
    // grid-frequency ripple every frame -> the wavy moire / halftone stripes around flames and smoke.
    let o = 0.5;
    let hc = dsrc(src);
    let hl = (4.0 * hc
        + 2.0 * (dsrc(src + vec2<f32>(o, 0.0)) + dsrc(src + vec2<f32>(-o, 0.0)) + dsrc(src + vec2<f32>(0.0, o)) + dsrc(src + vec2<f32>(0.0, -o)))
        + dsrc(src + vec2<f32>(o, o)) + dsrc(src + vec2<f32>(-o, o)) + dsrc(src + vec2<f32>(o, -o)) + dsrc(src + vec2<f32>(-o, -o))) / 16.0;
    var h = max(hc, vec4<f32>(0.0));
    let r_adv = h.r;
    let k1 = 1.0 - exp(-dt / 2.5);
    let k2 = 1.0 - exp(-dt / 25.0);
    h = h + (c - max(hl, vec4<f32>(0.0))) * k1;
    h = mix(h, c, k2);
    // detail lives only where the coarse field has something (kills drift, trails and any uniform haze)
    let cm = max(c, anchor(src));
    let hi = cm * vec4<f32>(3.0, 2.5, 2.2, 2.0) + vec4<f32>(0.03, 0.004, 0.006, 0.01);
    h = clamp(h, vec4<f32>(0.0), hi);
    // flame luminance is a persistent glow tracer (hot soot carried by the flow): sourced by the coarse emission,
    // advected with the turbulent velocity and burning out over a few time units (faster in cooler gas). This turns
    // thin reaction fronts into filled, licking flame tongues rooted at the front and trailing downstream.
    let hot = max(c.a, anchor(src).a);
    // Only glowing soot persists: clean, fast exhaust emits where it is hot NOW (shock diamonds are stationary
    // features in a moving flow and must not be smeared downstream into streaks).
    let sooty = 0.2 + 0.8 * smoothstep(0.01, 0.12, max(c.g, anchor(src).g));
    let tau = (1.6 + 3.5 * hot) * sooty / (1.0 + 0.08 * spd * spd);
    let src_r = c.r * (0.85 + 0.3 * psi(cf * 1.7 + vec2<f32>(0.0, FP.stime * 0.6)) / 1.35);
    h.x = min(max(r_adv * exp(-dt / tau), max(h.x, src_r)), 64.0);
    h.y = min(h.y, 0.995);
    h.z = min(h.z, 0.995);
    h = select(c, h, h == h);
    textureStore(DDST, vec2<i32>(g.xy), h);

    // ---- chemistry colour tracers: advected with the same turbulent backtrace, sourced by burning matter (L10).
    // Emitter colour lives as long as the gas is hot (it is only displayed where the flame is luminous anyway);
    // white oxide smoke (Al2O3, MgO, K2CO3...) is a long-lived, slowly thinning render-only cloud.
    let seed = max(bil(LK, cf), vec4<f32>(0.0));
    var ct = max(textureSampleLevel(CSRC, SL, src / wh(), 0.0), vec4<f32>(0.0));
    let ktc = exp(-dt / (1.5 + 5.0 * hot));
    ct = vec4<f32>(ct.rgb * ktc, ct.a * exp(-dt / 70.0));
    ct = vec4<f32>(max(ct.rgb, seed.rgb), max(ct.a + seed.a * 0.08 * dt, seed.a * 0.3));
    ct = select(vec4<f32>(0.0), ct, ct == ct);
    textureStore(CDST, vec2<i32>(g.xy), clamp(ct, vec4<f32>(0.0), vec4<f32>(1.0, 1.0, 1.0, 4.0)));
}
