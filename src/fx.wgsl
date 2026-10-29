// VFX field packing (runs once per frame after the simulation and vis.wgsl's colorize/bloom passes).
//
// Writes NL = 10 cell-resolution HDR layers into the OUT buffer (packed rgba16f), which vfx.rs copies into one
// W x H x 9 Rgba16Float texture array that render.wgsl samples with B-spline (bicubic) filtering:
//   L0 A  gas emission (linear HDR rgb), a = heat-haze strength (gas only)
//   L1 B  cloud: premultiplied albedo rgb (smoke, steam, methane, oxygen), a = opacity.
//         Analytic views (1,2,3,5): display colour rgb, a = 1.
//   L2 C  solid layer: albedo rgb (Fire view: linear; other views: display colour), a = code = kind + 16 * material
//         kind: 0 gas, 1 wall, 2 burnable wall (shrinks, remaining in F.a), 3 powder, 4 liquid, 5 igniter,
//               6 fuel inlet, 7 O2 inlet, 8 air inlet, 9 vent, 10..13 fan (dir = kind - 10)
//   L3 D  two flow-advected texture-coordinate displacements (xy layer 0, zw layer 1) for two-phase flow noise
//   L4 E  solid/matter emission (linear HDR rgb, crisp), a = liquid mask
//   L5 F  r soot opacity, g flame-front intensity, b pressure - 1 (atm), a remaining fraction (burnable walls)
//   L6 G  tight bloom rgb, a = x of the unit direction toward brighter wide bloom (fire-light direction)
//   L7 H  wide bloom rgb (upsampled from quarter res), a = y of that direction
//   L9 V  velocity (cells / time unit) xy, z = turbulence weight from |vorticity|, a = 0 (drives detail.wgsl)
//   L8 M  coverage masks for smooth contours: r liquid, g powder, b wall/igniter (burnable walls: remaining
//         fraction, so they recede smoothly as they burn), a = 0
//   L10 K chemistry seed for detail.wgsl's colour tracers: rgb = flame-emitter colour (max component = strength)
//         of burning matter touching this gas cell (Na yellow, K lilac, Cu green-blue, S blue, Mg/Al white...),
//         a = white oxide smoke source (Al2O3, MgO, K2CO3, NH4Cl/HCl fog)
// Gas layers (A, B, F.rg) are dilated into solid cells from their gas neighbours so filtered lookups next to walls
// do not darken. Entry `noisegen` (run once) writes a 256x256 tileable noise texture after the NL layers.
// Own bind group (the simulation layout is at its storage-buffer limit).

struct Params {
    w: u32, h: u32, nops: u32, view: u32,
    dt: f32, p_fuel: f32, fan_pow: f32, time: f32,
    ign_on: u32, frame: u32, srgb: u32, _p1: u32,
    p_o2: f32, p_air: f32, cam_x: f32, cam_y: f32,
    cam_w: f32, cam_h: f32, wall_dt: f32, gravity: f32,
}
struct FxParams { dt: f32, ph0: f32, ph1: f32, reset: u32, src: u32, stime: f32, _b: u32, _c: u32 }

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var<uniform> FP: FxParams;
@group(0) @binding(2) var<storage, read> U0: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read> META: array<u32>;
@group(0) @binding(4) var<storage, read> U2: array<vec4<f32>>;
@group(0) @binding(5) var<storage, read_write> UVB: array<vec4<f32>>; // ping-pong halves [0,N) / [N,2N)
@group(0) @binding(6) var<storage, read> U1: array<vec4<f32>>;
@group(0) @binding(7) var<storage, read> WALL: array<vec4<f32>>;
@group(0) @binding(8) var<storage, read_write> OUT: array<vec2<u32>>;

// must match sim.wgsl
const RS = vec4<f32>(1.80625, 0.903125, 1.032142857, 1.111538462);
// number of layers in OUT / the texture array (must match LAYERS in vfx.rs)
const NL: u32 = 11u;
const CVS = vec4<f32>(5.827, 2.258, 2.580, 4.446);

fn cid(x: i32, y: i32) -> u32 {
    return u32(clamp(y, 0, i32(P.h) - 1)) * P.w + u32(clamp(x, 0, i32(P.w) - 1));
}
fn solid_t(t: u32) -> bool { return t == 1u || t == 6u || t == 8u; }
fn fancy() -> bool { return P.view == 0u || P.view == 6u; }

// Flame-emitter colour of a burning condensed element (atomic/molecular band emission and incandescent oxide
// smoke), keyed by the matter element id (matter.wgsl ids; walls 0-9 never reach here except solid propellant 8).
// rgb: emitter hue (max component 1 = strong line emitter, smaller = weak tint over the normal flame colour),
// a: white smoke yield. Elements without a characteristic colour return 0 (the flame keeps its blackbody/soot look).
fn emitter(e: u32) -> vec4<f32> {
    switch e {
        case 8u: { return vec4<f32>(1.0, 0.86, 0.62, 1.0); }    // APCP (AP + Al): white-orange, thick Al2O3 smoke
        case 12u: { return vec4<f32>(0.22, 0.38, 1.0, 0.25); }  // sulfur: blue (S2 bands), SO2 haze
        case 13u: { return vec4<f32>(0.70, 0.45, 1.0, 0.70); }  // KNO3: potassium lilac, K2CO3 smoke
        case 15u: { return vec4<f32>(1.0, 0.97, 0.92, 1.0); }   // aluminium: white-hot, Al2O3 smoke
        case 16u: { return vec4<f32>(0.0, 0.0, 0.0, 0.45); }    // ammonium perchlorate: HCl / NH4Cl fog
        case 17u: { return vec4<f32>(1.0, 0.80, 0.55, 0.5); }   // iron oxide (thermite): white-orange
        case 21u: { return vec4<f32>(0.30, 0.48, 1.0, 0.0); }   // ethanol: faint blue, clean
        case 28u, 29u, 43u: { return vec4<f32>(1.0, 0.70, 0.10, 0.15); } // NaCl / brine / NaOH: sodium D line
        case 39u: { return vec4<f32>(1.0, 1.0, 1.0, 1.0); }     // magnesium: blinding white, MgO smoke
        case 40u: { return vec4<f32>(1.0, 0.62, 0.25, 0.1); }   // iron filings: orange sparks
        case 41u: { return vec4<f32>(1.0, 0.68, 0.08, 0.6); }   // sodium: intense yellow, Na2O smoke
        case 42u: { return vec4<f32>(0.78, 0.48, 1.0, 0.5); }   // potassium: lilac
        case 47u, 60u: { return vec4<f32>(0.72, 0.45, 1.0, 0.4); } // KClO4 / KMnO4: potassium lilac
        case 48u: { return vec4<f32>(1.0, 0.66, 0.30, 0.0); }   // nitrocellulose: clean orange flash, no smoke
        case 57u: { return vec4<f32>(1.0, 0.58, 0.30, 1.0); }   // gunpowder: orange, dense white-grey smoke
        case 59u: { return vec4<f32>(0.15, 1.0, 0.55, 0.2); }   // copper oxide: green-blue (CuO / CuCl bands)
        case 65u: { return vec4<f32>(1.0, 0.60, 0.28, 0.5); }   // fuse (black-powder core): orange, smoke
        default: { return vec4<f32>(0.0); }
    }
}

// seed for the colour tracers from the burning neighbours of gas cell (x, y); tg = gas temperature (T units)
fn chem_seed(x: i32, y: i32, tg: f32) -> vec4<f32> {
    var sd = vec4<f32>(0.0);
    var offs = array<vec2<i32>, 4>(vec2<i32>(1, 0), vec2<i32>(-1, 0), vec2<i32>(0, 1), vec2<i32>(0, -1));
    for (var k = 0; k < 4; k++) {
        let j = cid(x + offs[k].x, y + offs[k].y);
        let mj = META[j];
        let tj = mj & 0xFFu;
        let id = (mj >> 8u) & 0xFFu;
        let w = WALL[j];
        var e = 0u;
        var on = 0.0;
        if (tj == 8u) {
            e = id;
            let kind = u32(floor(max(w.z, 0.0) / 1000.0));
            // burning / reacting / decomposing (hot products, surface burning, H2 release) or simply sitting in a
            // flame (salts colour any flame they are heated in)
            let burning = select(0.0, 1.0, kind >= 3u && kind <= 5u && w.z > 1.0) * smoothstep(1.3, 2.5, w.x);
            on = max(burning, smoothstep(3.5, 5.0, max(tg, w.x)));
        } else if (tj == 1u && id == 8u && w.w < 0.999 && w.x > 2.0) {
            e = 8u;
            on = 1.0;
        }
        if (on > 0.0) {
            let em = emitter(e);
            sd = max(sd, em * on);
        }
    }
    return sd;
}

fn put(layer: u32, i: u32, v0: vec4<f32>) {
    var v = select(vec4<f32>(0.0), v0, v0 == v0);
    v = clamp(v, vec4<f32>(-60000.0), vec4<f32>(60000.0));
    OUT[layer * P.w * P.h + i] = vec2<u32>(pack2x16float(v.xy), pack2x16float(v.zw));
}

fn vel_at(i: u32) -> vec2<f32> {
    if (solid_t(META[i] & 0xFFu)) { return vec2<f32>(0.0); }
    let a = U0[2u * i];
    let b = U0[2u * i + 1u];
    let rho = max(a.x + a.y + a.z + a.w, 0.002);
    let v = vec2<f32>(b.x, b.y) / rho;
    return select(vec2<f32>(0.0), v, v == v);
}

fn uv_old(x: i32, y: i32) -> vec4<f32> {
    return UVB[FP.src * P.w * P.h + cid(x, y)];
}
// bilinear lookup of the previous displacement field at a continuous cell position
fn uv_at(p: vec2<f32>) -> vec4<f32> {
    let q = p - 0.5;
    let f = floor(q);
    let t = q - f;
    let x = i32(f.x);
    let y = i32(f.y);
    return mix(mix(uv_old(x, y), uv_old(x + 1, y), t.x), mix(uv_old(x, y + 1), uv_old(x + 1, y + 1), t.x), t.y);
}

struct GasF {
    em: vec3<f32>,
    haze: f32,
    cloud: vec4<f32>,
    soot: f32,
    burn: f32,
    pdev: f32,
}

fn gas_fields(i: u32) -> GasF {
    var g: GasF;
    let base = U2[2u * i];
    let emr = U2[2u * i + 1u];
    let a = max(U0[2u * i], vec4<f32>(0.0));
    let b = U0[2u * i + 1u];
    let rho = max(a.x + a.y + a.z + a.w, 0.002);
    let ke = 0.5 * (b.x * b.x + b.y * b.y) / rho;
    let p = dot(a, RS) / max(dot(a, CVS), 1e-6) * (b.z - ke);
    let tt = emr.a;
    let soot = max(b.w, 0.0);
    let molt = max(dot(a, RS), 1e-6);
    let xf = a.x * RS.x / molt;
    let xo = max(a.y - 0.304 * a.z, 0.0) * RS.y / molt;
    let xox = a.y * RS.y / molt;
    let xp = a.w * RS.w / molt;
    g.pdev = clamp(p - 1.0, -1.0, 40.0);
    g.haze = smoothstep(1.25, 4.5, tt);
    // smoke opacity: wood/candle soot loads (~0.003-0.02) give a visible, thinning grey-brown plume (0.2-0.75)
    g.soot = 1.0 - exp(-max(soot - 0.0006, 0.0) * 70.0); // threshold: trace soot (clean exhaust) stays clear
    g.burn = clamp(sqrt(xf * xox) * 4.0, 0.0, 1.0) * smoothstep(2.0, 4.0, tt);
    g.em = vec3<f32>(0.0);
    if (fancy()) {
        g.em = min(emr.rgb, vec3<f32>(60.0));
        // luminous soot zone of diffusion flames: lies on the FUEL side of the reaction sheet, a few cells thick.
        // Fuel-rich gas near a front (tight-bloom field = front emission spread over ~2 cells) glows with the
        // front's colour, so flames read as filled, self-luminous bodies instead of thin outlines.
        let near = U1[2u * i + 1u].rgb;
        let rich = smoothstep(0.06, 0.40, xf);
        g.em += min(near * 2.2 * rich, vec3<f32>(30.0));
    }
    var col = vec3<f32>(0.0);
    var tr = 1.0; // transmittance
    if (P.view == 4u) {
        // Gases view: display-space colours, strong
        let opf = 0.92 * (1.0 - exp(-xf * 4.0));
        let oo = 0.92 * (1.0 - exp(-xo * 4.0));
        let op = 0.65 * (1.0 - exp(-xp * 3.0));
        let os = 1.0 - exp(-soot * 60.0);
        col += vec3<f32>(1.0, 0.80, 0.10) * opf; tr *= 1.0 - opf;
        col += vec3<f32>(0.15, 0.62, 1.0) * oo; tr *= 1.0 - oo;
        col += vec3<f32>(0.86, 0.90, 0.95) * op; tr *= 1.0 - op;
        col = col * (1.0 - os) + vec3<f32>(0.30, 0.29, 0.28) * os; tr *= 1.0 - os;
        let hot = smoothstep(2.5, 6.0, tt);
        col = mix(col, vec3<f32>(1.0, 0.42, 0.08) * max(1.0 - tr, 0.4), hot * 0.7);
        tr = min(tr, 1.0 - hot * 0.4);
        g.cloud = vec4<f32>(col, 1.0 - tr);
    } else if (fancy()) {
        // linear albedos; render.wgsl lights them (ambient sky + fire light)
        let cold = 1.0 - smoothstep(2.5, 4.5, tt);
        // Real fuel gas, O2, CO2 and water vapour are transparent: only a faint tint so the user can still see
        // where unburnt gas is (the Gases view shows them properly). Diluted combustion products in a warm room
        // stay invisible (this was the "grey smoke slab"); only near-pure products/steam that have cooled
        // (condensing steam plumes from boiling water, exhaust right after quenching) become a white cloud.
        let opf = 0.015 * (1.0 - exp(-xf * 2.5)) * cold;
        let oo = 0.015 * (1.0 - exp(-xo * 2.5)) * cold;
        let st = 0.45 * smoothstep(0.62, 0.95, xp) * (1.0 - smoothstep(1.25, 1.7, tt));
        let os = g.soot;
        col += vec3<f32>(0.60, 0.56, 0.44) * opf; tr *= 1.0 - opf;   // fuel gas / pyrolysis vapour: warm pale haze
        col += vec3<f32>(0.36, 0.58, 1.0) * oo; tr *= 1.0 - oo;      // oxygen: pale blue
        col += vec3<f32>(0.80, 0.81, 0.83) * st; tr *= 1.0 - st;     // condensing steam: white
        col = col * (1.0 - os) + vec3<f32>(0.23, 0.21, 0.19) * os; tr *= 1.0 - os; // soot: dark grey-brown
        g.cloud = vec4<f32>(col, 1.0 - tr);
    } else {
        g.cloud = vec4<f32>(base.rgb, 1.0);
    }
    return g;
}

fn bloom_wide(cf: vec2<f32>) -> vec3<f32> {
    let qw = (P.w + 3u) / 4u;
    let qh = (P.h + 3u) / 4u;
    let base = 2u * qw * qh;
    let gp = cf / 4.0 - 0.5;
    let g0 = floor(gp);
    let t = clamp(gp - g0, vec2<f32>(0.0), vec2<f32>(1.0));
    let x0 = clamp(i32(g0.x), 0, i32(qw) - 1);
    let y0 = clamp(i32(g0.y), 0, i32(qh) - 1);
    let x1 = clamp(i32(g0.x) + 1, 0, i32(qw) - 1);
    let y1 = clamp(i32(g0.y) + 1, 0, i32(qh) - 1);
    let a = U1[2u * (base + u32(y0) * qw + u32(x0))].rgb;
    let b = U1[2u * (base + u32(y0) * qw + u32(x1))].rgb;
    let c = U1[2u * (base + u32(y1) * qw + u32(x0))].rgb;
    let d = U1[2u * (base + u32(y1) * qw + u32(x1))].rgb;
    return mix(mix(a, b, t.x), mix(c, d, t.x), t.y);
}

@compute @workgroup_size(16, 16)
fn fx(@builtin(global_invocation_id) g: vec3<u32>) {
    if (g.x >= P.w || g.y >= P.h) { return; }
    let i = g.y * P.w + g.x;
    let n = P.w * P.h;
    let m = META[i];
    let t = m & 0xFFu;
    let solid = solid_t(t);
    let x = i32(g.x);
    let y = i32(g.y);

    // ---- two-phase advected detail coordinates: uv(x) = x + d(x); semi-Lagrangian d_new(x) = d_old(x - v dt) - v dt
    let v = vel_at(i);
    let step = v * FP.dt;
    var d = uv_at(vec2<f32>(f32(g.x) + 0.5, f32(g.y) + 0.5) - step) - vec4<f32>(step, step);
    if ((FP.reset & 1u) != 0u) { d.x = 0.0; d.y = 0.0; }
    if ((FP.reset & 2u) != 0u) { d.z = 0.0; d.w = 0.0; }
    if (solid || !(d.x == d.x) || !(d.z == d.z)) { d = vec4<f32>(0.0); }
    d = clamp(d, vec4<f32>(-80.0), vec4<f32>(80.0));
    UVB[(1u - FP.src) * n + i] = d;
    put(3u, i, d);
    {
        let vxp = vel_at(cid(x + 1, y));
        let vxm = vel_at(cid(x - 1, y));
        let vyp = vel_at(cid(x, y + 1));
        let vym = vel_at(cid(x, y - 1));
        let vort = 0.5 * ((vxp.y - vxm.y) - (vyp.x - vym.x));
        put(9u, i, vec4<f32>(v, clamp(abs(vort) * 1.5, 0.0, 1.0), 0.0));
    }

    let base = U2[2u * i];
    let em = U2[2u * i + 1u];
    // bloom layers (all views; zero in analytic views since colorize emits nothing there) + fire-light direction
    let cc = vec2<f32>(f32(g.x) + 0.5, f32(g.y) + 0.5);
    let lw = vec3<f32>(0.2126, 0.7152, 0.0722);
    let lgx = dot(bloom_wide(cc + vec2<f32>(3.0, 0.0)) - bloom_wide(cc - vec2<f32>(3.0, 0.0)), lw);
    let lgy = dot(bloom_wide(cc + vec2<f32>(0.0, 3.0)) - bloom_wide(cc - vec2<f32>(0.0, 3.0)), lw);
    let lgl = length(vec2<f32>(lgx, lgy));
    let ldir = select(vec2<f32>(0.0, 1.0), vec2<f32>(lgx, lgy) / lgl, lgl > 1e-6);
    put(6u, i, vec4<f32>(U1[2u * i + 1u].rgb, ldir.x));
    put(7u, i, vec4<f32>(bloom_wide(cc), ldir.y));

    // ---- solid layer code
    let mat = (m >> 8u) & 0xFFu;
    var kind = 0u;
    var rem = 1.0;
    var liquid = 0.0;
    var semi = vec3<f32>(0.0);
    if (t == 1u) {
        kind = select(1u, 2u, mat >= 7u && mat <= 9u);
        rem = clamp(WALL[i].w, 0.0, 1.0);
        if (kind == 1u) { rem = 1.0; }
    } else if (t == 8u) {
        kind = select(3u, 4u, base.a > 1.5);
        liquid = select(0.0, 1.0, kind == 4u);
    } else if (t == 6u) {
        kind = 5u;
    } else if (t == 2u) { kind = 6u; }
    else if (t == 3u) { kind = 7u; }
    else if (t == 4u) { kind = 8u; }
    else if (t == 7u) { kind = 9u; }
    else if (t == 5u) { kind = 10u + min((m >> 16u) & 0xFFu, 3u); }
    let code = f32(kind + 16u * min(mat, 127u));
    var salb = vec3<f32>(0.0);
    if (solid) { salb = base.rgb; semi = em.rgb; }
    if (!fancy()) { semi = vec3<f32>(0.0); }
    put(2u, i, vec4<f32>(salb, code));
    var wmask = select(0.0, 1.0, kind == 1u || kind == 5u);
    if (kind == 2u) { wmask = sqrt(rem); }
    put(8u, i, vec4<f32>(liquid, select(0.0, 1.0, kind == 3u), wmask, 0.0));
    put(4u, i, vec4<f32>(min(semi, vec3<f32>(60.0)), liquid));

    // ---- gas layers (dilated into solids from gas neighbours)
    var gf: GasF;
    if (!solid) {
        gf = gas_fields(i);
    } else {
        var acc_em = vec3<f32>(0.0);
        var acc_cl = vec4<f32>(0.0);
        var acc_s = 0.0;
        var acc_b = 0.0;
        var acc_p = 0.0;
        var acc_h = 0.0;
        var wsum = 0.0;
        var offs = array<vec2<i32>, 4>(vec2<i32>(1, 0), vec2<i32>(-1, 0), vec2<i32>(0, 1), vec2<i32>(0, -1));
        for (var k = 0; k < 4; k++) {
            let o = offs[k];
            let j = cid(x + o.x, y + o.y);
            if (!solid_t(META[j] & 0xFFu)) {
                let q = gas_fields(j);
                acc_em += q.em; acc_cl += q.cloud; acc_s += q.soot; acc_b += q.burn; acc_p += q.pdev; acc_h += q.haze;
                wsum += 1.0;
            }
        }
        let iw = 1.0 / max(wsum, 1.0);
        gf.em = acc_em * iw; gf.cloud = acc_cl * iw; gf.soot = acc_s * iw; gf.burn = acc_b * iw; gf.pdev = acc_p * iw;
        gf.haze = acc_h * iw * 0.5;
        if (!fancy() && P.view != 4u) {
            // analytic views: walls keep their own colour under the gas colour lookups
            gf.cloud = vec4<f32>(base.rgb, 1.0);
        }
    }
    put(0u, i, vec4<f32>(gf.em, gf.haze));
    put(1u, i, gf.cloud);
    put(5u, i, vec4<f32>(gf.soot, gf.burn, gf.pdev, rem));
    var seed = vec4<f32>(0.0);
    if (!solid && fancy()) { seed = chem_seed(x, y, em.a); }
    put(10u, i, seed);
}

// ---------------- tileable noise (run once) ----------------

const NZ: u32 = 256u;

fn nhash(x: u32, y: u32, s: u32) -> u32 {
    var h = x * 0x9E3779B1u ^ y * 0x85EBCA77u ^ s * 0xC2B2AE3Du;
    h ^= h >> 15u;
    h *= 0x2C1B3C6Du;
    h ^= h >> 12u;
    h *= 0x297A2D39u;
    h ^= h >> 15u;
    return h;
}
fn wrapi(v: i32, per: i32) -> u32 { return u32(((v % per) + per) % per); }

fn grad(ix: i32, iy: i32, per: i32, s: u32) -> vec2<f32> {
    let h = nhash(wrapi(ix, per), wrapi(iy, per), s);
    let a = f32(h & 0xFFFFu) / 65536.0 * 6.2831853;
    return vec2<f32>(cos(a), sin(a));
}

// periodic gradient noise, period `per` lattice cells, range ~[-0.7, 0.7]
fn pnoise(p: vec2<f32>, per: i32, s: u32) -> f32 {
    let i = floor(p);
    let f = p - i;
    let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let ix = i32(i.x);
    let iy = i32(i.y);
    let a = dot(grad(ix, iy, per, s), f);
    let b = dot(grad(ix + 1, iy, per, s), f - vec2<f32>(1.0, 0.0));
    let c = dot(grad(ix, iy + 1, per, s), f - vec2<f32>(0.0, 1.0));
    let d = dot(grad(ix + 1, iy + 1, per, s), f - vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

fn fbm(uv: vec2<f32>, base: i32, oct: i32, s: u32) -> f32 {
    var sum = 0.0;
    var amp = 0.5;
    var per = base;
    var norm = 0.0;
    for (var k = 0; k < oct; k++) {
        sum += amp * pnoise(uv * f32(per), per, s + u32(k) * 101u);
        norm += amp;
        amp *= 0.5;
        per *= 2;
    }
    return sum / norm;
}

// periodic cellular noise: distance to the nearest jittered feature point
fn worley(uv: vec2<f32>, per: i32, s: u32) -> f32 {
    let p = uv * f32(per);
    let i = floor(p);
    let f = p - i;
    var dmin = 9.0;
    for (var yy = -1; yy <= 1; yy++) {
        for (var xx = -1; xx <= 1; xx++) {
            let h = nhash(wrapi(i32(i.x) + xx, per), wrapi(i32(i.y) + yy, per), s);
            let o = vec2<f32>(f32(h & 0xFFFFu), f32(h >> 16u)) / 65536.0;
            let dd = vec2<f32>(f32(xx), f32(yy)) + o - f;
            dmin = min(dmin, dot(dd, dd));
        }
    }
    return sqrt(dmin);
}

// r, g: two decorrelated fBm (detail / flame tongues); b: billowy cellular fBm (smoke puffs); a: ridged (wisps).
// Band-limited mip chain generated directly (each level drops the finest octave): rows [0,256) level 0 (256 wide),
// [256,384) level 1 (128), [384,448) level 2 (64), [448,480) level 3 (32); written after the NL layers in OUT.
@compute @workgroup_size(16, 16)
fn noisegen(@builtin(global_invocation_id) g: vec3<u32>) {
    if (g.x >= NZ || g.y >= 480u) { return; }
    var lvl = 0u;
    var row = g.y;
    var base = 0u;
    if (g.y >= 448u) { lvl = 3u; row = g.y - 448u; base = 256u * 256u + 128u * 128u + 64u * 64u; }
    else if (g.y >= 384u) { lvl = 2u; row = g.y - 384u; base = 256u * 256u + 128u * 128u; }
    else if (g.y >= 256u) { lvl = 1u; row = g.y - 256u; base = 256u * 256u; }
    let wl = NZ >> lvl;
    if (g.x >= wl) { return; }
    let uv = (vec2<f32>(f32(g.x), f32(row)) + 0.5) / f32(wl);
    let ol = i32(lvl);
    // domain-warped fBm (5 octaves at level 0: finest period 4 texels)
    let wq = vec2<f32>(fbm(uv + vec2<f32>(0.13, 0.71), 4, 3, 71u), fbm(uv + vec2<f32>(0.57, 0.29), 4, 3, 83u));
    let r = 0.5 + 1.0 * fbm(uv + wq * 0.06, 4, 5 - ol, 11u);
    let gg = 0.5 + 1.0 * fbm(uv - wq * 0.06, 4, 5 - ol, 977u);
    // billows: soft cellular puffs (smoothed by mixing with fBm); the finer cell octave fades out with level
    let w1 = 1.0 - worley(uv + wq * 0.04, 5, 5u);
    let w2 = 1.0 - worley(uv, 10, 6u);
    let w2f = mix(w2, 0.55, f32(lvl) / 3.0);
    let b = clamp(0.55 * w1 * w1 + 0.20 * w2f + 0.55 * fbm(uv, 4, max(4 - ol, 1), 313u) + 0.12, 0.0, 1.0);
    let rid = 1.0 - abs(fbm(uv + wq * 0.08, 4, 5 - ol, 2221u)) * 2.2;
    let a = clamp(rid * rid, 0.0, 1.0);
    let v = clamp(vec4<f32>(r, gg, b, a), vec4<f32>(0.0), vec4<f32>(1.0));
    OUT[NL * P.w * P.h + base + row * wl + g.x] = vec2<u32>(pack2x16float(v.xy), pack2x16float(v.zw));
}
