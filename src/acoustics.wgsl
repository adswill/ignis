// Ignis acoustics: virtual microphone + acoustic source features, computed from the live solver state.
// Own module and bind group (the sim bind group is at the 8-storage-buffer limit). Read-only on the sim.
//
// The physics constants used here (RS, CV, Q_FUEL, A_PRE, T_ACT, C_EDC, T_EDC, D_SPEC, CS2, NU_T_MAX, T_CHEM_MIN, T_DISS0/1,
// K_WOOD, T_WOOD, T_PROP_IGN, RHO_MIN, P_MIN) are NOT defined in this file: acoustics.rs prepends the
// matching `const` lines extracted from sim.wgsl at pipeline-creation time, so this stays in sync with the
// solver automatically (with documented fallbacks if a constant is ever renamed).
//
// Kernels:
//   mic_sample  (1 thread, dispatched once per sim step inside the sim pass): appends the gas pressure at the
//               left ear, centre and right ear of the listener (bilinear, atm) and the gas speed at the centre
//               to MIC. MIC[0].x holds the sample count (u32 bits); the Rust side clears it after each frame's copy.
//   features    (dispatch NB x NS workgroups of 256, once per frame): per spatial band (NB columns across the
//               camera view; the outer bands also catch everything off-screen) and per row slab (NS), reduces
//               the acoustic source terms listed in F_* below into PART[(slab * NB + band) * NF + f].
//               The CPU sums the NS slabs (or takes the max for the F_*MAX slots).

struct AcParams {
    w: u32, h: u32, nb: u32, ns: u32,
    lx: f32, ly: f32, ear: f32, dt: f32,
    cam_x: f32, cam_w: f32, burn_thr: f32, jet_u: f32,
    mic_cap: u32, jet_m: f32, frame: u32, _p2: u32,
}

@group(0) @binding(0) var<uniform> A: AcParams;
@group(0) @binding(1) var<storage, read> U0: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> META: array<u32>;
@group(0) @binding(3) var<storage, read> WALL: array<vec4<f32>>;
@group(0) @binding(4) var<storage, read_write> MIC: array<vec4<f32>>;
@group(0) @binding(5) var<storage, read_write> PART: array<f32>;

const NF: u32 = 48u;
// feature slots (keep in sync with `pub mod f` in acoustics.rs)
const F_Q: u32 = 0u;       // sum q = Q_FUEL * burn_rate   (heat release, energy units / tu / cell)
const F_QX: u32 = 1u;      // sum q * dx   (dx, dy = cell position relative to the listener, cells)
const F_QY: u32 = 2u;
const F_QXX: u32 = 3u;     // sum q * dx^2
const F_QYY: u32 = 4u;
const F_FLN: u32 = 5u;     // flame cells (q > burn_thr)
const F_FLT: u32 = 6u;     // sum of T over flame cells
const F_PR: u32 = 7u;      // Proudman source: sum rho * eps * Mt^5 (sim units)
const F_PRX: u32 = 8u;
const F_PRY: u32 = 9u;
const F_PRU: u32 = 10u;    // sum Proudman * u'  (for the turbulence velocity scale)
const F_JN: u32 = 11u;     // jet cells (|u| > jet_u)
const F_JKE: u32 = 12u;    // sum 0.5 rho |u|^3 over jet cells
const F_JU4: u32 = 13u;    // sum rho |u|^4
const F_JRHO: u32 = 14u;   // sum rho
const F_JX: u32 = 15u;     // jet cell position moments (relative to listener)
const F_JY: u32 = 16u;
const F_JXX: u32 = 17u;
const F_JYY: u32 = 18u;
const F_JXY: u32 = 19u;
const F_UMAX: u32 = 20u;   // max |u| (max-reduced)
const F_VORT: u32 = 21u;   // sum |curl u|
const F_SHN: u32 = 22u;    // shock cells (|grad p| / p > 0.15 per cell)
const F_SHS: u32 = 23u;    // sum of |grad p| / p over shock cells
const F_SHJ: u32 = 24u;    // shock cells inside jets (Mach-wave / crackle sources)
const F_PMAX: u32 = 25u;   // max p (max-reduced)
const F_WOODN: u32 = 26u;  // exposed, pyrolysing wood cells
const F_WOODR: u32 = 27u;  // sum pyrolysis rate K_WOOD exp(-T_WOOD / T)
const F_PROPN: u32 = 28u;  // burning propellant surface cells
const F_MBN: u32 = 29u;    // matter cells burning / emitting hot products (emission kind 3, 4)
const F_MBR: u32 = 30u;    // sum of their emission rates
const F_BOILN: u32 = 31u;  // matter cells boiling / vapourising (kind 1)
const F_BOILR: u32 = 32u;
const F_FIZN: u32 = 33u;   // gas-releasing reactions (kinds 2, 5, 6, 7)
const F_FIZR: u32 = 34u;
const F_DETN: u32 = 35u;   // matter cells flagged to detonate this frame
const F_WALLS: u32 = 36u;  // wall cells
const F_SW: u32 = 37u;     // sum stress^2 over walls with stress > 0.3
const F_SWF: u32 = 38u;    // sum stress^2 * first modal frequency of the material (Hz)
const F_SWQ: u32 = 39u;    // sum stress^2 * modal Q
const F_SMAX: u32 = 40u;   // max stress ratio (max-reduced)
const F_FAIL: u32 = 41u;   // walls about to fail (damage > 0.9)
const F_FAILF: u32 = 42u;  // sum of their modal frequency
const F_FANN: u32 = 43u;   // fan cells
const F_FANU: u32 = 44u;   // sum |u| at fan cells
const F_INN: u32 = 45u;    // gas cells next to a gas inlet (fuel / O2 / air)
const F_INF: u32 = 46u;    // sum rho |u| there (mass flux)
const F_INR: u32 = 47u;    // sum rho there

// representative first bending-mode frequency (Hz) and modal Q of cm-scale structural members, wall materials 0-9:
// steel, aluminium, copper, titanium, inconel, ceramic, cooled copper, wood, propellant, explosive
var<private> AC_F1: array<f32, 10> = array<f32, 10>(380.0, 520.0, 330.0, 450.0, 360.0, 1400.0, 330.0, 160.0, 250.0, 250.0);
var<private> AC_Q: array<f32, 10> = array<f32, 10>(900.0, 1200.0, 600.0, 1000.0, 800.0, 400.0, 400.0, 25.0, 15.0, 15.0);

var<private> AC_OFF: array<vec2<i32>, 4> = array<vec2<i32>, 4>(vec2<i32>(1, 0), vec2<i32>(-1, 0), vec2<i32>(0, 1), vec2<i32>(0, -1));

struct G { a: vec4<f32>, b: vec4<f32> }

fn ac_cid(x: i32, y: i32) -> u32 {
    let cx = clamp(x, 0, i32(A.w) - 1);
    let cy = clamp(y, 0, i32(A.h) - 1);
    return u32(cy) * A.w + u32(cx);
}
fn ac_tile(i: u32) -> u32 { return META[i] & 0xFFu; }
fn ac_solid(t: u32) -> bool { return t == 1u || t == 6u || t == 8u || t == 9u || t == 10u || t == 12u || t == 13u; }
fn ac_ld(i: u32) -> G { return G(U0[2u * i], U0[2u * i + 1u]); }
fn ac_rho(s: G) -> f32 { return max(s.a.x + s.a.y + s.a.z + s.a.w, RHO_MIN); }
fn ac_gm1(s: G) -> f32 { return dot(s.a, RS) / max(dot(s.a, CV), 1e-6); }
fn ac_p(s: G) -> f32 {
    let rho = ac_rho(s);
    let ke = 0.5 * (s.b.x * s.b.x + s.b.y * s.b.y) / rho;
    return max(ac_gm1(s) * (s.b.z - ke), P_MIN);
}
fn ac_t(s: G) -> f32 { return ac_p(s) / (ac_rho(s) * (dot(s.a, RS) / ac_rho(s))); }
fn ac_vel(i: u32) -> vec2<f32> {
    if (ac_solid(ac_tile(i))) { return vec2<f32>(0.0); }
    let s = ac_ld(i);
    return vec2<f32>(s.b.x, s.b.y) / ac_rho(s);
}
// same as sim.wgsl `burn_rate` (nu_t = 0, as used by the `stats` readout): Arrhenius + EDC floor, no chemistry
// below T_CHEM_MIN, dissociation limit between T_DISS0 and T_DISS1
fn ac_burn(s: G) -> f32 {
    let tt = ac_t(s);
    if (tt < T_CHEM_MIN || s.a.x <= 1e-6 || s.a.y <= 1e-6) { return 0.0; }
    var w = A_PRE * s.a.x * s.a.y * exp(-T_ACT / tt);
    if (tt > T_EDC) { w = max(w, C_EDC * D_SPEC * min(s.a.x, 0.25 * s.a.y)); }
    let fd = clamp((T_DISS1 - tt) / (T_DISS1 - T_DISS0), 0.0, 1.0);
    return min(w * fd * fd * (3.0 - 2.0 * fd), s.a.x / A.dt);
}

// bilinear gas pressure (atm) at a cell-space point (cell centres at integer + 0.5); solids are skipped
fn p_at(px: f32, py: f32) -> f32 {
    let fx = px - 0.5;
    let fy = py - 0.5;
    let x0 = i32(floor(fx));
    let y0 = i32(floor(fy));
    let tx = fx - f32(x0);
    let ty = fy - f32(y0);
    var acc = 0.0;
    var wsum = 0.0;
    for (var k = 0; k < 4; k++) {
        let ox = k & 1;
        let oy = k >> 1u;
        let i = ac_cid(x0 + ox, y0 + oy);
        let wx = select(1.0 - tx, tx, ox == 1);
        let wy = select(1.0 - ty, ty, oy == 1);
        let wgt = wx * wy;
        if (!ac_solid(ac_tile(i))) {
            acc += wgt * ac_p(ac_ld(i));
            wsum += wgt;
        }
    }
    if (wsum < 1e-4) { return 1.0; }
    return acc / wsum;
}

@compute @workgroup_size(1)
fn mic_sample() {
    let n = bitcast<u32>(MIC[0].x);
    if (n >= A.mic_cap) { return; }
    let pl = p_at(A.lx - A.ear, A.ly);
    let pc = p_at(A.lx, A.ly);
    let pr = p_at(A.lx + A.ear, A.ly);
    let ci = ac_cid(i32(floor(A.lx)), i32(floor(A.ly)));
    let spd = length(ac_vel(ci));
    MIC[1u + n] = vec4<f32>(pl, pc, pr, spd);
    MIC[0] = vec4<f32>(bitcast<f32>(n + 1u), 0.0, 0.0, 0.0);
}

// Per-thread accumulators: a private array indexed ONLY with constants (cell() after inlining, and the
// reduction code that acoustics.rs generates unrolled at the REDUCE marker in features), so it stays in registers.
// Keep workgroup memory small (4.5 KB) for occupancy: the kernel is memory-latency bound.
const WG: u32 = 128u;
var<private> ACC: array<f32, 48>;
var<private> WGT: f32;
var<workgroup> RED: array<f32, 1024>; // 8 features x WG
var<workgroup> RED2: array<f32, 128>;
fn sh_add(k: u32, v: f32) { ACC[k] += v * WGT; }
fn sh_max(k: u32, v: f32) { ACC[k] = max(ACC[k], v); }

fn cell(x: i32, y: i32) {
    WGT = 1.0;
    let i = u32(y) * A.w + u32(x);
    let t = ac_tile(i);
    let dx = f32(x) + 0.5 - A.lx;
    let dy = f32(y) + 0.5 - A.ly;
    if (t == 1u) {
        let w = WALL[i];
        let m = min((META[i] >> 8u) & 0xFFu, 9u);
        sh_add(F_WALLS, 1.0);
        let st = w.z;
        if (st > 0.3) {
            let s2 = st * st;
            sh_add(F_SW, s2);
            sh_add(F_SWF, s2 * AC_F1[m]);
            sh_add(F_SWQ, s2 * AC_Q[m]);
        }
        sh_max(F_SMAX, st);
        if (w.y > 0.9) {
            sh_add(F_FAIL, 1.0);
            sh_add(F_FAILF, AC_F1[m]);
        }
        if (m == 7u || m == 8u) {
            var exposed = false;
            for (var k = 0; k < 4; k++) {
                let o = AC_OFF[k];
                let j = ac_cid(x + o.x, y + o.y);
                if (j != i && !ac_solid(ac_tile(j))) { exposed = true; }
            }
            if (exposed && w.w > 0.0) {
                if (m == 7u && w.x > 1.8) {
                    sh_add(F_WOODN, 1.0);
                    sh_add(F_WOODR, K_WOOD * exp(-T_WOOD / w.x));
                }
                if (m == 8u && w.x > T_PROP_IGN) { sh_add(F_PROPN, 1.0); }
            }
        }
        return;
    }
    if (t == 8u) {
        let w = WALL[i];
        let kind = floor(w.z / 1000.0 + 1e-3);
        let rate = max(w.z - kind * 1000.0, 0.0);
        if (kind == 3.0 || kind == 4.0) { sh_add(F_MBN, 1.0); sh_add(F_MBR, rate); }
        if (kind == 1.0) { sh_add(F_BOILN, 1.0); sh_add(F_BOILR, rate); }
        if (kind == 2.0 || kind == 5.0 || kind == 6.0 || kind == 7.0) { sh_add(F_FIZN, 1.0); sh_add(F_FIZR, rate); }
        if (w.y >= 1.95 && w.y < 2.5) { sh_add(F_DETN, 1.0); }
        return;
    }
    if (t == 6u || t == 2u || t == 3u || t == 4u || t == 7u) { return; }
    // gas cells: checkerboard, alternating every frame, weight 2 (halves the cost; the synth smooths over frames)
    if (((u32(x) + u32(y) + A.frame) & 1u) != 0u) { return; }
    WGT = 2.0;
    // gas (0) or fan (5)
    let s = ac_ld(i);
    let rho = ac_rho(s);
    let p = ac_p(s);
    let tt = ac_t(s);
    let u = vec2<f32>(s.b.x, s.b.y) / rho;
    let spd = length(u);
    sh_max(F_PMAX, p);
    sh_max(F_UMAX, spd);

    // heat release (combustion monopole source)
    let q = Q_FUEL * ac_burn(s);
    if (q > 0.0) {
        sh_add(F_Q, q);
        sh_add(F_QX, q * dx);
        sh_add(F_QY, q * dy);
        sh_add(F_QXX, q * dx * dx);
        sh_add(F_QYY, q * dy * dy);
        if (q > A.burn_thr) {
            sh_add(F_FLN, 1.0);
            sh_add(F_FLT, tt);
        }
    }

    // neighbours (each loaded once): velocity gradients (no-slip at solids), pressure gradient, inlet adjacency
    let ie = ac_cid(x + 1, y);
    let iw = ac_cid(x - 1, y);
    let is = ac_cid(x, y + 1);
    let inn = ac_cid(x, y - 1);
    let te = ac_tile(ie);
    let tw = ac_tile(iw);
    let ts = ac_tile(is);
    let tn = ac_tile(inn);
    let se = ac_ld(ie);
    let sw = ac_ld(iw);
    let ss = ac_ld(is);
    let sn = ac_ld(inn);
    let ue = select(vec2<f32>(se.b.x, se.b.y) / ac_rho(se), vec2<f32>(0.0), ac_solid(te));
    let uw = select(vec2<f32>(sw.b.x, sw.b.y) / ac_rho(sw), vec2<f32>(0.0), ac_solid(tw));
    let us = select(vec2<f32>(ss.b.x, ss.b.y) / ac_rho(ss), vec2<f32>(0.0), ac_solid(ts));
    let un = select(vec2<f32>(sn.b.x, sn.b.y) / ac_rho(sn), vec2<f32>(0.0), ac_solid(tn));
    let gx = 0.5 * (ue - uw);
    let gy = 0.5 * (us - un);
    let sxy = gy.x + gx.y;
    let smag = sqrt(2.0 * (gx.x * gx.x + gy.y * gy.y) + sxy * sxy);
    let vort = abs(gx.y - gy.x);
    sh_add(F_VORT, vort);

    let c = sqrt((1.0 + ac_gm1(s)) * p / rho);
    // Proudman (1952) acoustic power of turbulence per unit volume: P = alpha rho eps Mt^5, using the
    // Smagorinsky sub-grid turbulence of the solver's LES (nu_t = CS2 |S|, eps = nu_t |S|^2, u' = sqrt(nu_t |S|))
    let nu = min(CS2 * smag, NU_T_MAX);
    let eps = nu * smag * smag;
    let up = sqrt(nu * smag);
    let mt = min(up / max(c, 1e-3), 1.0);
    let mt2 = mt * mt;
    let pr = rho * eps * mt2 * mt2 * mt;
    // only in fast (pressure-driven) flow: buoyant / flame-driven flow speeds are inflated by the game-clock
    // chemistry and their noise is the combustion monopole, counted above
    // jets (Lighthill U^8 sources): fast AND high local Mach number (excludes the thermal-expansion flow of flames)
    let jet = spd > A.jet_u && spd > A.jet_m * c;
    // (jet cells are excluded: their fine-scale turbulence noise is Tam's fine-scale component of the jet model)
    // fans and the cells they force are excluded too: the fan model (blade passing + dipole broadband) covers them
    let near_fan = t == 5u || te == 5u || tw == 5u || ts == 5u || tn == 5u;
    if (pr > 0.0 && spd > 0.5 * A.jet_u && !jet && !near_fan) {
        sh_add(F_PR, pr);
        sh_add(F_PRX, pr * dx);
        sh_add(F_PRY, pr * dy);
        sh_add(F_PRU, pr * up);
    }

    if (jet) {
        let u3 = spd * spd * spd;
        sh_add(F_JN, 1.0);
        sh_add(F_JKE, 0.5 * rho * u3);
        sh_add(F_JU4, rho * u3 * spd);
        sh_add(F_JRHO, rho);
        sh_add(F_JX, dx);
        sh_add(F_JY, dy);
        sh_add(F_JXX, dx * dx);
        sh_add(F_JYY, dy * dy);
        sh_add(F_JXY, dx * dy);
    }

    // shocks: relative pressure jump across the cell
    let pe = select(ac_p(se), p, ac_solid(te));
    let pw = select(ac_p(sw), p, ac_solid(tw));
    let ps = select(ac_p(ss), p, ac_solid(ts));
    let pn = select(ac_p(sn), p, ac_solid(tn));
    let g = 0.5 * length(vec2<f32>(pe - pw, ps - pn)) / p;
    if (g > 0.15) {
        sh_add(F_SHN, 1.0);
        sh_add(F_SHS, g);
        if (jet) { sh_add(F_SHJ, 1.0); }
    }

    if (t == 5u) {
        sh_add(F_FANN, 1.0);
        sh_add(F_FANU, spd);
    }
    let inlet = (te >= 2u && te <= 4u) || (tw >= 2u && tw <= 4u) || (ts >= 2u && ts <= 4u) || (tn >= 2u && tn <= 4u);
    if (inlet) {
        sh_add(F_INN, 1.0);
        sh_add(F_INF, rho * spd);
        sh_add(F_INR, rho);
    }
}

fn is_max_slot(f: u32) -> bool { return f == F_UMAX || f == F_PMAX || f == F_SMAX; }

@compute @workgroup_size(128)
fn features(@builtin(local_invocation_index) li: u32, @builtin(workgroup_id) wid: vec3<u32>) {
    let b = wid.x;
    let sl = wid.y;
    let fw = A.cam_w / f32(A.nb);
    var x0 = i32(floor(A.cam_x + f32(b) * fw));
    var x1 = i32(floor(A.cam_x + f32(b + 1u) * fw));
    if (b == 0u) { x0 = 0; }
    if (b + 1u == A.nb) { x1 = i32(A.w); }
    x0 = clamp(x0, 0, i32(A.w));
    x1 = clamp(x1, x0, i32(A.w));
    let y0 = i32(sl * A.h / A.ns);
    let y1 = i32((sl + 1u) * A.h / A.ns);
    let bw = x1 - x0;
    let n = bw * (y1 - y0);
    for (var k = i32(li); k < n; k += i32(WG)) {
        cell(x0 + k % bw, y0 + k / bw);
    }
    let out = (sl * A.nb + b) * NF;
    //@REDUCE@
}
