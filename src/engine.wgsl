// Reciprocating-engine kinematics: crank-slider pistons, cam-timed valves, spark plugs and fuel injectors.
// Appended to sim.wgsl (shares its bindings and helpers). State and configuration live in the AUX buffer (u32 words,
// f32 values are stored as bit patterns), starting at word ENG; the CPU uploads the configuration image on scene load
// (engine.rs builds it) and the GPU owns the crank angle and the per-step window flags.
//
//   ENG+0  omega        crank angular speed, rad per simulation time unit
//   ENG+1  theta        current crank angle, rad, wrapped to [0, period)            (GPU written)
//   ENG+2  n_pistons    (<= 8)
//   ENG+3  n_cells      number of timed cells in the toggle list (<= 512)
//   ENG+4  period       cam cycle in rad: 4*pi (four-stroke) or 2*pi (two-stroke)
//   ENG+5  thickness    piston body thickness in cells
//   ENG_PIST + 8*p      x_tdc, stroke, rod length, phase (f32); y0, rows (u32); x_head_old, x_head_target (i32)
//   ENG_VALVE + 4*v     open angle, close angle, phase (f32); open flag (u32, GPU written)
//   ENG_SPARK + 4*s     on angle, off angle, phase (f32); active flag (u32, GPU written)
//   ENG_INJ   + 4*j     on angle, off angle, phase (f32); active flag (u32, GPU written)
//   ENG_CELLS + n       toggled cells: cell index | (kind << 20); kind 0 valve, 1 injector. The id is the cell's material byte.
//
// Tiles: 9 piston (solid), 10 valve closed (solid), 11 valve open (gas), 12 spark plug (solid; heats gas while its window
// is active), 13 injector off (solid), 14 injector on (pinned fuel inlet), 15 premixed intake inlet (pinned, phi = mat/10).

const ENG: u32 = 24576u;
const ENG_PIST: u32 = ENG + 16u;
const ENG_VALVE: u32 = ENG + 80u;
const ENG_SPARK: u32 = ENG + 144u;
const ENG_INJ: u32 = ENG + 208u;
const ENG_CELLS: u32 = ENG + 272u;
const T_PISTON: u32 = 9u;
const T_VALVE_C: u32 = 10u;
const T_VALVE_O: u32 = 11u;
const T_SPARK: u32 = 12u;
const T_INJ_OFF: u32 = 13u;
const T_INJ_ON: u32 = 14u;
const T_PREMIX: u32 = 15u;

fn eng_f(w: u32) -> f32 { return bitcast<f32>(AUX[w]); }
fn eng_set_f(w: u32, v: f32) { AUX[w] = bitcast<u32>(v); }
fn spark_active(id: u32) -> bool { return AUX[ENG_SPARK + min(id, 15u) * 4u + 3u] != 0u; }

fn eng_in_window(theta: f32, on: f32, off: f32, phase: f32, period: f32) -> bool {
    var a = theta + phase;
    a = a - floor(a / period) * period;
    if (on <= off) { return a >= on && a < off; }
    return a >= on || a < off;
}

// head cell x of piston p at crank angle theta (crank-slider with connecting-rod length)
fn piston_head(p: u32, theta: f32) -> i32 {
    let b = ENG_PIST + p * 8u;
    let xt = eng_f(b);
    let stroke = eng_f(b + 1u);
    let l_raw = eng_f(b + 2u);
    let l = abs(l_raw);
    let sg = select(1.0, -1.0, l_raw < 0.0);
    let a = theta + eng_f(b + 3u);
    let r = 0.5 * stroke;
    let sn = sin(a);
    let d = r * (1.0 - cos(a)) + l - sqrt(max(l * l - r * r * sn * sn, 0.0));
    return i32(floor(xt - sg * d + 0.5));
}

// Advances the crank by the current (adaptive) time step and evaluates every timing window. One workgroup.
@compute @workgroup_size(64)
fn engine_step(@builtin(local_invocation_index) li: u32) {
    let period = max(eng_f(ENG + 4u), 1.0);
    let th_old = eng_f(ENG + 1u);
    var th = th_old + eng_f(ENG) * sim_dt();
    th = th - floor(th / period) * period;
    workgroupBarrier();
    if (li == 0u) { eng_set_f(ENG + 1u, th); }
    if (li < 8u) {
        if (li < AUX[ENG + 2u]) {
            AUX[ENG_PIST + li * 8u + 7u] = bitcast<u32>(piston_head(li, th));
        }
    } else if (li < 56u) {
        var base = ENG_VALVE;
        var k = li - 8u;
        if (li >= 24u) { base = ENG_SPARK; k = li - 24u; }
        if (li >= 40u) { base = ENG_INJ; k = li - 40u; }
        let e = base + k * 4u;
        let on = eng_f(e);
        let off = eng_f(e + 1u);
        if (on != off) {
            AUX[e + 3u] = select(0u, 1u, eng_in_window(th, on, off, eng_f(e + 2u), period));
        }
    }
}

// Moves each piston toward its target head cell, one cell at a time, one thread per bore row. The displaced gas is
// merged into the cell in front of the piston with the p dV compression work added (energy would otherwise be
// conserved but the piston's work missed, so the charge would not heat up); on retreat the cell in front is split
// in two and the expansion work is taken out. The crankcase side is open to ambient.
@compute @workgroup_size(64)
fn engine_piston(@builtin(workgroup_id) wid: vec3<u32>, @builtin(local_invocation_index) li: u32) {
    let p = wid.x;
    if (p >= AUX[ENG + 2u]) { return; }
    let b = ENG_PIST + p * 8u;
    if (li >= AUX[b + 5u]) { return; }
    let y = i32(AUX[b + 4u] + li);
    var xo = bitcast<i32>(AUX[b + 6u]);
    let xt = bitcast<i32>(AUX[b + 7u]);
    let th = i32(AUX[ENG + 5u]);
    // a negative rod length marks a mirrored piston (opposed-piston engines): its body extends towards +x
    let sg = select(1, -1, eng_f(b + 2u) < 0.0);
    for (var n = 0; n < 2; n++) {
        if (xo == xt) { break; }
        if (sg * (xt - xo) > 0) {
            // advance: the cell the crown moves into is spread over the (up to three) gas cells ahead of it
            let ci = cid(xo + sg, y);
            if (tile_of(ci) != 0u) { break; }
            let sc = ld0(ci);
            var ids = array<u32, 3>(0u, 0u, 0u);
            var n = 0u;
            for (var k = 2; k <= 4; k++) {
                let fk = cid(xo + sg * k, y);
                let tf = tile_of(fk);
                if (tf != 0u && tf != T_VALVE_O) { break; }
                ids[n] = fk;
                n += 1u;
            }
            if (n > 0u) {
                let inv = 1.0 / f32(n);
                let work = pres(sc) * inv;
                for (var k = 0u; k < n; k++) {
                    var sf = ld0(ids[k]);
                    sf = S(sf.a + sc.a * inv, sf.b + sc.b * inv);
                    sf.b.z += work;
                    st0(ids[k], sf);
                }
            }
            META[ci] = T_PISTON | (p << 8u);
            let ri = cid(xo - sg * (th - 1), y);
            META[ri] = 0u;
            st0(ri, air_state(1.0, 1.0));
            xo += sg;
        } else {
            // retreat: the vacated cell takes an equal share from the (up to three) gas cells ahead, and the gas
            // does expansion work on the piston
            let ci = cid(xo, y);
            var ids = array<u32, 3>(0u, 0u, 0u);
            var n = 0u;
            for (var k = 1; k <= 3; k++) {
                let fk = cid(xo + sg * k, y);
                let tf = tile_of(fk);
                if (tf != 0u && tf != T_VALVE_O) { break; }
                ids[n] = fk;
                n += 1u;
            }
            if (n > 0u) {
                let share = 1.0 / f32(n + 1u);
                var acc = S(vec4<f32>(0.0), vec4<f32>(0.0));
                var pavg = 0.0;
                for (var k = 0u; k < n; k++) {
                    let sf = ld0(ids[k]);
                    acc = S(acc.a + sf.a * share, acc.b + sf.b * share);
                    pavg += pres(sf);
                }
                pavg = pavg / f32(n);
                let wk = pavg * share;
                for (var k = 0u; k < n; k++) {
                    var sf = ld0(ids[k]);
                    sf = S(sf.a * (1.0 - share), sf.b * (1.0 - share));
                    sf.b.z -= wk;
                    st0(ids[k], sf);
                }
                var sn = acc;
                sn.b.z -= wk;
                st0(ci, sn);
            } else {
                st0(ci, air_state(1.0, 1.0));
            }
            META[ci] = 0u;
            let ri = cid(xo - sg * th, y);
            META[ri] = T_PISTON | (p << 8u);
            xo -= sg;
        }
    }
}

// Opens/closes timed valves and injectors (cell toggle list) and commits the piston positions of this step.
@compute @workgroup_size(64)
fn engine_valve(@builtin(global_invocation_id) g: vec3<u32>) {
    if (g.x < 8u && g.x < AUX[ENG + 2u]) {
        let b = ENG_PIST + g.x * 8u;
        let xo = bitcast<i32>(AUX[b + 6u]);
        let xt = bitcast<i32>(AUX[b + 7u]);
        AUX[b + 6u] = bitcast<u32>(xo + clamp(xt - xo, -2, 2));
    }
    if (g.x >= AUX[ENG + 3u]) { return; }
    let e = AUX[ENG_CELLS + g.x];
    let ci = e & 0xFFFFFu;
    let kind = e >> 20u;
    let m = META[ci];
    let t = m & 0xFFu;
    let id = min((m >> 8u) & 0xFFu, 15u);
    var want_open = false;
    var closed_t = T_VALVE_C;
    var open_t = T_VALVE_O;
    if (kind == 0u) {
        want_open = AUX[ENG_VALVE + id * 4u + 3u] != 0u;
    } else {
        want_open = AUX[ENG_INJ + id * 4u + 3u] != 0u;
        closed_t = T_INJ_OFF;
        open_t = T_INJ_ON;
    }
    if (want_open && t == closed_t) {
        // open: the cell becomes gas; start from the average of its ordinary gas neighbours (else ambient air)
        var acc = S(vec4<f32>(0.0), vec4<f32>(0.0));
        var cnt = 0.0;
        let x = i32(ci % P.w);
        let y = i32(ci / P.w);
        for (var k = 0; k < 4; k++) {
            let j = cid(x + OFF[k].x, y + OFF[k].y);
            if (j != ci && tile_of(j) == 0u) {
                let sj = ld0(j);
                acc = S(acc.a + sj.a, acc.b + sj.b);
                cnt += 1.0;
            }
        }
        var ns = air_state(1.0, 1.0);
        if (cnt > 0.0) { ns = S(acc.a / cnt, acc.b / cnt); }
        st0(ci, ns);
        META[ci] = open_t | (id << 8u);
    } else if (!want_open && t == open_t) {
        META[ci] = closed_t | (id << 8u);
    }
}
