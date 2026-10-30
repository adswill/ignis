// Sparks & embers: a ring buffer of NP small incandescent particles (Fire view only).
//   spark_spawn  (per cell): burning powders, molten metal spatter, burning wood, sooty flame tips, blasts, igniters
//   spark_update (per particle): drag toward the local gas velocity, gravity, bounce off solids, cooling
//   pvs / pfs: instanced soft streak quads (motion-blurred along velocity), additive over the tone-mapped image
// Own bind groups (compute: P, FP, U0, META, WALL, U2, PARTS, CTR; draw: P, PARTS ro, FP).

struct Params {
    w: u32, h: u32, nops: u32, view: u32,
    dt: f32, p_fuel: f32, fan_pow: f32, time: f32,
    ign_on: u32, frame: u32, srgb: u32, _p1: u32,
    p_o2: f32, p_air: f32, cam_x: f32, cam_y: f32,
    cam_w: f32, cam_h: f32, wall_dt: f32, gravity: f32,
}
struct FxParams { dt: f32, ph0: f32, ph1: f32, reset: u32, src: u32, stime: f32, _b: u32, _c: u32 }
// heat: 0..1 (1 = ~2900 K), cool: cooling rate per time unit, drag: relaxation rate to the gas velocity
struct Spark { pos: vec2<f32>, vel: vec2<f32>, heat: f32, cool: f32, drag: f32, size: f32 }

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var<uniform> FP: FxParams;

const NP: u32 = 32768u;
// gravity on sparks (cells / time unit^2); a 3 cells/tu toss rises ~40 cells
const G_SPARK: f32 = 0.11;
const RS = vec4<f32>(1.80625, 0.903125, 1.032142857, 1.111538462);
const CVS = vec4<f32>(5.827, 2.258, 2.580, 4.446);

fn cid(x: i32, y: i32) -> u32 {
    return u32(clamp(y, 0, i32(P.h) - 1)) * P.w + u32(clamp(x, 0, i32(P.w) - 1));
}

fn hash3(x: u32, y: u32, f: u32) -> u32 {
    var h = x * 0x9E3779B1u ^ y * 0x85EBCA77u ^ f * 0xC2B2AE3Du;
    h ^= h >> 15u;
    h *= 0x2C1B3C6Du;
    h ^= h >> 12u;
    h *= 0x297A2D39u;
    h ^= h >> 15u;
    return h;
}
fn rnd(s: ptr<function, u32>) -> f32 {
    *s = hash3(*s, 0x51ED27u, 0x9E37u);
    return f32(*s & 0xFFFFFFu) / 16777216.0;
}

// Blackbody-ish display colour for a spark of heat h (0..1): cherry -> orange -> yellow-white, HDR intensity
fn spark_col(h: f32) -> vec3<f32> {
    let tk = 900.0 + 2000.0 * clamp(h, 0.0, 1.0);
    let r = tk / 2000.0;
    let i = r * r * r * r * 1.4;
    let c = mix(vec3<f32>(1.0, 0.18, 0.03), vec3<f32>(1.0, 0.55, 0.16), smoothstep(1000.0, 1700.0, tk));
    let c2 = mix(c, vec3<f32>(1.0, 0.88, 0.62), smoothstep(1800.0, 2700.0, tk));
    return c2 * i;
}

// ---------------- simulation ----------------

@group(0) @binding(2) var<storage, read> U0: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read> META: array<u32>;
@group(0) @binding(4) var<storage, read> WALL: array<vec4<f32>>;
@group(0) @binding(5) var<storage, read> U2: array<vec4<f32>>;
@group(0) @binding(6) var<storage, read_write> PARTS: array<Spark>;
@group(0) @binding(7) var<storage, read_write> CTR: array<atomic<u32>>;
// last frame's gas pressure per cell: blast sparks fly only where pressure jumps (explosions), not in steady chambers
@group(0) @binding(8) var<storage, read_write> PPREV: array<f32>;

fn solid_at(i: u32) -> bool { let t = META[i] & 0xFFu; return t == 1u || t == 6u || t == 8u || t == 9u || t == 10u || t == 12u || t == 13u; }
fn solid_p(p: vec2<f32>) -> bool {
    if (p.x < 0.0 || p.y < 0.0 || p.x >= f32(P.w) || p.y >= f32(P.h)) { return false; }
    return solid_at(cid(i32(floor(p.x)), i32(floor(p.y))));
}
fn vel_at(i: u32) -> vec2<f32> {
    if (solid_at(i)) { return vec2<f32>(0.0); }
    let a = U0[2u * i];
    let b = U0[2u * i + 1u];
    let rho = max(a.x + a.y + a.z + a.w, 0.002);
    let v = vec2<f32>(b.x, b.y) / rho;
    return select(vec2<f32>(0.0), clamp(v, vec2<f32>(-40.0), vec2<f32>(40.0)), v == v);
}
fn vel(p: vec2<f32>) -> vec2<f32> {
    let q = p - 0.5;
    let f = floor(q);
    let t = q - f;
    let x = i32(f.x);
    let y = i32(f.y);
    let a = vel_at(cid(x, y));
    let b = vel_at(cid(x + 1, y));
    let c = vel_at(cid(x, y + 1));
    let d = vel_at(cid(x + 1, y + 1));
    return mix(mix(a, b, t.x), mix(c, d, t.x), t.y);
}

fn emit(p: vec2<f32>, v: vec2<f32>, heat: f32, cool: f32, drag: f32, size: f32) {
    let k = atomicAdd(&CTR[0], 1u) % NP;
    PARTS[k] = Spark(p, v, heat, cool, drag, size);
}

@compute @workgroup_size(16, 16)
fn spark_spawn(@builtin(global_invocation_id) g: vec3<u32>) {
    if (g.x >= P.w || g.y >= P.h) { return; }
    let i = g.y * P.w + g.x;
    let m = META[i];
    let t = m & 0xFFu;
    // gas pressure history (always kept current)
    var pjump = 0.0;
    if (t == 0u) {
        let a0 = max(U0[2u * i], vec4<f32>(0.0));
        let b0 = U0[2u * i + 1u];
        let rho0 = max(a0.x + a0.y + a0.z + a0.w, 0.002);
        let p0 = dot(a0, RS) / max(dot(a0, CVS), 1e-6) * (b0.z - 0.5 * (b0.x * b0.x + b0.y * b0.y) / rho0);
        let pp = select(1.0, p0, p0 == p0);
        if (FP.dt > 0.0) {
            pjump = pp - PPREV[i];
            PPREV[i] = pp;
        }
    } else if (FP.dt > 0.0) {
        PPREV[i] = 1.0;
    }
    if (!(P.view == 0u || P.view == 6u) || FP.dt <= 0.0) { return; }
    var s = hash3(g.x, g.y, P.frame * 7919u + 13u);
    let dt = FP.dt;
    let cpos = vec2<f32>(f32(g.x), f32(g.y));
    if (t == 8u) {
        // condensed matter: burning powders throw sparks, hot molten metal spatters
        let e = (m >> 8u) & 0xFFu;
        let w = WALL[i];
        let kindc = floor(w.z / 1000.0);
        // only surface cells (gas above or beside) emit
        let up = cid(i32(g.x), i32(g.y) - 1);
        let open = !solid_at(up) || !solid_at(cid(i32(g.x) - 1, i32(g.y))) || !solid_at(cid(i32(g.x) + 1, i32(g.y)));
        if (!open) { return; }
        var rate = 0.0;
        var spd = 2.0;
        var heat = 0.8;
        var drag = 0.6;
        var size = 0.32;
        if ((kindc == 3.0 || kindc == 4.0) && w.x > 2.5) {
            rate = 0.10;
            spd = 3.2;
            heat = 0.9;
            if (e == 15u) { rate = 0.30; spd = 5.0; heat = 1.0; drag = 0.25; size = 0.28; } // aluminium: white sparks
            if (e == 11u) { rate = 0.06; spd = 1.6; heat = 0.7; drag = 0.9; size = 0.36; }  // charcoal embers
        }
        if (e == 24u && w.x > 5.0) { rate = max(rate, 0.05 * smoothstep(5.0, 10.0, w.x)); spd = 3.5; heat = 0.95; drag = 0.15; size = 0.4; }
        if (rate <= 0.0) { return; }
        if (rnd(&s) < rate * dt) {
            let a = -1.5708 + (rnd(&s) - 0.5) * 2.2;
            let sp = spd * (0.4 + 0.9 * rnd(&s));
            let v = vec2<f32>(cos(a), sin(a)) * sp + vel(cpos + vec2<f32>(0.5, -0.5));
            emit(cpos + vec2<f32>(rnd(&s), -0.05), v, heat * (0.85 + 0.15 * rnd(&s)), 0.25 + 0.3 * rnd(&s), drag, size);
        }
        return;
    }
    if (t == 1u) {
        // burning wood: slow embers off exposed faces
        let mat = (m >> 8u) & 0xFFu;
        if (mat != 7u) { return; }
        let w = WALL[i];
        if (w.x < 2.8 || w.w >= 0.999) { return; }
        let up = cid(i32(g.x), i32(g.y) - 1);
        if (solid_at(up)) { return; }
        if (rnd(&s) < 0.02 * dt) {
            let v = vec2<f32>((rnd(&s) - 0.5) * 1.2, -0.6 - 1.4 * rnd(&s)) + vel(cpos + vec2<f32>(0.5, -0.5));
            emit(cpos + vec2<f32>(rnd(&s), -0.05), v, 0.75, 0.12 + 0.12 * rnd(&s), 1.2, 0.36);
        }
        return;
    }
    if (t == 6u) {
        if (P.ign_on != 0u && rnd(&s) < 0.04 * dt) {
            let a = rnd(&s) * 6.2831853;
            emit(cpos + 0.5, vec2<f32>(cos(a), sin(a)) * (1.0 + 2.0 * rnd(&s)), 0.95, 0.9, 0.3, 0.26);
        }
        return;
    }
    if (t != 0u) { return; }
    // gas: embers lifted out of hot sooty flames; debris sparks thrown by blasts
    let a = max(U0[2u * i], vec4<f32>(0.0));
    let b = U0[2u * i + 1u];
    let rho = max(a.x + a.y + a.z + a.w, 0.002);
    let tt = U2[2u * i + 1u].a;
    let soot = max(b.w, 0.0);
    if (tt < 3.5) { return; }
    let ke = 0.5 * (b.x * b.x + b.y * b.y) / rho;
    let p = dot(a, RS) / max(dot(a, CVS), 1e-6) * (b.z - ke);
    let v = vec2<f32>(b.x, b.y) / rho;
    var rate = 0.0012 * smoothstep(0.004, 0.04, soot) * smoothstep(3.5, 6.0, tt);
    var heat = 0.7;
    var drag = 1.4;
    var size = 0.3;
    var kick = 0.6;
    if (pjump > 0.8 && p > 2.5 && tt > 5.0) {
        rate += 0.05 * smoothstep(0.8, 6.0, pjump);
        heat = 1.0;
        drag = 0.2;
        kick = 2.5;
        size = 0.26;
    }
    if (rate <= 0.0) { return; }
    if (rnd(&s) < rate * dt) {
        let an = rnd(&s) * 6.2831853;
        let vv = select(vec2<f32>(0.0), v, v == v) * (1.0 + 0.3 * rnd(&s)) + vec2<f32>(cos(an), sin(an)) * kick * rnd(&s);
        emit(cpos + vec2<f32>(rnd(&s), rnd(&s)), vv, heat * (0.8 + 0.2 * rnd(&s)), 0.2 + 0.25 * rnd(&s), drag, size);
    }
}

@compute @workgroup_size(256)
fn spark_update(@builtin(global_invocation_id) g: vec3<u32>) {
    let id = g.x;
    if (id >= NP) { return; }
    var sp = PARTS[id];
    if (sp.heat <= 0.0) { return; }
    if (!(P.view == 0u || P.view == 6u)) { sp.heat = 0.0; PARTS[id] = sp; return; }
    let dt = FP.dt;
    if (dt <= 0.0) { return; }
    var pos = sp.pos;
    var v = sp.vel;
    var s = hash3(id, P.frame, 0xA5A5u);
    let nsub = u32(clamp(ceil(length(v) * dt / 0.7), 1.0, 12.0));
    let h = dt / f32(nsub);
    for (var k = 0u; k < nsub; k++) {
        let gv = vel(pos);
        // drag toward the gas velocity (exact exponential relaxation), plus gravity
        let f = 1.0 - exp(-sp.drag * h);
        v = mix(v, gv, f) + vec2<f32>(0.0, G_SPARK * h);
        var np = pos + v * h;
        if (solid_p(np)) {
            // bounce: find the blocked axis
            if (solid_p(vec2<f32>(np.x, pos.y))) { v.x = -v.x * 0.35; np.x = pos.x; }
            if (solid_p(vec2<f32>(np.x, np.y))) { v.y = -v.y * 0.3; v.x *= 0.7; np.y = pos.y; }
            sp.heat -= 0.04; // contact cools
        }
        pos = np;
    }
    // flicker-free exponential cooling; hot gas around keeps embers alive
    let c = cid(i32(floor(pos.x)), i32(floor(pos.y)));
    let tg = U2[2u * c + 1u].a;
    let env = smoothstep(3.0, 7.0, tg);
    sp.heat *= exp(-sp.cool * dt * (1.0 - 0.8 * env));
    sp.heat -= 0.004 * dt;
    if (pos.x < -2.0 || pos.y < -2.0 || pos.x > f32(P.w) + 2.0 || pos.y > f32(P.h) + 2.0 || !(pos.x == pos.x) || sp.heat < 0.08) {
        sp.heat = 0.0;
    }
    sp.pos = pos;
    sp.vel = v;
    PARTS[id] = sp;
}

// ---------------- drawing: instanced soft streaks, additive ----------------

@group(1) @binding(0) var<storage, read> PR: array<Spark>;

struct VO { @builtin(position) pos: vec4<f32>, @location(0) col: vec3<f32>, @location(1) q: vec2<f32>, @location(2) len: f32 }

@vertex
fn pvs(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> VO {
    var o: VO;
    o.pos = vec4<f32>(-9.0, -9.0, 0.0, 1.0);
    o.col = vec3<f32>(0.0);
    o.q = vec2<f32>(0.0);
    o.len = 0.0;
    if (!(P.view == 0u || P.view == 6u)) { return o; }
    let p = PR[ii];
    if (p.heat <= 0.0) { return o; }
    // streak: from the position a short while ago to now (motion blur), at least a round dot
    let blur = 0.45; // time units of motion blur (~ one 60 Hz frame at 1x)
    var dv = p.vel * blur;
    let l = length(dv);
    let maxl = 14.0;
    if (l > maxl) { dv *= maxl / l; }
    let len = min(l, maxl);
    let dir = select(vec2<f32>(1.0, 0.0), dv / max(len, 1e-4), len > 1e-4);
    let nrm = vec2<f32>(-dir.y, dir.x);
    // pixel scale: keep sparks at least ~1.6 screen pixels wide
    let r = max(p.size, 0.0);
    var corner = array<vec2<f32>, 6>(vec2<f32>(0.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(0.0, 1.0),
                                     vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(0.0, 1.0));
    let cn = corner[vi];
    // along: from tail (pos - dv - r) to head (pos + r); across: +-2r (soft gaussian falloff)
    let along = mix(-len - r * 2.0, r * 2.0, cn.x);
    let wp = p.pos + dir * along + nrm * cn.y * r * 2.5;
    let uv = (wp - vec2<f32>(P.cam_x, P.cam_y)) / vec2<f32>(P.cam_w, P.cam_h);
    o.pos = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    // energy spread over the streak length: long streaks get dimmer per pixel
    let hc = spark_col(p.heat) * (2.0 * r / (2.0 * r + len * 0.6));
    o.col = hc;
    o.q = vec2<f32>(along / r, cn.y * 2.5);
    o.len = len / r;
    return o;
}

@fragment
fn pfs(in: VO) -> @location(0) vec4<f32> {
    // distance to the segment [-len, 0] in radius units
    let ax = clamp(in.q.x, -in.len, 0.0);
    let d = vec2<f32>(in.q.x - ax, in.q.y);
    let fall = exp(-dot(d, d) * 1.1);
    // head brighter than the tail
    let ht = select(1.0, 0.35 + 0.65 * (1.0 + ax / max(in.len, 1e-3)), in.len > 0.0);
    var c = in.col * fall * ht;
    // simple tone curve (the main image is already tone-mapped; sparks are added on top)
    c = c / (1.0 + c);
    c = c * 1.6;
    if (P.srgb != 0u) { c = pow(max(c, vec3<f32>(0.0)), vec3<f32>(2.2)); }
    return vec4<f32>(c, 0.0);
}
