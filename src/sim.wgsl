// Compressible multi-species reacting gas dynamics + thermo-mechanical walls, all on the GPU.
// Units: ambient air is rho=1, p=1 (1 atm), T=1 (300 K), dx=1.

struct Params {
    w: u32, h: u32, nops: u32, view: u32,
    dt: f32, p_fuel: f32, fan_pow: f32, time: f32,
    ign_on: u32, frame: u32, srgb: u32, sflags: u32, // sflags bit0: sparse (active-tile) stepping
    p_o2: f32, p_air: f32, cam_x: f32, cam_y: f32,
    cam_w: f32, cam_h: f32, wall_dt: f32, gravity: f32,
}
// shape: 0 disc at (x0,y0) radius r, 1 filled box, 2 hollow box (thickness r+1), 3 line (half-width r)
struct Op { kind: u32, shape: u32, x0: i32, y0: i32, x1: i32, y1: i32, r: i32, tile: u32, mat: u32, dir: u32, _p0: u32, _p1: u32 }

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var<storage, read_write> U0: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> U1: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> U2: array<vec4<f32>>;
@group(0) @binding(4) var<storage, read_write> META: array<u32>;
@group(0) @binding(5) var<storage, read_write> WALL: array<vec4<f32>>; // (temp, damage, stress, remaining solid fraction); [0,N) current, [N,2N) next
@group(0) @binding(6) var<storage, read> OPS: array<Op>;
// Active-region bookkeeping (perf, phys): per RK tile (RK_TX x RK_TY cells)
//   AUX[t]           calm flag written by `post` (1 = every cell of the tile changed by < CALM_EPS this step, gas only,
//                    below T_CHEM_MIN; 0 = anything else; cleared to 0 by gpu.rs after edits/scene loads)
//   AUX[AUX_ACT + t] active flag for the current step, written by `act` (tile or any of its 8 neighbours not calm)
//   AUX[AUX_SOL + t] solid bits written by `post`: 1 = has wall (tile 1), 2 = has matter (tile 8)
@group(0) @binding(7) var<storage, read_write> AUX: array<u32>;
// 0 max p, 1 max gas T, 2 max wall T/T_melt, 3 burn sum (x1000), 4 wall count, 5 max stress, 6 thrust x (x1000), 7 thrust y (x1000)
@group(0) @binding(8) var<storage, read_write> STATS: array<atomic<u32>>;

const GAMMA: f32 = 1.4;
const RHO_MIN: f32 = 0.002;
const P_MIN: f32 = 0.002;
// Heat of combustion per unit fuel mass, tuned so the open stoichiometric CH4-air flame reaches Tad = 2231 K
// (real 2226 K) and the lean limit is 5.4 % CH4 (real 5 %); see tools/VALIDATION.md §1, §3.
const Q_FUEL: f32 = 485.0;
// One-step Arrhenius: steep enough that premixed CH4-air does not autoignite below ~810 K (real AIT 810 K),
// pre-exponential re-fitted so the laminar flame speed is unchanged (VALIDATION §4).
const T_ACT: f32 = 60.0;
const A_PRE: f32 = 282000.0;
// no gas-phase chemistry below this temperature (2.7 = 810 K, CH4 autoignition temperature)
const T_CHEM_MIN: f32 = 2.7;
const T_IGNITER: f32 = 10.0;
const K_COND: f32 = 0.75;
const D_SPEC: f32 = 0.3;
const HT: f32 = 0.035;
// sigma*eps (eps ~ 0.8) in sim units: 5.67e-8 W/m2K4 * (300 K)^4 * 1 cm2 * 34 us / 0.1013 J = 1.55e-5 per unit T^4
const RAD: f32 = 1.2e-5;
// Gas -> solid radiation: emissivity of the gas cell touching a surface, eps = 1 - exp(-(K_RAD_SOOT*soot + K_RAD_GAS*products))
// (luminous soot dominates; CO2/H2O bands give a small non-luminous part).
const K_RAD_SOOT: f32 = 30.0;
const K_RAD_GAS: f32 = 0.02;
const K_DMG: f32 = 0.25;
// soot: formed by fuel pyrolysis where hot and oxygen-starved, burnt by O2, radiates
const K_PYRO: f32 = 6.0;
const T_PYRO: f32 = 28.0;
const K_SOOT_OX: f32 = 200.0;
const T_SOOT_OX: f32 = 20.0;
const K_SOOT_RAD: f32 = 0.0005;
// turbulence (Smagorinsky LES) and molecular viscosity
const CS2: f32 = 0.06;
const NU_MOL: f32 = 0.0;
const NU_T_MAX: f32 = 1.5;
const PR_T: f32 = 0.7;
// burnable solids (materials 7 wood, 8 solid propellant, 9 explosive)
const SOLID_MASS: f32 = 10.0;     // gas-mass units in one solid cell
const K_WOOD: f32 = 8.0;          // wood pyrolysis rate per exposed face
const T_WOOD: f32 = 12.0;         // pyrolysis activation (3600 K)
const K_PROP: f32 = 0.2;         // propellant burn rate per face at 1 atm (Saint-Robert's law r = a p^n)
const N_PROP: f32 = 0.35;
const T_PROP_IGN: f32 = 2.5;      // 750 K surface ignition
const T_PROP_FLAME: f32 = 10.0;   // 3000 K exhaust
const T_EXPL_IGN: f32 = 2.0;      // 600 K
const P_EXPL_SHOCK: f32 = 8.0;    // shock sensitivity (atm)
const P_DET: f32 = 60.0;          // detonation products pressure
const T_DET: f32 = 11.5;          // ~3450 K
// mixing-controlled combustion (Eddy Dissipation Concept, as in NIST FDS): once hot, fuel and O2
// burn as fast as diffusion/turbulence mixes them
const C_EDC: f32 = 4.0;
const T_EDC: f32 = 4.0;
const V_MAX: f32 = 10.0;
// Thermal dissociation (CO2/H2O -> CO, H2, OH, O ...) limits real flame temperatures: heat release is equilibrium-
// limited between T_DISS0 (2400 K, no effect on fuel-air flames) and T_DISS1 (3600 K). Fuel and O2 that cannot
// react stay in the gas (stand-in for the dissociated species) and recombine/burn once the gas cools, e.g. in the
// nozzle, so energy is conserved. T_MAX is only a numerical safety net far above anything physical.
const T_DISS0: f32 = 8.0;
const T_DISS1: f32 = 12.0;
const T_MAX: f32 = 30.0;
const RS = vec4<f32>(1.80625, 0.903125, 1.032142857, 1.111538462);
// specific heats at constant volume per species (same units as RS): CH4 (gamma 1.31), O2 and N2 (1.40),
// hot CO2/H2O exhaust (1.25). The mixture's gamma = 1 + R/cv follows the local composition.
const CV = vec4<f32>(5.827, 2.258, 2.580, 4.446);

// materials: 0 steel, 1 aluminium, 2 copper, 3 titanium, 4 inconel, 5 ceramic, 6 cooled copper,
// 7 wood (pyrolyses into fuel gas), 8 solid propellant (self-oxidising, pressure-dependent burn), 9 explosive
const M_STR: array<f32, 26> = array<f32, 26>(4.0, 2.0, 1.5, 6.5, 5.0, 1.2, 3.0, 1.5, 1.2, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0);
// real melting points (K/300): steel 1740, Al 933, Cu 1358, Ti-6Al-4V 1920, Inconel 1590, ceramic (zirconia-class) 2850,
// cooled Cu 1358; softening = ~40-50 % loss of yield strength
const M_TSOFT: array<f32, 26> = array<f32, 26>(2.6, 1.7, 2.0, 2.5, 3.3, 7.0, 2.0, 3.0, 99.0, 99.0, 99.0, 99.0, 99.0, 99.0, 99.0, 99.0, 99.0, 99.0, 99.0, 99.0, 99.0, 99.0, 99.0, 99.0, 99.0, 99.0);
const M_TMELT: array<f32, 26> = array<f32, 26>(5.8, 3.11, 4.53, 6.4, 5.3, 9.5, 4.53, 99.0, 99.0, 99.0, 99.0, 99.0, 99.0, 99.0, 99.0, 99.0, 99.0, 99.0, 99.0, 99.0, 99.0, 99.0, 99.0, 99.0, 99.0, 99.0);
const M_COND: array<f32, 26> = array<f32, 26>(1.0, 2.6, 3.6, 0.5, 0.35, 0.12, 3.6, 0.15, 0.2, 0.2, 0.3, 0.1, 0.2, 0.5, 0.2, 2.0, 0.3, 0.5, 0.3, 0.6, 0.15, 0.17, 0.15, 0.2, 0.8, 0.1);
const M_CAP: array<f32, 26> = array<f32, 26>(12.0, 9.0, 11.0, 10.0, 14.0, 16.0, 11.0, 10.0, 10.0, 10.0, 8.0, 8.0, 8.0, 8.0, 8.0, 9.0, 8.0, 7.0, 8.0, 14.0, 9.0, 10.0, 8.0, 9.0, 6.0, 8.0);
// active cooling (regenerative coolant channels): heat removed per unit (T - 300 K)
const M_COOL: array<f32, 26> = array<f32, 26>(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 10.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
const M_SHOCK: array<f32, 26> = array<f32, 26>(1e9, 1e9, 1e9, 1e9, 1e9, 2.2, 1e9, 1e9, 1e9, 1e9, 1e9, 1e9, 1e9, 1e9, 1e9, 1e9, 1e9, 1e9, 1e9, 1e9, 1e9, 1e9, 1e9, 1e9, 1e9, 1e9);
const M_COL: array<vec3<f32>, 26> = array<vec3<f32>, 26>(
    vec3<f32>(0.47, 0.49, 0.53), vec3<f32>(0.76, 0.78, 0.82), vec3<f32>(0.75, 0.43, 0.27),
    vec3<f32>(0.58, 0.60, 0.68), vec3<f32>(0.36, 0.43, 0.47), vec3<f32>(0.88, 0.84, 0.76),
    vec3<f32>(0.62, 0.40, 0.30), vec3<f32>(0.45, 0.30, 0.16), vec3<f32>(0.70, 0.62, 0.60),
    vec3<f32>(0.85, 0.55, 0.25),
    // matter elements 10-25
    vec3<f32>(0.80, 0.70, 0.45), vec3<f32>(0.13, 0.12, 0.11), vec3<f32>(0.90, 0.84, 0.20),
    vec3<f32>(0.93, 0.93, 0.95), vec3<f32>(0.96, 0.93, 0.86), vec3<f32>(0.75, 0.77, 0.80),
    vec3<f32>(0.84, 0.90, 0.98), vec3<f32>(0.56, 0.20, 0.12), vec3<f32>(0.97, 0.96, 0.90),
    vec3<f32>(0.15, 0.35, 0.85), vec3<f32>(0.76, 0.55, 0.20), vec3<f32>(0.80, 0.86, 0.92),
    vec3<f32>(0.55, 0.75, 1.00), vec3<f32>(0.95, 0.90, 0.60), vec3<f32>(1.00, 0.45, 0.10),
    vec3<f32>(0.45, 0.44, 0.42));
const OFF: array<vec2<i32>, 4> = array<vec2<i32>, 4>(
    vec2<i32>(1, 0), vec2<i32>(-1, 0), vec2<i32>(0, 1), vec2<i32>(0, -1));
// OFF[k] as an offset in the post pass's SH_NU tile (row length PO_NX = 18)
const NU_OFF: array<i32, 4> = array<i32, 4>(1, -1, 18, -18);

struct S { a: vec4<f32>, b: vec4<f32> } // a = species densities, b = (mx, my, E, soot density)
struct Prim { r: vec4<f32>, w: vec4<f32> } // w = (u, v, p, soot)

fn cid(x: i32, y: i32) -> u32 {
    let cx = clamp(x, 0, i32(P.w) - 1);
    let cy = clamp(y, 0, i32(P.h) - 1);
    return u32(cy) * P.w + u32(cx);
}
fn tile_of(i: u32) -> u32 { return META[i] & 0xFFu; }
fn mat_of(i: u32) -> u32 { return min((META[i] >> 8u) & 0xFFu, 25u); }
fn dir_of(i: u32) -> u32 { return (META[i] >> 16u) & 0xFFu; }
fn solid_t(t: u32) -> bool { return t == 1u || t == 6u || t == 8u; }
fn is_solid(i: u32) -> bool { return solid_t(tile_of(i)); }

// ---- active-region (sparse) stepping, see AUX above ----
const AUX_ACT: u32 = 8192u;
const AUX_SOL: u32 = 16384u;
// relative change per step below which a gas cell counts as calm (f32 round-off of a uniform state is ~1e-7)
const CALM_EPS: f32 = 1e-6;
fn ntx() -> u32 { return (P.w + RK_TX - 1u) / RK_TX; }
fn nty() -> u32 { return (P.h + RK_TY - 1u) / RK_TY; }
// is the tile holding (clamped) cell x,y stepped this step?
fn act_xy(x: i32, y: i32) -> bool {
    let cx = u32(clamp(x, 0, i32(P.w) - 1));
    let cy = u32(clamp(y, 0, i32(P.h) - 1));
    return AUX[AUX_ACT + (cy / RK_TY) * ntx() + cx / RK_TX] != 0u;
}
fn act_i(i: u32) -> bool { return AUX[AUX_ACT + ((i / P.w) / RK_TY) * ntx() + (i % P.w) / RK_TX] != 0u; }
// U2 of a neighbour: cells of skipped tiles hold stale scratch (vis reuses U1/U2), their state is U0
fn ld2a(i: u32) -> S { if (act_i(i)) { return ld2(i); } return ld0(i); }
var<workgroup> WG_ACT: u32;
// workgroup-uniform: is this RK tile stepped? (contains a barrier; call first, from uniform control flow)
fn wg_active(li: u32, wid: vec3<u32>) -> bool {
    if (li == 0u) { WG_ACT = AUX[AUX_ACT + wid.y * ntx() + wid.x]; }
    return workgroupUniformLoad(&WG_ACT) != 0u;
}
// Per-step active mask: a tile is stepped unless it and its 8 neighbours are all calm (the one-tile halo is wider than
// the solver's per-step domain of dependence: 2 cells per RK stage + 2 for post, so no signal can outrun it).
@compute @workgroup_size(64)
fn act(@builtin(global_invocation_id) g: vec3<u32>) {
    let nx = ntx();
    let ny = nty();
    if (g.x >= nx * ny) { return; }
    var a = 1u;
    if ((P.sflags & 1u) != 0u) {
        a = 0u;
        let tx = i32(g.x % nx);
        let ty = i32(g.x / nx);
        for (var dy = -1; dy <= 1; dy++) {
            for (var dx = -1; dx <= 1; dx++) {
                let x = tx + dx;
                let y = ty + dy;
                if (x < 0 || y < 0 || x >= i32(nx) || y >= i32(ny)) { continue; }
                if (AUX[u32(y) * nx + u32(x)] == 0u) { a = 1u; }
            }
        }
    }
    AUX[AUX_ACT + g.x] = a;
}

fn ld0(i: u32) -> S { return S(U0[2u * i], U0[2u * i + 1u]); }
fn ld1(i: u32) -> S { return S(U1[2u * i], U1[2u * i + 1u]); }
fn ld2(i: u32) -> S { return S(U2[2u * i], U2[2u * i + 1u]); }
fn st0(i: u32, s: S) { U0[2u * i] = s.a; U0[2u * i + 1u] = s.b; }
fn st1(i: u32, s: S) { U1[2u * i] = s.a; U1[2u * i + 1u] = s.b; }
fn st2(i: u32, s: S) { U2[2u * i] = s.a; U2[2u * i + 1u] = s.b; }

fn rho_s(s: S) -> f32 { return s.a.x + s.a.y + s.a.z + s.a.w; }
fn rmix(s: S) -> f32 { return dot(s.a, RS) / max(rho_s(s), RHO_MIN); }
fn cvmix(s: S) -> f32 { return dot(s.a, CV) / max(rho_s(s), RHO_MIN); }
// R/cv = gamma - 1 of the mixture
fn gm1(s: S) -> f32 { return dot(s.a, RS) / max(dot(s.a, CV), 1e-6); }
fn gm1_r(r: vec4<f32>) -> f32 { return dot(r, RS) / max(dot(r, CV), 1e-6); }
fn pres(s: S) -> f32 {
    let rho = max(rho_s(s), RHO_MIN);
    let ke = 0.5 * (s.b.x * s.b.x + s.b.y * s.b.y) / rho;
    return max(gm1(s) * (s.b.z - ke), P_MIN);
}
fn temp(s: S) -> f32 {
    let rho = max(rho_s(s), RHO_MIN);
    return pres(s) / (rho * rmix(s));
}
fn gas_state(y: vec4<f32>, p: f32, t: f32) -> S {
    let rm = dot(y, RS);
    let rho = p / (rm * t);
    return S(y * rho, vec4<f32>(0.0, 0.0, p * dot(y, CV) / rm, 0.0));
}
fn air_state(p: f32, t: f32) -> S { return gas_state(vec4<f32>(0.0, 0.233, 0.767, 0.0), p, t); }
fn set_temp(s: S, t: f32) -> S {
    var r = s;
    let rho = max(rho_s(s), RHO_MIN);
    let ke = 0.5 * (s.b.x * s.b.x + s.b.y * s.b.y) / rho;
    r.b.z = ke + rho * cvmix(s) * t;
    return r;
}
fn floor_state(s: S) -> S {
    var r = s;
    r.a = select(vec4<f32>(0.0), r.a, r.a > vec4<f32>(0.0));
    let rho0 = rho_s(r);
    if (rho0 < RHO_MIN) { r.a.z += RHO_MIN - rho0; }
    let rho = rho_s(r);
    var m = vec2<f32>(r.b.x, r.b.y);
    if (!(m.x == m.x) || !(m.y == m.y)) { m = vec2<f32>(0.0); }
    let ke_old = 0.5 * dot(m, m) / rho;
    let sp = length(m) / rho;
    var e = r.b.z;
    if (!(e == e)) { e = ke_old + P_MIN / gm1(r); }
    if (sp > V_MAX) {
        let internal = e - ke_old;
        m = m * (V_MAX / sp);
        e = internal + 0.5 * dot(m, m) / rho;
    }
    let ke = 0.5 * dot(m, m) / rho;
    let emin = ke + P_MIN / gm1(r);
    if (e < emin) { e = emin; }
    r.b.x = m.x; r.b.y = m.y; r.b.z = e;
    r.b.w = select(0.0, r.b.w, r.b.w > 0.0);
    let t = temp(r);
    if (t > T_MAX) { r = set_temp(r, T_MAX); }
    return r;
}

// ---- boundary conditions ----
// Inlets (tiles 2/3/4) are subsonic stagnation-state boundaries (reservoir at p0, T0): the cell keeps the velocity
// the flow solver gave it (|M| <= 0.9) and its static state follows isentropically, T = T0 - u^2/(2 cp),
// p = p0 (T/T0)^(g/(g-1)). The old "gas at rest at p0" state cost 25-30 % of the stagnation pressure.
fn inlet_state(y: vec4<f32>, p0: f32, t0: f32, s: S) -> S {
    let r = max(rho_s(s), RHO_MIN);
    var u = vec2<f32>(s.b.x, s.b.y) / r;
    let rm = dot(y, RS);
    let cvm = dot(y, CV);
    let cp = cvm + rm;
    let g = cp / cvm;
    let tmin = t0 / (1.0 + 0.5 * (g - 1.0) * 0.81); // M = 0.9
    let q2 = dot(u, u);
    var t = t0 - 0.5 * q2 / cp;
    if (t < tmin) {
        t = tmin;
        u = u * sqrt(2.0 * cp * (t0 - tmin) / max(q2, 1e-12));
    }
    let p = p0 * pow(t / t0, g / (g - 1.0));
    var ns = gas_state(y, p, t);
    let rho = rho_s(ns);
    ns.b.x = rho * u.x;
    ns.b.y = rho * u.y;
    ns.b.z += 0.5 * rho * dot(u, u);
    return ns;
}
// Vents (tile 7) are static-pressure outlets at 1 atm: the gas keeps its composition, velocity and temperature
// (zero-gradient), only the pressure is set (no 0.2-0.5 atm back-pressure from a "gas at rest" state). The cell
// relaxes slowly towards ambient air so a vent that only ever sees inflow breathes clean air.
fn vent_state(s: S) -> S {
    let r = max(rho_s(s), RHO_MIN);
    let u = vec2<f32>(s.b.x, s.b.y) / r;
    let y = mix(s.a / r, vec4<f32>(0.0, 0.233, 0.767, 0.0), 0.02);
    let t = mix(clamp(temp(s), 0.3, T_MAX), 1.0, 0.02);
    var ns = gas_state(y, 1.0, t);
    let rho = rho_s(ns);
    ns.b.x = rho * u.x;
    ns.b.y = rho * u.y;
    ns.b.z += 0.5 * rho * dot(u, u);
    ns.b.w = s.b.w / r * rho * 0.98;
    return ns;
}

// ---- conservation accounting ----
// STATS slots 8/9: cumulative mass / total energy that entered the domain through boundaries (inlets, vents,
// domain edges); 10/11: mass / energy added by physical sources (walls and matter heat exchange and emission,
// fans, igniters, soot radiation, burnt-through walls, user edits). Fixed point x ACC_SCALE as wrapping i32,
// never cleared per frame (gpu.rs unwraps the deltas into f64). "Energy" is E + chemical energy
// (Q_FUEL per unit fuel, 2 Q_FUEL per unit soot), so combustion itself is not a source.
const ACC_SCALE: f32 = 1000.0;
fn en_tot(s: S) -> f32 { return s.b.z + Q_FUEL * s.a.x + 2.0 * Q_FUEL * s.b.w; }
fn mass_tot(s: S) -> f32 { return rho_s(s); }
// per-thread partial sums (bnd mass, bnd energy, src mass, src energy), reduced per workgroup
var<workgroup> RED: array<vec4<f32>, 128>;
// (no module-scope var<private>: gpu.rs rewrites those into consts, see ARCHITECTURE.md)
fn bnd_acc(after: S, before: S) -> vec4<f32> {
    return vec4<f32>(mass_tot(after) - mass_tot(before), en_tot(after) - en_tot(before), 0.0, 0.0);
}
fn src_acc(after: S, before: S) -> vec4<f32> {
    return vec4<f32>(0.0, 0.0, mass_tot(after) - mass_tot(before), en_tot(after) - en_tot(before));
}
fn acc_hash(v: u32) -> f32 {
    var x = v * 747796405u + 2891336453u;
    x = ((x >> ((x >> 28u) + 4u)) ^ x) * 277803737u;
    x = (x >> 22u) ^ x;
    return f32(x) * (1.0 / 4294967296.0);
}
// unbiased (dithered) fixed-point add into a cumulative STATS slot
fn acc_global(k: u32, v: f32, seed: u32) {
    if (v == 0.0) { return; }
    let q = i32(floor(v * ACC_SCALE + acc_hash(seed * 4u + k + P.frame * 7919u)));
    if (q != 0) { atomicAdd(&STATS[k], bitcast<u32>(q)); }
}
// a solid cell turned into gas (per-thread, rare)
fn src_global(d: S, i: u32) {
    acc_global(10u, mass_tot(d), i);
    acc_global(11u, en_tot(d), i);
}
// workgroup tree reduction of v (128 threads) and one global add per slot; call in uniform control flow
fn acc_flush(li: u32, seed: u32, v: vec4<f32>) {
    RED[li] = v;
    workgroupBarrier();
    for (var st = 64u; st > 0u; st = st >> 1u) {
        if (li < st) { RED[li] += RED[li + st]; }
        workgroupBarrier();
    }
    if (li == 0u) {
        let r = RED[0];
        acc_global(8u, r.x, seed);
        acc_global(9u, r.y, seed);
        acc_global(10u, r.z, seed);
        acc_global(11u, r.w, seed);
    }
}

fn to_prim(s: S) -> Prim {
    let rho = max(rho_s(s), RHO_MIN);
    let vx = s.b.x / rho;
    let vy = s.b.y / rho;
    let ke = 0.5 * (s.b.x * vx + s.b.y * vy);
    let p = max(gm1(s) * (s.b.z - ke), P_MIN);
    return Prim(s.a, vec4<f32>(vx, vy, p, max(s.b.w, 0.0)));
}
fn ldprim(i: u32, stage: u32) -> Prim {
    if (stage == 0u) { return to_prim(ld0(i)); }
    return to_prim(ld1(i));
}

fn minmod4(a: vec4<f32>, b: vec4<f32>) -> vec4<f32> {
    let m = select(b, a, abs(a) < abs(b));
    return select(m, vec4<f32>(0.0), a * b <= vec4<f32>(0.0));
}
fn minmod3(a: vec4<f32>, b: vec4<f32>) -> vec4<f32> {
    let m = select(b, a, abs(a) < abs(b));
    return select(m, vec4<f32>(0.0), a * b <= vec4<f32>(0.0));
}

fn hllc(l: Prim, r: Prim) -> S {
    let rl = l.r.x + l.r.y + l.r.z + l.r.w;
    let rr = r.r.x + r.r.y + r.r.z + r.r.w;
    let ul = l.w.x; let vl = l.w.y; let pl = l.w.z;
    let ur = r.w.x; let vr = r.w.y; let pr = r.w.z;
    let gl = gm1_r(l.r);
    let gr = gm1_r(r.r);
    let el = pl / gl + 0.5 * rl * (ul * ul + vl * vl);
    let er = pr / gr + 0.5 * rr * (ur * ur + vr * vr);
    let cl = sqrt((1.0 + gl) * pl / rl);
    let cr = sqrt((1.0 + gr) * pr / rr);
    let sl = min(ul - cl, ur - cr);
    let sr = max(ul + cl, ur + cr);
    let zl = l.w.w;
    let zr = r.w.w;
    // Perf: only the selected HLL region's flux is built (same formulas as the full form, so the result is
    // unchanged; divergent lanes still run each branch at most once).
    if (sl >= 0.0) {
        return S(l.r * ul, vec4<f32>(rl * ul * ul + pl, rl * ul * vl, (el + pl) * ul, zl * ul));
    }
    if (sr <= 0.0) {
        return S(r.r * ur, vec4<f32>(rr * ur * ur + pr, rr * ur * vr, (er + pr) * ur, zr * ur));
    }
    let sm = (pr - pl + rl * ul * (sl - ul) - rr * ur * (sr - ur)) / (rl * (sl - ul) - rr * (sr - ur));
    if (sm >= 0.0) {
        let fla = l.r * ul;
        let flb = vec4<f32>(rl * ul * ul + pl, rl * ul * vl, (el + pl) * ul, zl * ul);
        let kl = (sl - ul) / (sl - sm);
        let usla = l.r * kl;
        let uslb = vec4<f32>(rl * kl * sm, rl * kl * vl, kl * (el + (sm - ul) * (rl * sm + pl / (sl - ul))), zl * kl);
        let ucl = vec4<f32>(rl * ul, rl * vl, el, zl);
        return S(fla + sl * (usla - l.r), flb + sl * (uslb - ucl));
    }
    let fra = r.r * ur;
    let frb = vec4<f32>(rr * ur * ur + pr, rr * ur * vr, (er + pr) * ur, zr * ur);
    let kr = (sr - ur) / (sr - sm);
    let usra = r.r * kr;
    let usrb = vec4<f32>(rr * kr * sm, rr * kr * vr, kr * (er + (sm - ur) * (rr * sm + pr / (sr - ur))), zr * kr);
    let ucr = vec4<f32>(rr * ur, rr * vr, er, zr);
    return S(fra + sr * (usra - r.r), frb + sr * (usrb - ucr));
}

// MUSCL-reconstructed HLLC flux across one face. a|b are the cells either side, ap / bn the next cells
// outwards (for the slope limiter); s* say whether each cell is solid (reflecting wall).
fn face_p(sap: bool, pap: Prim, sa: bool, wa: Prim, sb: bool, wb: Prim, sbn: bool, pbn: Prim, axis: u32) -> S {
    if (sa && sb) { return S(vec4<f32>(0.0), vec4<f32>(0.0)); }
    var l: Prim;
    var r: Prim;
    if (sa) {
        r = wb;
        l = r;
        if (axis == 0u) { l.w.x = -l.w.x; } else { l.w.y = -l.w.y; }
    } else if (sb) {
        l = wa;
        r = l;
        if (axis == 0u) { r.w.x = -r.w.x; } else { r.w.y = -r.w.y; }
    } else {
        l = wa;
        r = wb;
        let dr = wb.r - wa.r;
        let dw = wb.w - wa.w;
        if (!sap) {
            l.r += 0.5 * minmod4(wa.r - pap.r, dr);
            l.w += 0.5 * minmod3(wa.w - pap.w, dw);
        }
        if (!sbn) {
            r.r -= 0.5 * minmod4(dr, pbn.r - wb.r);
            r.w -= 0.5 * minmod3(dw, pbn.w - wb.w);
        }
    }
    let lw = l.w;
    let rw = r.w;
    let sw = axis == 1u;
    let ln = Prim(l.r, select(lw, vec4<f32>(lw.y, lw.x, lw.z, lw.w), sw));
    let rn = Prim(r.r, select(rw, vec4<f32>(rw.y, rw.x, rw.z, rw.w), sw));
    let f = hllc(ln, rn);
    let fb = f.b;
    return S(f.a, select(fb, vec4<f32>(fb.y, fb.x, fb.z, fb.w), sw));
}

// ---- fused flux + RK stage (perf): each RK_TX x RK_TY workgroup loads the primitive variables of its tile plus a
// 2-cell halo into workgroup memory once, computes every face flux of the tile once, exchanges them through
// workgroup memory and applies the update. Replaces the old flux0/rk1/flux1/rk2 passes and the FLUX round trip
// (same arithmetic as before, results match the old solver).
const RK_TX: u32 = 16u;
const RK_TY: u32 = 8u;
const RK_HX: u32 = 20u; // tile + 2 halo cells on each side
const RK_HY: u32 = 12u;
const RK_SH: u32 = 240u; // RK_HX * RK_HY (>= face counts (RK_TX+1)*RK_TY, RK_TX*(RK_TY+1))
var<workgroup> SH_A: array<vec4<f32>, RK_SH>;
var<workgroup> SH_B: array<vec4<f32>, RK_SH>;
var<workgroup> SH_S: array<u32, RK_SH>;

fn sh_prim(k: u32) -> Prim { return Prim(SH_A[k], SH_B[k]); }
// face between halo cells k-d (a) and k (b) along axis (d = 1 for x, RK_HX for y)
fn sh_face(k: u32, d: u32, axis: u32) -> S {
    let kap = k - 2u * d;
    let ka = k - d;
    let kbn = k + d;
    return face_p(SH_S[kap] != 0u, sh_prim(kap), SH_S[ka] != 0u, sh_prim(ka), SH_S[k] != 0u, sh_prim(k), SH_S[kbn] != 0u, sh_prim(kbn), axis);
}

fn rk_stage(lid: vec3<u32>, wid: vec3<u32>, stage: u32) {
    let ox = i32(wid.x * RK_TX) - 2;
    let oy = i32(wid.y * RK_TY) - 2;
    let li = lid.y * RK_TX + lid.x;
    if (!wg_active(li, wid)) { return; }
    for (var k = li; k < RK_HX * RK_HY; k += RK_TX * RK_TY) {
        let hx = k % RK_HX;
        let hy = k / RK_HX;
        // corners of the halo are never used by a face stencil
        if ((hx < 2u || hx >= RK_TX + 2u) && (hy < 2u || hy >= RK_TY + 2u)) { SH_S[k] = 1u; continue; }
        let gi = cid(ox + i32(hx), oy + i32(hy));
        let sol = is_solid(gi);
        SH_S[k] = u32(sol);
        if (!sol) {
            var stg = stage;
            if (stg == 1u && !act_xy(ox + i32(hx), oy + i32(hy))) { stg = 0u; }
            let p = ldprim(gi, stg);
            SH_A[k] = p.r;
            SH_B[k] = p.w;
        }
    }
    workgroupBarrier();
    let c = (lid.y + 2u) * RK_HX + lid.x + 2u;
    // own left (x) and top (y) faces; the first RK_TY threads also do the tile's right column of x faces,
    // the next RK_TX threads its bottom row of y faces
    // (one call site for the flux function keeps the kernel small)
    var fl: S;
    var ft: S;
    var fe = S(vec4<f32>(0.0), vec4<f32>(0.0));
    let nj = select(2u, 3u, li < RK_TY + RK_TX);
    for (var r = 0u; r < nj; r++) {
        var k = c;
        var d = 1u;
        if (r == 1u) { d = RK_HX; }
        if (r == 2u) {
            if (li < RK_TY) { k = (li + 2u) * RK_HX + RK_TX + 2u; }
            else { k = (RK_TY + 2u) * RK_HX + (li - RK_TY) + 2u; d = RK_HX; }
        }
        let f = sh_face(k, d, select(0u, 1u, d != 1u));
        if (r == 0u) { fl = f; } else if (r == 1u) { ft = f; } else { fe = f; }
    }
    workgroupBarrier();
    // x faces: index row*(RK_TX+1) + col (col 0..RK_TX)
    let xr = RK_TX + 1u;
    SH_A[lid.y * xr + lid.x] = fl.a;
    SH_B[lid.y * xr + lid.x] = fl.b;
    if (li < RK_TY) { SH_A[li * xr + RK_TX] = fe.a; SH_B[li * xr + RK_TX] = fe.b; }
    workgroupBarrier();
    let fr = S(SH_A[lid.y * xr + lid.x + 1u], SH_B[lid.y * xr + lid.x + 1u]);
    workgroupBarrier();
    // y faces: index row*RK_TX + col (row 0..RK_TY)
    SH_A[lid.y * RK_TX + lid.x] = ft.a;
    SH_B[lid.y * RK_TX + lid.x] = ft.b;
    if (li >= RK_TY && li < RK_TY + RK_TX) { SH_A[RK_TY * RK_TX + li - RK_TY] = fe.a; SH_B[RK_TY * RK_TX + li - RK_TY] = fe.b; }
    workgroupBarrier();
    let fb = S(SH_A[(lid.y + 1u) * RK_TX + lid.x], SH_B[(lid.y + 1u) * RK_TX + lid.x]);

    let gx = wid.x * RK_TX + lid.x;
    let gy = wid.y * RK_TY + lid.y;
    // conservation accounting: what flows in through the domain edges (zero-gradient outflow faces). Over the
    // two RK stages the update is U0 + dt/2 (L(U0) + L(U1)), so each stage books dt/2 of its edge fluxes.
    let edge_wg = wid.x == 0u || wid.y == 0u || (wid.x + 1u) * RK_TX >= P.w || (wid.y + 1u) * RK_TY >= P.h;
    if (edge_wg) {
        var fin = S(vec4<f32>(0.0), vec4<f32>(0.0));
        if (gx < P.w && gy < P.h) {
            if (gx == 0u) { fin = S(fin.a + fl.a, fin.b + fl.b); }
            if (gx == P.w - 1u) { fin = S(fin.a - fr.a, fin.b - fr.b); }
            if (gy == 0u) { fin = S(fin.a + ft.a, fin.b + ft.b); }
            if (gy == P.h - 1u) { fin = S(fin.a - fb.a, fin.b - fb.b); }
        }
        let h = 0.5 * P.dt;
        let acc = vec4<f32>(h * (fin.a.x + fin.a.y + fin.a.z + fin.a.w), h * (fin.b.z + Q_FUEL * fin.a.x + 2.0 * Q_FUEL * fin.b.w), 0.0, 0.0);
        workgroupBarrier();
        acc_flush(li, wid.y * 64u + wid.x + stage * 7777u, acc);
    }
    if (gx >= P.w || gy >= P.h) { return; }
    let i = gy * P.w + gx;
    let s0 = ld0(i);
    if (stage == 0u) {
        if (is_solid(i)) { st1(i, s0); return; }
    } else {
        if (is_solid(i)) { st2(i, s0); return; }
    }
    let la = fl.a - fr.a + ft.a - fb.a;
    let lb = fl.b - fr.b + ft.b - fb.b;
    if (stage == 0u) {
        st1(i, floor_state(S(s0.a + P.dt * la, s0.b + P.dt * lb)));
    } else {
        let s1 = ld1(i);
        st2(i, floor_state(S(0.5 * (s0.a + s1.a + P.dt * la), 0.5 * (s0.b + s1.b + P.dt * lb))));
    }
}

// U1 = U0 + dt L(U0)
@compute @workgroup_size(16, 8)
fn rk1(@builtin(local_invocation_id) lid: vec3<u32>, @builtin(workgroup_id) wid: vec3<u32>) { rk_stage(lid, wid, 0u); }

// U2 = (U0 + U1 + dt L(U1)) / 2
@compute @workgroup_size(16, 8)
fn rk2(@builtin(local_invocation_id) lid: vec3<u32>, @builtin(workgroup_id) wid: vec3<u32>) { rk_stage(lid, wid, 1u); }

fn vel2(i: u32) -> vec2<f32> {
    if (is_solid(i)) { return vec2<f32>(0.0); } // no-slip
    let s = ld2a(i);
    return vec2<f32>(s.b.x, s.b.y) / max(rho_s(s), RHO_MIN);
}

// Smagorinsky eddy viscosity from the resolved strain rate. (perf) Computed inside `post` in workgroup memory:
// the post workgroup (RK_TX x RK_TY) loads velocities on a 2-cell halo and evaluates nu on a 1-cell halo,
// which replaces the old separate `turb` pass and its FLUX round trip (same arithmetic).
const PO_VX: u32 = 20u; // RK_TX + 4
const PO_VY: u32 = 12u; // RK_TY + 4
const PO_NX: u32 = 18u; // RK_TX + 2
const PO_NY: u32 = 10u; // RK_TY + 2
var<workgroup> SH_V: array<vec2<f32>, 240>;  // PO_VX * PO_VY velocities (0 in solids)
var<workgroup> SH_VS: array<u32, 240>;       // solid flags
var<workgroup> SH_NU: array<f32, 180>;       // PO_NX * PO_NY eddy viscosity

fn post_turb(lid: vec3<u32>, wid: vec3<u32>) {
    let ox = i32(wid.x * RK_TX) - 2;
    let oy = i32(wid.y * RK_TY) - 2;
    let li = lid.y * RK_TX + lid.x;
    for (var k = li; k < PO_VX * PO_VY; k += RK_TX * RK_TY) {
        let gi = cid(ox + i32(k % PO_VX), oy + i32(k / PO_VX));
        let sol = is_solid(gi);
        SH_VS[k] = u32(sol);
        SH_V[k] = vel2(gi);
    }
    workgroupBarrier();
    for (var k = li; k < PO_NX * PO_NY; k += RK_TX * RK_TY) {
        let c = (k / PO_NX + 1u) * PO_VX + k % PO_NX + 1u;
        var nu = 0.0;
        if (SH_VS[c] == 0u) {
            let ue = SH_V[c + 1u];
            let uw = SH_V[c - 1u];
            let us = SH_V[c + PO_VX];
            let un = SH_V[c - PO_VX];
            let dx = 0.5 * (ue - uw);
            let dy = 0.5 * (us - un);
            let sxy = dy.x + dx.y;
            let smag = sqrt(2.0 * (dx.x * dx.x + dy.y * dy.y) + sxy * sxy);
            nu = min(CS2 * smag, NU_T_MAX);
        }
        SH_NU[k] = nu;
    }
    workgroupBarrier();
}

// Diffusion, inlets, fans, igniters, gas<->wall heat exchange and combustion.
@compute @workgroup_size(16, 8)
fn post(@builtin(local_invocation_id) lid: vec3<u32>, @builtin(workgroup_id) wid: vec3<u32>) {
    let li = lid.y * RK_TX + lid.x;
    if (li == 0u) { atomicStore(&WG_FL, 0u); }
    if (!wg_active(li, wid)) { return; }
    post_turb(lid, wid);
    let acc = post_cell(lid, wid);
    workgroupBarrier();
    //ACCX
    if (li == 0u) {
        let f = atomicLoad(&WG_FL);
        let t = wid.y * ntx() + wid.x;
        AUX[t] = select(1u, 0u, (f & 1u) != 0u);
        AUX[AUX_SOL + t] = f >> 1u;
    }
}
// post's per-workgroup flags: bit0 not calm, bit1 has wall, bit2 has matter
var<workgroup> WG_FL: atomic<u32>;

// Per-cell body of `post`; returns this cell's (boundary mass, boundary energy, source mass, source energy).
fn post_cell(lid: vec3<u32>, wid: vec3<u32>) -> vec4<f32> {
    let g = vec3<u32>(wid.x * RK_TX + lid.x, wid.y * RK_TY + lid.y, 0u);
    // this cell's index in SH_NU; neighbours along OFF[k] are at lc + NU_OFF[k]
    let lc = (lid.y + 1u) * PO_NX + lid.x + 1u;
    if (g.x >= P.w || g.y >= P.h) { return vec4<f32>(0.0); }
    let x = i32(g.x);
    let y = i32(g.y);
    let i = g.y * P.w + g.x;
    let t = tile_of(i);
    var s = ld2(i);
    if (solid_t(t)) {
        atomicOr(&WG_FL, 1u | select(0u, 2u, t == 1u) | select(0u, 4u, t == 8u));
        st0(i, air_state(1.0, 1.0));
        return vec4<f32>(0.0);
    }
    if (t != 0u) { atomicOr(&WG_FL, 1u); }
    if (t >= 2u && t <= 4u) {
        var yk = vec4<f32>(0.0, 0.233, 0.767, 0.0);
        var p0 = P.p_air;
        if (t == 2u) { yk = vec4<f32>(1.0, 0.0, 0.0, 0.0); p0 = P.p_fuel; }
        if (t == 3u) { yk = vec4<f32>(0.0, 1.0, 0.0, 0.0); p0 = P.p_o2; }
        let ns = inlet_state(yk, p0, 1.0, s);
        st0(i, ns);
        return bnd_acc(ns, s);
    }
    if (t == 7u) {
        let ns = vent_state(s);
        st0(i, ns);
        return bnd_acc(ns, s);
    }
    let s_in = s;
    let dt = P.dt;
    let rc = max(rho_s(s), RHO_MIN);
    let tc = temp(s);
    let spd = sqrt(s.b.x * s.b.x + s.b.y * s.b.y) / rc;
    let pc = pres(s);
    let zs = s.b.w;
    let prod_c = s.a.w;
    var da = vec4<f32>(0.0);
    var de = 0.0;
    var dz = 0.0;
    var dm = vec2<f32>(0.0);
    var igniter = false;
    let nu_c = SH_NU[lc];
    let uc = vec2<f32>(s.b.x, s.b.y) / rc;
    let zc = s.b.w / rc;
    for (var k = 0; k < 4; k++) {
        let o = OFF[k];
        let j = cid(x + o.x, y + o.y);
        if (j == i) { continue; }
        let tj = tile_of(j);
        if (tj == 6u) { igniter = true; }
        if (tj == 1u || tj == 8u) {
            let m = mat_of(j);
            let tw = WALL[j].x;
            let q = gas_wall_flux(rc, tc, spd, pc, zs, prod_c, tw);
            let lim = 0.4 * min(rc * cvmix(s), m_cap(select(m, elem_of(j), tj == 8u))) * abs(tc - tw);
            de -= clamp(dt * q.x, -lim, lim);
            if (tj == 8u) { s = absorb_matter_emission(s, j, dt); }
            if (tj == 1u && (m == 7u || m == 8u) && WALL[j].w > 0.0) {
                let dmass = dt * solid_burn_rate(m, tw, pres(s));
                if (m == 7u) {
                    // wood pyrolysis gas (treated as methane) at the wood's temperature, with some smoke
                    s.a.x += dmass;
                    s.b.z += dmass * CV.x * tw;
                    s.b.w += dmass * 0.03;
                } else {
                    // propellant exhaust: mostly products, fuel-rich (afterburns in air), at flame temperature
                    s.a.w += 0.8 * dmass;
                    s.a.x += 0.2 * dmass;
                    s.b.z += dmass * (0.8 * CV.w + 0.2 * CV.x) * T_PROP_FLAME;
                    s.b.w += dmass * 0.004;
                }
            }
        }
        if (tj == 1u || tj == 6u || tj == 8u) {
            // wall friction (no-slip)
            dm -= dt * (NU_MOL + nu_c) * rc * uc;
            continue;
        }
        let sn = ld2a(j);
        let rn = max(rho_s(sn), RHO_MIN);
        let nu_f = 0.5 * (nu_c + SH_NU[u32(i32(lc) + NU_OFF[k])]);
        let rbar = 0.5 * (rc + rn);
        de += dt * min(rc, rn) * (K_COND + 2.8 * nu_f / PR_T) * (temp(sn) - tc);
        let dif = dt * (D_SPEC + nu_f / PR_T) * rbar;
        da += dif * (sn.a / rn - s.a / rc);
        dz += dif * (sn.b.w / rn - zc);
        dm += dt * (NU_MOL + nu_f) * rbar * (vec2<f32>(sn.b.x, sn.b.y) / rn - uc);
    }
    s.a += da;
    s.b.z += de;
    s.b.w += dz;
    {
        // momentum diffusion and wall friction keep the total energy (the kinetic energy they remove is viscous
        // heating), buoyancy does work on the gas (kinetic energy added, internal energy unchanged)
        let rho = max(rho_s(s), RHO_MIN);
        s.b.x += dm.x;
        s.b.y += dm.y;
        let ke0 = 0.5 * (s.b.x * s.b.x + s.b.y * s.b.y) / rho;
        // buoyancy relative to cold air at the same pressure: hot gas and methane rise
        s.b.y += dt * P.gravity * (rho - pres(s));
        let ke1 = 0.5 * (s.b.x * s.b.x + s.b.y * s.b.y) / rho;
        s.b.z += ke1 - ke0;
    }

    if (t == 5u) {
        let d = dir_of(i);
        var dv = vec2<f32>(0.0, -1.0);
        if (d == 0u) { dv = vec2<f32>(1.0, 0.0); }
        else if (d == 1u) { dv = vec2<f32>(0.0, 1.0); }
        else if (d == 2u) { dv = vec2<f32>(-1.0, 0.0); }
        let rho = max(rho_s(s), RHO_MIN);
        let ke0 = 0.5 * (s.b.x * s.b.x + s.b.y * s.b.y) / rho;
        s.b.x += dt * rho * P.fan_pow * dv.x;
        s.b.y += dt * rho * P.fan_pow * dv.y;
        let ke1 = 0.5 * (s.b.x * s.b.x + s.b.y * s.b.y) / rho;
        s.b.z += ke1 - ke0;
    }
    if (igniter && P.ign_on != 0u) {
        let tt = temp(s);
        if (tt < T_IGNITER) { s = set_temp(s, tt + (T_IGNITER - tt) * min(3.0 * dt, 1.0)); }
    }
    // one-step methane oxidation CH4 + 2 O2 -> CO2 + 2 H2O (high activation energy: only fast near flame temperature)
    if (s.a.x > 1e-6 && s.a.y > 1e-6) {
        let tt = temp(s);
        if (tt > T_CHEM_MIN) {
            let w = reaction_rate(s, tt, nu_c) * diss_factor(tt);
            // equilibrium limit: never release more heat than takes the cell to T_DISS1
            let db_eq = max(T_DISS1 - tt, 0.0) * max(rho_s(s), RHO_MIN) * cvmix(s) / Q_FUEL;
            let db = min(min(min(w * dt, s.a.x), s.a.y * 0.25), db_eq);
            s.a.x -= db;
            s.a.y -= 4.0 * db;
            s.a.w += 5.0 * db;
            s.b.z += Q_FUEL * db;
        }
    }
    {
        let tt = temp(s);
        // pyrolysis: hot fuel without enough oxygen cracks into soot (+ gas)
        if (tt > 3.0 && s.a.x > 1e-6) {
            let starved = clamp(1.0 - s.a.y / max(4.0 * s.a.x, 1e-6), 0.0, 1.0);
            let dp = min(s.a.x, dt * K_PYRO * s.a.x * exp(-T_PYRO / tt) * starved);
            s.a.x -= dp;
            s.a.w += dp;
            s.b.w += 0.5 * dp;
        }
        // soot oxidation: releases the rest of the fuel's heat
        if (s.b.w > 1e-7 && s.a.y > 1e-6 && tt > 1.5) {
            let dso_eq = max(T_DISS1 - tt, 0.0) * max(rho_s(s), RHO_MIN) * cvmix(s) / (2.0 * Q_FUEL);
            let dso = min(min(min(s.b.w, s.a.y / 8.0), dt * K_SOOT_OX * s.b.w * s.a.y * exp(-T_SOOT_OX / tt) * diss_factor(tt)), dso_eq);
            s.b.w -= dso;
            s.a.y -= 8.0 * dso;
            s.a.w += 8.0 * dso;
            s.b.z += 2.0 * Q_FUEL * dso;
        }
        // soot radiates heat away (keeps sooty flames from overheating, and cools smoke)
        let t2 = tt * tt;
        let rho = max(rho_s(s), RHO_MIN);
        let room = max(tt - 1.0, 0.0) * rho * cvmix(s) * 0.02;
        s.b.z -= min(dt * K_SOOT_RAD * s.b.w * max(t2 * t2 - 1.0, 0.0) * 10.0, room);
    }
    // sources = everything post changed (pairwise diffusion terms cancel in the sum); the final floor_state
    // clamp is numerical and deliberately left out, so it shows up as drift
    let acc = src_acc(s, s_in);
    let sf = floor_state(s);
    {
        // calm test against the state before this step (U0 is still the old state here)
        let so = ld0(i);
        let da0 = abs(sf.a - so.a);
        let db0 = abs(sf.b - so.b);
        let r = max(rho_s(sf), RHO_MIN);
        let dmx = max(max(max(da0.x, da0.y), max(da0.z, da0.w)), max(max(db0.x, db0.y), db0.w));
        if (dmx > CALM_EPS * r || db0.z > CALM_EPS * abs(sf.b.z) || temp(sf) > T_CHEM_MIN) { atomicOr(&WG_FL, 1u); }
    }
    st0(i, sf);
    return acc;
}

// Gas-mass production rate per exposed face of a burning solid.
fn solid_burn_rate(m: u32, tw: f32, p: f32) -> f32 {
    if (m == 7u && tw > 1.8) { return K_WOOD * exp(-T_WOOD / tw); }
    if (m == 8u && tw > T_PROP_IGN) { return K_PROP * pow(max(p, 0.1), N_PROP); }
    return 0.0;
}

fn strength_factor(m: u32, t: f32) -> f32 {
    let ts = M_TSOFT[m];
    if (t <= ts) { return 1.0; }
    let f = min((t - ts) / (M_TMELT[m] - ts), 1.0);
    return 1.0 - 0.9 * f * f;
}

// Gas -> solid surface heat flux (per face, per unit time) and the gas emissivity.
// Convection: boundary-layer h ~ k Re^0.5 with k, mu ~ T^0.7 gives h ~ sqrt(p) T^-0.15 (nearly independent of
// the gas temperature at fixed pressure), so hot flame gas impinging on a plate heats it strongly, unlike the old
// h ~ rho law; equal to the old HT for still ambient air. Forced-convection boost (1 + 0.5 |u|) kept.
// Radiation: eps * sigma (Tg^4 - Tw^4) from luminous soot and product gas in the adjacent cell.
fn gas_wall_flux(rho: f32, tg: f32, spd: f32, p: f32, soot: f32, prod: f32, tw: f32) -> vec2<f32> {
    let h = HT * sqrt(max(p, 0.05)) * pow(max(tg, 0.3), -0.15) * (1.0 + 0.5 * spd);
    let eps = 1.0 - exp(-(K_RAD_SOOT * max(soot, 0.0) + K_RAD_GAS * max(prod, 0.0)));
    let tg2 = tg * tg;
    let tw2 = tw * tw;
    return vec2<f32>(h * (tg - tw) + RAD * eps * (tg2 * tg2 - tw2 * tw2), eps);
}

// Result of the shared solid heat-exchange stage (walls and matter).
struct WallHeat { ntw: f32, exposed: f32, worst: f32, gp_sum: f32, gp_max: f32, gt_max: f32, o2_sum: f32 }

// Conduction to touching solids, igniter contact, convection to/from the neighbouring gas, radiation and
// active cooling for the solid cell i (tile 1 or 8) of material m at temperature tw.
fn wall_heat(i: u32, x: i32, y: i32, m: u32, tw: f32, dt: f32) -> WallHeat {
    // per-element heat capacity / conductivity (matter.wgsl hooks: real values for matter ids 10..N_ELEM-1)
    let e = select(m, elem_of(i), tile_of(i) == 8u);
    let cap = m_cap(e);
    let cond = m_cond(e);
    var open_sky = 0.0;
    var dq = 0.0;
    var exposed = 0.0;
    var worst = 0.0;
    var gp_sum = 0.0;
    var gp_max = 0.0;
    var gt_max = 0.0;
    var o2_sum = 0.0;
    for (var k = 0; k < 4; k++) {
        let o = OFF[k];
        let j = cid(x + o.x, y + o.y);
        if (j == i) { continue; }
        let tj = tile_of(j);
        if (tj == 1u || tj == 8u) {
            let tn = WALL[j].x;
            let ej = select(mat_of(j), elem_of(j), tj == 8u);
            dq += dt * min(cond, m_cond(ej)) * (tn - tw);
            worst = max(worst, abs(tn - tw));
        } else if (tj == 6u) {
            // igniters heat touching solids directly (pyrotechnic igniter / glow plug contact)
            if (P.ign_on != 0u && m >= 7u && tw < T_IGNITER) { dq += dt * 2.0 * (T_IGNITER - tw); }
        } else {
            exposed += 1.0;
            let gs = ld2a(j);
            let gpj = pres(gs);
            gp_sum += gpj;
            gp_max = max(gp_max, gpj);
            gt_max = max(gt_max, temp(gs));
            o2_sum += gs.a.y;
            let rho = max(rho_s(gs), RHO_MIN);
            let tg = temp(gs);
            let spd = sqrt(gs.b.x * gs.b.x + gs.b.y * gs.b.y) / rho;
            let q = gas_wall_flux(rho, tg, spd, gpj, gs.b.w, gs.a.w, tw);
            let lim = 0.4 * min(rho * cvmix(gs), cap) * abs(tg - tw);
            dq += clamp(dt * q.x, -lim, lim);
            open_sky += 1.0 - q.y;
        }
    }
    // the surface's own emission escapes through the transparent part of the touching gas
    let t2 = tw * tw;
    dq -= dt * RAD * max(t2 * t2 - 1.0, 0.0) * open_sky;
    dq -= min(dt * M_COOL[m], 0.5 * cap) * (tw - 1.0);
    let ntw = max(tw + dq / cap, 0.05);
    return WallHeat(ntw, exposed, worst, gp_sum, gp_max, gt_max, o2_sum);
}

// Matter cells (tile 8): heat exchange + matter_update (matter.wgsl). Split from `wall` so the heavy
// register set of matter_update does not throttle occupancy of the wall kernel; gas cells exit at once.
@compute @workgroup_size(16, 8)
fn wall_m(@builtin(global_invocation_id) g: vec3<u32>, @builtin(workgroup_id) wid: vec3<u32>) {
    if ((AUX[AUX_SOL + wid.y * ntx() + wid.x] & 2u) == 0u) { return; }
    if (g.x >= P.w || g.y >= P.h) { return; }
    let i = g.y * P.w + g.x;
    if (tile_of(i) != 8u) { return; }
    let x = i32(g.x);
    let y = i32(g.y);
    let m = mat_of(i);
    let w = WALL[i];
    let dt = P.wall_dt;
    let hh = wall_heat(i, x, y, m, w.x, dt);
    let ex = max(hh.exposed, 1.0);
    WALL[P.w * P.h + i] = matter_update(i, x, y, m, hh.ntw, w.w, hh.exposed, hh.gp_sum / ex, hh.gp_max, hh.gt_max, hh.o2_sum / ex, dt);
}

// Wall heating/conduction and mechanical failure (tile 1). Only solid cells write the next half;
// `commit` copies it back for tiles 1/8 (no full-grid wall_copy pass).
@compute @workgroup_size(16, 8)
fn wall(@builtin(global_invocation_id) g: vec3<u32>, @builtin(workgroup_id) wid: vec3<u32>) {
    if ((AUX[AUX_SOL + wid.y * ntx() + wid.x] & 1u) == 0u) { return; }
    if (g.x >= P.w || g.y >= P.h) { return; }
    let i = g.y * P.w + g.x;
    if (tile_of(i) != 1u) { return; }
    let x = i32(g.x);
    let y = i32(g.y);
    let n = P.w * P.h;
    let w = WALL[i];
    let m = mat_of(i);
    let dt = P.wall_dt;
    let hh = wall_heat(i, x, y, m, w.x, dt);
    let ntw = hh.ntw;
    let exposed = hh.exposed;
    let worst = hh.worst;
    let gp_sum = hh.gp_sum;
    let gp_max = hh.gp_max;
    let gt_max = hh.gt_max;
    var dmg = w.y;
    var ratio = 0.0;
    if (ntw >= M_TMELT[m]) {
        dmg = 1.5;
    } else {
        let sf = strength_factor(m, ntw);
        // Pressure load across the wall along its surface normal: the normal is the direction gas
        // pushes from (sum of offsets to neighbouring gas cells, 8-connected); trace through the wall
        // the other way to find its real thickness, including sloped walls.
        var nrm = vec2<f32>(0.0);
        var p_sum = 0.0;
        var p_hi = 0.0;
        var p_lo = 1e9;
        var cnt = 0.0;
        for (var dy = -1; dy <= 1; dy++) {
            for (var dx = -1; dx <= 1; dx++) {
                if (dx == 0 && dy == 0) { continue; }
                let ax = x + dx;
                let ay = y + dy;
                if (ax < 0 || ay < 0 || ax >= i32(P.w) || ay >= i32(P.h)) { continue; }
                let a = u32(ay) * P.w + u32(ax);
                if (is_solid(a)) { continue; }
                let pa = pres(ld2a(a));
                nrm += vec2<f32>(f32(dx), f32(dy));
                p_sum += pa;
                p_hi = max(p_hi, pa);
                p_lo = min(p_lo, pa);
                cnt += 1.0;
            }
        }
        if (cnt > 0.0) {
            let pa = p_sum / cnt;
            var thick = 1.0;
            var pb = 1.0;
            let mx = max(abs(nrm.x), abs(nrm.y));
            if (mx < 0.5) {
                // gas on opposite sides of a one-cell sliver
                pb = p_lo;
                ratio = (p_hi - p_lo) / (M_STR[m] * sf);
            } else {
                let st = vec2<i32>(i32(round(-nrm.x / mx)), i32(round(-nrm.y / mx)));
                let step_len = length(vec2<f32>(st));
                var kx = x + st.x;
                var ky = y + st.y;
                for (var k = 0; k < 12; k++) {
                    if (kx < 0 || ky < 0 || kx >= i32(P.w) || ky >= i32(P.h)) { break; }
                    let ki = u32(ky) * P.w + u32(kx);
                    let tk = tile_of(ki);
                    if (tk == 1u) { thick += step_len; }
                    else if (tk == 6u) { break; }
                    else { pb = pres(ld2a(ki)); break; }
                    kx += st.x;
                    ky += st.y;
                }
                ratio = abs(pa - pb) / (M_STR[m] * sf * thick);
            }
        }
        var dd = -0.01 * dt;
        // damage grows with overload; far-over-limit loads (blasts) tear walls apart within a few steps
        if (ratio > 1.0) { dd = K_DMG * dt * (ratio - 0.8) * (1.0 + ratio); }
        if (worst > M_SHOCK[m]) { dd += 0.5 * dt * worst / M_SHOCK[m]; }
        dmg = max(dmg + dd, 0.0);
    }
    // burnable solids
    var rem = w.w;
    var tout = ntw;
    if ((m == 7u || m == 8u) && exposed > 0.0) {
        let r = solid_burn_rate(m, ntw, gp_sum / exposed);
        rem -= dt * r * exposed / (SOLID_MASS * select(1.0, 3.0, m == 7u));
        // burning surface stays hot; hot gas sweeping the bore ignites fresh propellant almost instantly
        if (m == 8u && (ntw > T_PROP_IGN || gt_max > 5.0)) { tout = max(ntw, 3.0); }
        if (rem <= 0.0) { rem = 0.0; dmg = 1.5; }
    }
    if (m == 9u && (ntw > T_EXPL_IGN || gp_max > P_EXPL_SHOCK || gt_max > 6.0)) { dmg = 2.0; }
    WALL[n + i] = vec4<f32>(tout, dmg, ratio, rem);
}

// Copies the wall/matter result (next half) back, then turns failed walls into gas (averaged from
// neighbouring gas) and applies matter state flags.
@compute @workgroup_size(16, 8)
fn commit(@builtin(global_invocation_id) g: vec3<u32>, @builtin(workgroup_id) wid: vec3<u32>) {
    if (AUX[AUX_SOL + wid.y * ntx() + wid.x] == 0u) { return; }
    if (g.x >= P.w || g.y >= P.h) { return; }
    let x = i32(g.x);
    let y = i32(g.y);
    let i = g.y * P.w + g.x;
    let tl = tile_of(i);
    if (tl != 1u && tl != 8u) { return; }
    let wnext = WALL[P.w * P.h + i];
    WALL[i] = wnext;
    if (wnext.y < 1.0) { return; }
    if (tl == 8u) {
        let wv = WALL[i];
        let fl = wv.y;
        if (fl > 2.9) {
            // molten metal solidified back into its metal (thermite iron -> steel)
            let orig = select(0u, u32(wv.z), wv.z >= 0.5 && wv.z < 7.0);
            META[i] = 1u | (orig << 8u);
            WALL[i] = vec4<f32>(wv.x, 0.0, 0.0, 1.0);
            return;
        }
        if (fl > 1.95) {
            var d = gas_state(vec4<f32>(0.05, 0.0, 0.1, 0.85), P_DET, T_DET);
            d.b.w = 0.01;
            st0(i, d);
            src_global(d, i);
            META[i] = 0u;
            WALL[i] = vec4<f32>(1.0, 0.0, 0.0, 1.0);
            return;
        }
        if (fl > 1.75) { META[i] = MATTER | (25u << 8u); WALL[i] = vec4<f32>(wv.x, 0.0, 0.0, 1.0); return; }
        if (fl > 1.65) { META[i] = MATTER | (24u << 8u); WALL[i] = vec4<f32>(max(wv.x, 9.0), 0.0, 0.0, 1.0); return; }
        if (fl > 1.55) {
            // the cell's remaining reaction gas, hot and somewhat compressed (a bang inside a pile)
            var d = gas_state(vec4<f32>(0.0, 0.0, 0.15, 0.85), 4.0, max(wv.x, 5.0));
            d.b.w = 0.02;
            st0(i, d);
            src_global(d, i);
            META[i] = 0u;
            WALL[i] = vec4<f32>(1.0, 0.0, 0.0, 1.0);
            return;
        }
    }
    let wm = mat_of(i);
    let wv1 = WALL[i];
    if (wm <= 6u && wm != 5u && wv1.x >= M_TMELT[wm]) {
        // a metal wall that melted turns into molten metal that flows; it remembers what it was
        META[i] = MATTER | (24u << 8u);
        WALL[i] = vec4<f32>(wv1.x + 0.3, 0.0, f32(wm) + 0.5, 1.0);
        return;
    }
    if (wm == 7u && wv1.w <= 0.0) {
        // wood burnt through leaves hot ash
        META[i] = MATTER | (25u << 8u);
        WALL[i] = vec4<f32>(wv1.x, 0.0, 0.0, 1.0);
        return;
    }
    if (wm == 9u) {
        // detonation: the solid becomes dense, very hot, oxygen-poor gas -> blast wave + sooty fireball
        var d = gas_state(vec4<f32>(0.12, 0.0, 0.0, 0.88), P_DET, T_DET);
        d.b.w = 0.015;
        st0(i, d);
        src_global(d, i);
        META[i] = 0u;
        WALL[i] = vec4<f32>(1.0, 0.0, 0.0, 1.0);
        return;
    }
    var acc = S(vec4<f32>(0.0), vec4<f32>(0.0));
    var cnt = 0.0;
    for (var k = 0; k < 4; k++) {
        let o = OFF[k];
        let j = cid(x + o.x, y + o.y);
        if (j == i || is_solid(j)) { continue; }
        let sj = ld0(j);
        acc = S(acc.a + sj.a, acc.b + sj.b);
        cnt += 1.0;
    }
    var s = air_state(1.0, 1.0);
    if (cnt > 0.0) { s = S(acc.a / cnt, acc.b / cnt); }
    st0(i, s);
    src_global(s, i);
    META[i] = 0u;
    WALL[i] = vec4<f32>(1.0, 0.0, 0.0, 1.0);
}

@compute @workgroup_size(16, 16)
fn init(@builtin(global_invocation_id) g: vec3<u32>) {
    if (g.x >= P.w || g.y >= P.h) { return; }
    let i = g.y * P.w + g.x;
    let n = P.w * P.h;
    let s = air_state(1.0, 1.0);
    st0(i, s); st1(i, s); st2(i, s);
    WALL[i] = vec4<f32>(1.0, 0.0, 0.0, 1.0);
    WALL[n + i] = vec4<f32>(1.0, 0.0, 0.0, 1.0);
}

fn in_shape(o: Op, x: i32, y: i32) -> bool {
    if (o.shape == 0u) {
        let dx = x - o.x0;
        let dy = y - o.y0;
        return dx * dx + dy * dy <= o.r * o.r + o.r / 2;
    }
    let lx = min(o.x0, o.x1);
    let hx = max(o.x0, o.x1);
    let ly = min(o.y0, o.y1);
    let hy = max(o.y0, o.y1);
    if (o.shape == 1u) { return x >= lx && x <= hx && y >= ly && y <= hy; }
    if (o.shape == 2u) {
        if (x < lx || x > hx || y < ly || y > hy) { return false; }
        let t = o.r + 1;
        return x < lx + t || x > hx - t || y < ly + t || y > hy - t;
    }
    let a = vec2<f32>(f32(o.x0), f32(o.y0));
    let b = vec2<f32>(f32(o.x1), f32(o.y1));
    let p = vec2<f32>(f32(x), f32(y));
    let ab = b - a;
    let t = clamp(dot(p - a, ab) / max(dot(ab, ab), 1e-6), 0.0, 1.0);
    return length(p - (a + ab * t)) <= f32(o.r) + 0.5;
}

// User edits. kind: 0 tile, 1 erase, 2 fuel, 3 oxygen, 4 air, 5 flame, 6 cool, 7 vacuum
@compute @workgroup_size(16, 16)
fn apply_ops(@builtin(global_invocation_id) g: vec3<u32>) {
    if (g.x >= P.w || g.y >= P.h) { return; }
    let x = i32(g.x);
    let y = i32(g.y);
    let i = g.y * P.w + g.x;
    var s = ld0(i);
    var mt = META[i];
    var w = WALL[i];
    let s_in = s;
    let gas_in = !solid_t(mt & 0xFFu);
    for (var k = 0u; k < P.nops; k++) {
        let o = OPS[k];
        if (!in_shape(o, x, y)) { continue; }
        let cur = mt & 0xFFu;
        if (o.kind == 0u && o.tile == 8u) {
            if (cur == 0u) {
                // composition brush: _p0 = second element, _p1 = its cell fraction x1000 (per-cell hash picks);
                // dir bit 0 = place as a cast / pressed grain
                var el = o.mat;
                if (o._p1 > 0u && mhash(u32(x) * 3u + 1u, u32(y) * 5u + 2u, 77u) * 1000.0 < f32(o._p1)) { el = o._p0; }
                mt = 8u | (el << 8u) | ((o.dir & 1u) << 16u);
                w = vec4<f32>(E_TINIT[min(el, N_ELEM - 1u)], 0.0, 0.0, 1.0);
            }
        } else if (o.kind == 9u) {
            // cast / press: matter cells in the shape become a packed grain (immobile, surface-burning)
            if (cur == 8u) { mt = mt | (1u << 16u); }
        } else if (o.kind == 0u) {
            let old_mat = (mt >> 8u) & 0xFFu;
            mt = o.tile | (o.mat << 8u) | (o.dir << 16u);
            if (solid_t(o.tile)) {
                s = air_state(1.0, 1.0);
                if (cur != 1u || o.tile != 1u || old_mat != o.mat) { w = vec4<f32>(1.0, 0.0, 0.0, 1.0); }
            }
        } else if (o.kind == 5u && cur == 1u) {
            // torch on a solid: heat its surface (ignites wood, propellant and explosives)
            w.x = max(w.x, 4.0);
        } else if (o.kind == 1u) {
            if (solid_t(cur) || (cur >= 2u && cur <= 4u) || cur == 7u) { s = air_state(1.0, 1.0); }
            mt = 0u;
            w = vec4<f32>(1.0, 0.0, 0.0, 1.0);
        } else if (!solid_t(cur)) {
            let t = temp(s);
            if (o.kind == 2u) {
                let m = min(0.05, max(0.6 - s.a.x, 0.0));
                s.a.x += m; s = set_temp(s, t);
            } else if (o.kind == 3u) {
                let m = min(0.07, max(1.1 - s.a.y, 0.0));
                s.a.y += m; s = set_temp(s, t);
            } else if (o.kind == 4u) {
                if (rho_s(s) < 1.2) { s.a.y += 0.023; s.a.z += 0.077; s = set_temp(s, t); }
            } else if (o.kind == 5u) {
                if (t < 9.0) { s = set_temp(s, 9.0); }
            } else if (o.kind == 6u) {
                s = set_temp(s, max(t * 0.9, 0.3));
            } else if (o.kind == 7u) {
                s = S(s.a * 0.85, s.b * 0.85);
            }
            s = floor_state(s);
        }
    }
    st0(i, s);
    // user edits are sources for the conservation accounting
    var dsrc = vec4<f32>(0.0);
    if (gas_in) { dsrc -= vec4<f32>(0.0, 0.0, mass_tot(s_in), en_tot(s_in)); }
    if (!solid_t(mt & 0xFFu)) { dsrc += vec4<f32>(0.0, 0.0, mass_tot(s), en_tot(s)); }
    acc_global(10u, dsrc.z, i);
    acc_global(11u, dsrc.w, i);
    META[i] = mt;
    WALL[i] = w;
}

// smooth equilibrium (dissociation) limit on the net heat-release rate, 1 below T_DISS0, 0 at T_DISS1
fn diss_factor(tt: f32) -> f32 {
    let f = clamp((T_DISS1 - tt) / (T_DISS1 - T_DISS0), 0.0, 1.0);
    return f * f * (3.0 - 2.0 * f);
}

fn reaction_rate(s: S, tt: f32, nu_t: f32) -> f32 {
    var w = A_PRE * s.a.x * s.a.y * exp(-T_ACT / tt);
    if (tt > T_EDC) {
        w = max(w, C_EDC * (D_SPEC + nu_t / PR_T) * min(s.a.x, 0.25 * s.a.y));
    }
    return w;
}

fn burn_rate(s: S) -> f32 {
    let tt = temp(s);
    if (tt < T_CHEM_MIN) { return 0.0; }
    return min(reaction_rate(s, tt, 0.0) * diss_factor(tt), s.a.x / P.dt);
}

// Workgroup-level partial results of `stats` (slots as in STATS): one global atomic per workgroup and slot
// instead of ~3 per cell on 8 shared addresses. Integer max/add are order-independent, so results are exact.
var<workgroup> STW: array<atomic<u32>, 8>;
fn atomic_max_f(k: u32, v: f32) { atomicMax(&STW[k], bitcast<u32>(max(v, 0.0))); }
fn atomic_add_fixed(k: u32, v: f32) { atomicAdd(&STW[k], bitcast<u32>(i32(round(v * 1000.0)))); }

fn stats_cell(x: i32, y: i32, i: u32) {
    let t = tile_of(i);
    if (t == 1u || t == 6u) {
        var f = vec2<f32>(0.0);
        for (var k = 0; k < 4; k++) {
            let o = OFF[k];
            let ax = x + o.x;
            let ay = y + o.y;
            if (ax < 0 || ay < 0 || ax >= i32(P.w) || ay >= i32(P.h)) { continue; }
            let a = u32(ay) * P.w + u32(ax);
            if (is_solid(a)) { continue; }
            // gas on side o pushes the wall towards -o
            f -= vec2<f32>(o) * (pres(ld0(a)) - 1.0);
        }
        atomic_add_fixed(6u, f.x);
        atomic_add_fixed(7u, f.y);
        if (t == 1u) {
            let w = WALL[i];
            atomic_max_f(2u, w.x / M_TMELT[mat_of(i)]);
            atomic_max_f(5u, w.z);
            atomicAdd(&STW[4], 1u);
        }
        return;
    }
    let s = ld0(i);
    if (t == 0u || t == 5u) {
        atomic_max_f(0u, pres(s));
        atomic_max_f(1u, temp(s));
    }
    atomic_add_fixed(3u, burn_rate(s));
    if (t == 5u) {
        let d = dir_of(i);
        var dv = vec2<f32>(0.0, -1.0);
        if (d == 0u) { dv = vec2<f32>(1.0, 0.0); }
        else if (d == 1u) { dv = vec2<f32>(0.0, 1.0); }
        else if (d == 2u) { dv = vec2<f32>(-1.0, 0.0); }
        let fan = -dv * max(rho_s(s), RHO_MIN) * P.fan_pow;
        atomic_add_fixed(6u, fan.x);
        atomic_add_fixed(7u, fan.y);
    }
}

// Whole-engine readouts: peaks, burn rate and the net pressure/fan force on the structure (thrust).
@compute @workgroup_size(16, 16)
fn stats(@builtin(global_invocation_id) g: vec3<u32>, @builtin(local_invocation_index) li: u32) {
    if (li < 8u) { atomicStore(&STW[li], 0u); }
    workgroupBarrier();
    if (g.x < P.w && g.y < P.h) { stats_cell(i32(g.x), i32(g.y), g.y * P.w + g.x); }
    workgroupBarrier();
    if (li < 8u) {
        let v = atomicLoad(&STW[li]);
        if (v != 0u) {
            if (li == 0u || li == 1u || li == 2u || li == 5u) { atomicMax(&STATS[li], v); }
            else { atomicAdd(&STATS[li], v); }
        }
    }
}

