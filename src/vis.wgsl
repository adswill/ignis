// Visualisation passes (appended to sim.wgsl at load time, so all sim helpers and bindings are in scope).
//
// Scratch layout (U1/U2 are free after the simulation steps):
//   U2[2i]   = base colour (linear albedo / tint in Fire view, display colour in the other views), a = smoke opacity
//   U2[2i+1] = emitted light (linear), a = gas/wall temperature (T units, 300 K)
//   U1[2i]   = tight-bloom horizontal temp, later reused for the quarter-res bloom chain:
//              U1[2j] quarter-res downsample, U1[2(NQ+j)] wide horizontal, U1[2(2NQ+j)] wide bloom (final)
//   U1[2i+1] = tight bloom (final)
// Passes: colorize, blurh, blurv, bdown, bwideh, bwidev.

// ---------------- colour helpers ----------------

fn fire_ramp(lv: f32) -> vec3<f32> {
    let l = clamp(lv, 0.0, 1.0);
    let c0 = vec3<f32>(0.30, 0.0, 0.0);
    let c1 = vec3<f32>(0.95, 0.20, 0.02);
    let c2 = vec3<f32>(1.0, 0.58, 0.10);
    let c3 = vec3<f32>(1.0, 0.88, 0.45);
    let c4 = vec3<f32>(1.0, 1.0, 0.92);
    if (l < 0.22) { return mix(c0, c1, l / 0.22); }
    if (l < 0.5) { return mix(c1, c2, (l - 0.22) / 0.28); }
    if (l < 0.78) { return mix(c2, c3, (l - 0.5) / 0.28); }
    return mix(c3, c4, (l - 0.78) / 0.22);
}

fn heat_ramp(tv: f32) -> vec3<f32> {
    var st = array<vec3<f32>, 8>(
        vec3<f32>(0.0, 0.0, 0.0), vec3<f32>(0.05, 0.0, 0.4), vec3<f32>(0.0, 0.3, 1.0),
        vec3<f32>(0.0, 0.9, 0.8), vec3<f32>(0.2, 0.95, 0.1), vec3<f32>(1.0, 0.95, 0.0),
        vec3<f32>(1.0, 0.2, 0.0), vec3<f32>(1.0, 1.0, 1.0));
    let f = clamp(tv, 0.0, 1.0) * 7.0;
    let k = min(u32(floor(f)), 6u);
    return mix(st[k], st[k + 1u], f - f32(k));
}

fn hsv(h: f32, s: f32, v: f32) -> vec3<f32> {
    let k = vec3<f32>(1.0, 2.0 / 3.0, 1.0 / 3.0);
    let p = abs(fract(vec3<f32>(h) + k) * 6.0 - 3.0);
    return v * mix(vec3<f32>(1.0), clamp(p - 1.0, vec3<f32>(0.0), vec3<f32>(1.0)), s);
}

fn stress_ramp(r: f32) -> vec3<f32> {
    let k = clamp(r, 0.0, 1.2);
    if (k < 0.5) { return mix(vec3<f32>(0.10, 0.45, 0.20), vec3<f32>(0.85, 0.85, 0.15), k / 0.5); }
    if (k < 1.0) { return mix(vec3<f32>(0.85, 0.85, 0.15), vec3<f32>(1.0, 0.15, 0.05), (k - 0.5) / 0.5); }
    return vec3<f32>(1.0, 0.1 + 0.4 * sin(P.time * 20.0), 0.6);
}

fn to_lin(c: vec3<f32>) -> vec3<f32> { return pow(max(c, vec3<f32>(0.0)), vec3<f32>(2.2)); }

fn vhash(x: u32, y: u32, f: u32) -> f32 {
    var h = x * 0x9E3779B1u ^ y * 0x85EBCA77u ^ f * 0xC2B2AE3Du;
    h ^= h >> 15u;
    h *= 0x2C1B3C6Du;
    h ^= h >> 12u;
    h *= 0x297A2D39u;
    h ^= h >> 15u;
    return f32(h & 0xFFFFu) / 65535.0;
}

fn vnoise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = p - i;
    let u = f * f * (3.0 - 2.0 * f);
    let x = u32(i32(i.x) + 4096);
    let y = u32(i32(i.y) + 4096);
    let a = vhash(x, y, 17u);
    let b = vhash(x + 1u, y, 17u);
    let c = vhash(x, y + 1u, 17u);
    let d = vhash(x + 1u, y + 1u, 17u);
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// Blackbody colour: Planck's law sampled at the R/G/B primaries' dominant wavelengths, normalised to red = 1.
// 1000 K -> deep red, 1500 K -> orange, 2000 K -> orange-yellow, 3000 K -> pale yellow (tone mapping then
// pushes the bright ones through yellow to white, like film/eyes do).
fn planck_col(tk: f32) -> vec3<f32> {
    let lam = vec3<f32>(0.625, 0.550, 0.465); // micrometres
    let t = max(tk, 500.0);
    let x = 14388.0 / (lam * t);
    // ratio form avoids overflow: I_c / I_r = (lam_r/lam_c)^5 * (exp(x_r)-1)/(exp(x_c)-1)
    let l5 = pow(lam.x / lam, vec3<f32>(5.0));
    let e = exp(max(x.x - x, vec3<f32>(-80.0))); // exp(x_r - x_c), x_c >= x_r
    let c = l5 * e * (1.0 - exp(-x.x)) / max(1.0 - exp(-x), vec3<f32>(1e-6));
    // slight extra saturation: dim incandescence reads as cherry red rather than brown after display gamma
    return pow(c, vec3<f32>(1.3));
}

// Display brightness of incandescence. Real radiance at 600 nm rises ~e^(-24000/T); a camera/eye compresses that,
// so use a steep power law: 1000 K ~ 0.017, 1500 K ~ 0.18, 2000 K = 1, 2500 K ~ 3.3 (tone mapping rolls it off).
fn bb_int(tk: f32) -> f32 {
    let r = max(tk, 0.0) / 2000.0;
    let r2 = r * r;
    let i = r2 * r2 * r2;
    return i * 10.0 / (9.0 + i); // soft cap ~10: very hot stuff stays bright without turning into a white wall
}

fn incandescence(tt: f32) -> vec3<f32> {
    let tk = tt * 300.0;
    return planck_col(tk) * bb_int(tk);
}

// Flames in the coarse solver run cooler than real ones (~1400-1900 K instead of ~1900-2300 K). For the look, gas
// emission uses a "visual temperature" lifted by up to 400 K above ~1300 K, so burning gas reads orange-yellow
// rather than dull red, while walls keep their true incandescent colours.
fn flame_tk(tk: f32) -> f32 { return tk + 400.0 * smoothstep(900.0, 1500.0, tk); }
fn flame_glow(tt: f32) -> vec3<f32> {
    let tk = flame_tk(tt * 300.0);
    return planck_col(tk) * bb_int(tk);
}

// Display-space albedo of a wall cell; burnable solids get procedural textures and char as they burn away.
fn mat_albedo(m: u32, x: u32, y: u32, rem: f32) -> vec3<f32> {
    var c = M_COL[m];
    let fx = f32(x);
    let fy = f32(y);
    if (m == 7u) {
        // wood: wavy grain lines along x, darker late-wood rings, a few knots
        let wob = vnoise(vec2<f32>(fx / 14.0, fy / 5.0)) * 3.0 + vnoise(vec2<f32>(fx / 40.0, fy / 9.0)) * 4.0;
        let ring = fract((fy + wob) * 0.42);
        let late = smoothstep(0.55, 0.85, ring) * (1.0 - smoothstep(0.9, 1.0, ring));
        c = mix(vec3<f32>(0.62, 0.42, 0.22), vec3<f32>(0.36, 0.22, 0.10), late);
        c *= 0.92 + 0.16 * vhash(x, y, 5u);
        // char: black with a dull grey ash sheen as it burns away
        let ch = smoothstep(0.0, 0.7, 1.0 - rem);
        c = mix(c, vec3<f32>(0.07, 0.06, 0.055) + vec3<f32>(0.10) * vhash(x, y, 9u) * ch, ch);
    } else if (m == 8u) {
        // composite propellant: pale grey-pink rubbery grain with bright aluminium specks
        c = vec3<f32>(0.74, 0.64, 0.64) * (0.94 + 0.12 * vnoise(vec2<f32>(fx, fy) / 3.0));
        if (vhash(x, y, 13u) < 0.05) { c = vec3<f32>(0.92, 0.92, 0.95); }
        c = mix(c, vec3<f32>(0.30, 0.24, 0.22), smoothstep(0.3, 1.0, 1.0 - rem));
    } else if (m == 9u) {
        // explosive: orange-tan blocks with dark diagonal hazard stripes
        c = vec3<f32>(0.86, 0.56, 0.24) * (0.93 + 0.12 * vhash(x, y, 21u));
        if (((x + y) / 4u) % 2u == 0u) { c = mix(c, vec3<f32>(0.20, 0.13, 0.06), 0.55); }
    }
    return c;
}

// Display-space albedo of a condensed-matter cell (powders: per-grain shade; liquids: smooth with slow ripples).
fn matter_albedo(e: u32, x: u32, y: u32) -> vec3<f32> {
    return matter_look(e, x, y);   // per-element styles: matter_look.wgsl
}

// ---------------- colorize ----------------

// Writes per-cell base colour to U2[2i] and glow emission to U2[2i+1].
@compute @workgroup_size(16, 16)
fn colorize(@builtin(global_invocation_id) g: vec3<u32>) {
    if (g.x >= P.w || g.y >= P.h) { return; }
    let i = g.y * P.w + g.x;
    var b = vec3<f32>(0.0);
    var gl = vec3<f32>(0.0);
    var op = 0.0;   // smoke opacity (fire view)
    var tcell = 1.0;
    let tl = tile_of(i);
    let v = select(P.view, 0u, P.view == 6u); // 6 = Fire with particle trails
    if (tl == 1u) {
        let m = mat_of(i);
        let w = WALL[i];
        tcell = w.x;
        op = select(1.0, clamp(w.w, 0.0, 1.0), m >= 7u); // remaining solid fraction (render shrinks burnables)
        if (v == 5u) {
            b = stress_ramp(w.z) * (1.0 - 0.5 * min(w.y, 1.0));
        } else if (v == 2u) {
            b = heat_ramp((w.x - 0.3) / 9.7) * 0.85 + 0.1;
        } else if (v == 0u) {
            // realistic: material albedo (linear), heat tint, damage, blackbody glow
            let dmg = clamp(w.y, 0.0, 1.0);
            var alb = to_lin(mat_albedo(m, g.x, g.y, w.w));
            // thin-oxide temper colours on warm metal (straw -> bronze -> blue), not on ceramic
            if (m != 5u && m < 7u) {
                let tq = clamp((w.x - 1.6) / 1.2, 0.0, 1.0);
                let temper = mix(vec3<f32>(1.0, 0.85, 0.55), vec3<f32>(0.55, 0.55, 1.0), smoothstep(0.4, 1.0, tq));
                alb *= mix(vec3<f32>(1.0), temper, 0.35 * sin(tq * 3.14159));
            }
            // damage: darkened, charred surface plus crack pixels
            alb *= 1.0 - 0.6 * dmg;
            // cracks: thin ridge lines of a coarse value-noise field that widen with damage, plus spalled pits
            let cn = vnoise(vec2<f32>(f32(g.x), f32(g.y)) / 5.0);
            let cr = vhash(g.x, g.y, 91u);
            let crack = dmg > 0.08 && (abs(cn - 0.5) < 0.02 + 0.07 * dmg || cr < dmg * dmg * 0.12);
            if (crack) { alb *= 0.15; }
            let tk = w.x * 300.0;
            // in the dark, very hot metal is dominated by its own light
            alb *= 1.0 - 0.95 * smoothstep(700.0, 1250.0, tk);
            b = alb;
            var e = 1.5;
            if (crack) { e = 1.8; } // cracks expose hotter interior
            gl = incandescence(w.x) * e;
        } else {
            let lvl = clamp((w.x - 2.2) / 5.0, 0.0, 1.0);
            var c = mat_albedo(m, g.x, g.y, w.w) * (1.0 - 0.5 * min(w.y, 1.0));
            if (lvl > 0.0) {
                let f = fire_ramp(lvl);
                let a = min(lvl * 2.0, 1.0);
                c = c * (1.0 - a) + f * a * 1.1;
            }
            b = c;
        }
    } else if (tl == MATTER) {
        // real element id (10..N_ELEM-1): mat_of() clamps to 25, which drew every element >= 26 (TNT, Mg, gunpowder…)
        // with the ash colour
        let e = elem_of(i);
        let w = WALL[i];
        tcell = w.x;
        let liq = E_PHASE[e] == 2u;
        op = select(1.0, 2.0, liq); // phase marker for render.wgsl
        let alb = matter_albedo(e, g.x, g.y);
        if (v == 2u) {
            b = heat_ramp((w.x - 0.3) / 9.7) * 0.85 + 0.1;
        } else if (v == 4u) {
            b = alb;
        } else if (v == 0u) {
            let tk = w.x * 300.0;
            b = to_lin(alb) * (1.0 - 0.9 * smoothstep(800.0, 1400.0, tk));
            gl = incandescence(w.x) * 1.6;
            // liquid oxygen: faint icy self-glow so it reads as frosty in the dark
            if (e == 22u) { gl += vec3<f32>(0.010, 0.025, 0.06) * (0.8 + 0.4 * vhash(g.x, g.y, P.frame / 4u)); }
        } else {
            b = alb * 0.55;
        }
    } else if (tl == 6u) {
        if (P.ign_on != 0u) {
            let fl = 0.8 + 0.2 * sin(P.time * 6.0);
            b = vec3<f32>(fl, 0.35, 0.12);
            if (v == 0u) {
                b = vec3<f32>(0.25, 0.05, 0.02);
                gl = vec3<f32>(1.6, 0.55, 0.12) * fl;
            }
        } else {
            b = vec3<f32>(0.45, 0.12, 0.1);
            if (v == 0u) { b = to_lin(b); }
        }
    } else {
        let s = ld0(i);
        let rho = max(rho_s(s), 1e-3);
        let tt = temp(s);
        tcell = tt;
        let p = pres(s);
        let soot = max(s.b.w, 0.0);
        if (v == 1u) {
            let q = (p - 1.0) / 1.5;
            b = vec3<f32>(clamp(q, 0.0, 1.0), clamp(q * 0.28, 0.0, 1.0), clamp(-q * 1.2, 0.0, 1.0));
            if (q > 1.0) { b.y = min(q - 1.0, 1.0); b.z = clamp(q - 1.5, 0.0, 1.0); }
        } else if (v == 2u) {
            b = heat_ramp((tt - 0.3) / 9.7);
        } else if (v == 3u) {
            let vx = s.b.x / rho;
            let vy = s.b.y / rho;
            let sp = sqrt(vx * vx + vy * vy);
            b = hsv(atan2(vy, vx) / 6.2831853, 0.85, min(sp / 2.5, 1.0));
        } else if (v == 4u) {
            // dim composition field under the particles (same palette as particles.wgsl view 4)
            let molt = max(dot(max(s.a, vec4<f32>(0.0)), RS), 1e-6);
            let xf = max(s.a.x, 0.0) * RS.x / molt;
            let xo = max(s.a.y - 0.304 * s.a.z, 0.0) * RS.y / molt;
            let xp = max(s.a.w, 0.0) * RS.w / molt;
            b = (vec3<f32>(1.0, 0.90, 0.10) * xf + vec3<f32>(0.10, 0.75, 1.0) * xo + vec3<f32>(0.92, 0.94, 0.97) * xp * 0.6) * 0.26;
            b = mix(b, vec3<f32>(0.10, 0.095, 0.09), 1.0 - exp(-soot * 40.0));
            b += vec3<f32>(0.035, 0.035, 0.045);
        } else if (v == 5u) {
            b = vec3<f32>(0.12) * min(rho, 2.0);
        } else {
            // ---- realistic fire (emission only; clouds/smoke are packed by fx.wgsl) ----
            let o2x = max(s.a.y - 0.304 * s.a.z, 0.0);
            op = 1.0 - exp(-soot * 38.0);
            b = vec3<f32>(0.075, 0.069, 0.062);
            let tk = tt * 300.0;
            // soot incandescence: Planck colour, very steep with T, emissivity grows with soot load
            let emis = 1.0 - exp(-soot * 160.0);
            // volumetric soot body: the whole sooty hot volume glows (optically thick flames are filled, not shells)
            let emis_v = 1.0 - exp(-soot * 320.0);
            // steeper in T than before (x T^2 relative to 1600 K): 1100-1300 K soot reads dull red, ~1500 K orange, and only
            // the hottest sheets reach yellow-white, so a sooty flame body has a bright core and dim red tips
            let tsteep = clamp(tk * tk / (1600.0 * 1600.0), 0.0, 1.6);
            let sg = (flame_glow(tt) * emis * 1.8 + planck_col(flame_tk(tk)) * bb_int(flame_tk(tk)) * emis_v * 2.2 * smoothstep(800.0, 1150.0, tk)) * tsteep;
            // flame front. Fuel-rich (diffusion) fronts are luminous: soot forms on the fuel side and glows
            // yellow-orange; lean / premixed fronts show only CH*/C2* chemiluminescence (thin blue sheet).
            let w = burn_rate(s);
            let fl = 1.0 - exp(-w * 30.0);
            let phi = s.a.x / max(o2x, 1e-4) * 4.0;          // equivalence ratio (CH4 : O2 = 1 : 4 by mass)
            let rich = smoothstep(0.7, 2.2, phi);
            let blue = (mix(vec3<f32>(0.012, 0.13, 1.0), vec3<f32>(0.06, 0.42, 1.25), fl * fl) + vec3<f32>(0.03, 0.01, 0.0)) * fl * 1.5;
            // luminous flames saturate quickly with burn rate (a thin sheet of glowing soot is already bright)
            let fll = 1.0 - exp(-w * 90.0);
            // hue and brightness from the local temperature: slow distributed reaction inside a cool (~800 K) fuel-rich
            // body stays dark; real fronts (~1400-2000 K) glow orange to yellow-white
            let tfl = flame_tk(tk) + 150.0;
            let lum_flame = planck_col(tfl) * fll * 1.3 * clamp(bb_int(tfl) * 1.5, 0.0, 1.0);
            var front = mix(blue, lum_flame, rich * 0.85);
            gl = sg + front / (1.0 + 3.0 * (sg.x + sg.y));
            // unresolved luminous body: the coarse grid cannot resolve the wrinkled flame sheets inside hot,
            // fuel-rich, sooty pyrolysis gas (campfires, pool fires, fuel-rich fireballs), so it only burns at the
            // blob's outline. Real flames there are self-luminous through the volume (sub-grid flamelets + hot soot):
            // lift the visual temperature of hot fuel-rich sooty gas toward ~1900 K, brightness steep in T.
            let yf = max(s.a.x, 0.0) / rho;
            let richb = smoothstep(0.02, 0.12, yf) * smoothstep(0.0, 0.004, soot + 0.1 * yf * yf);
            // The lift grows with the real temperature, so the cool pyrolysis core (~500-700 K) stays dull red under
            // its smoke and only gas close to the reacting sheet (~900-1400 K) reaches the yellow-white of a flame.
            let tvb = min(tk + (300.0 * smoothstep(450.0, 700.0, tk) + 800.0 * smoothstep(700.0, 1200.0, tk)) * richb, 1950.0);
            let emb = (1.0 - exp(-(soot * 140.0 + yf * 2.0))) * richb;
            gl += planck_col(tvb) * bb_int(tvb) * emb * 1.6;
            // hot burnt gas: H2O/CO2 band emission + afterburning, drawn as a warm glowing body with a blackbody hue
            // (flatter in T than soot so exhaust plumes and fireballs read as fire; shock diamonds flare)
            // Brightness AND hue come from temperature only (Planck x steep camera response): a 3000 K core is
            // white-yellow, 2000 K orange, 1300 K dull red, cooler mixed edges fade out (no flat product-mass fill).
            let prod = clamp(s.a.w / rho * 3.0, 0.0, 1.0);
            let thick_g = 0.35 + 0.65 * min(rho * 3.0, 1.0);
            let tkg = flame_tk(tk);
            // (a) warm continuum of hot burnt gas: only where it carries soot (a sooty fireball / kerolox trail glows
            // orange); clean CO2/H2O is nearly transparent in the visible
            let soot_g = smoothstep(0.0003, 0.004, soot);
            gl += planck_col(tkg) * bb_int(tkg) * 0.30 * prod * thick_g * smoothstep(800.0, 1300.0, tk) * soot_g;
            // (b) clean exhaust (methalox / H2-like): optically thin band + chemiluminescent emission (OH, CO2*, H2O),
            // proportional to the product DENSITY and very steep in temperature, so the re-compressed hot cells of
            // shock diamonds (higher rho and T) flare while the expanded regions between them stay dark and
            // see-through; cool mixed far field (< ~1800 K) is invisible. Blue-violet, pink in the hottest diamonds.
            let rp = max(s.a.w, 0.0);
            let tr8 = pow(clamp(tk / 2600.0, 0.0, 1.6), 10.0);
            let gam = 1.0 + gm1(s);
            let spd2 = (s.b.x * s.b.x + s.b.y * s.b.y) / (rho * rho);
            let mach = sqrt(spd2 / max(gam * p / rho, 1e-4));
            // 2D-slice correction: slow recirculating hot gas (start-up vortex rings) is a thin torus in 3D, the
            // supersonic core a column -> weight the former down
            let core = 0.30 + 0.70 * smoothstep(0.6, 1.4, mach);
            let band = min(rp / 0.15 * tr8, 12.0) * core * (1.0 - 0.7 * soot_g);
            let bcol = mix(vec3<f32>(0.30, 0.20, 1.0), vec3<f32>(1.0, 0.42, 0.80), smoothstep(2450.0, 3100.0, tk));
            gl += mix(bcol, vec3<f32>(1.0, 0.82, 0.92), smoothstep(2850.0, 3400.0, tk)) * band * 0.55;
        }
    }
    gl = select(vec3<f32>(0.0), gl, gl == gl);
    U2[2u * i] = vec4<f32>(b, op);
    U2[2u * i + 1u] = vec4<f32>(min(gl, vec3<f32>(60.0)), tcell);
}

// ---------------- bloom ----------------

// Gaussian, sigma 2.2 cells (tight bloom: the halo right around flames)
var<private> TW: array<f32, 7> = array<f32, 7>(0.1810, 0.1640, 0.1220, 0.0745, 0.0373, 0.0153, 0.0052);

// Bloom is occluded by solid walls: marching outward from the pixel, a wall cell still contributes its own glow
// but blocks everything behind it (light inside a closed steel chamber does not halo through the steel).
fn vis_open(j: u32) -> f32 { return select(1.0, 0.0, tile_of(j) == 1u); }

@compute @workgroup_size(16, 16)
fn blurh(@builtin(global_invocation_id) g: vec3<u32>) {
    if (g.x >= P.w || g.y >= P.h) { return; }
    var a = U2[2u * (g.y * P.w + g.x) + 1u].rgb * TW[0];
    var tp = 1.0;
    var tm = 1.0;
    for (var k = 1; k <= 6; k++) {
        let jp = cid(i32(g.x) + k, i32(g.y));
        let jm = cid(i32(g.x) - k, i32(g.y));
        a += (U2[2u * jp + 1u].rgb * tp + U2[2u * jm + 1u].rgb * tm) * TW[k];
        tp *= vis_open(jp);
        tm *= vis_open(jm);
    }
    U1[2u * (g.y * P.w + g.x)] = vec4<f32>(a, 1.0);
}

@compute @workgroup_size(16, 16)
fn blurv(@builtin(global_invocation_id) g: vec3<u32>) {
    if (g.x >= P.w || g.y >= P.h) { return; }
    var a = U1[2u * (g.y * P.w + g.x)].rgb * TW[0];
    var tp = 1.0;
    var tm = 1.0;
    for (var k = 1; k <= 6; k++) {
        let jp = cid(i32(g.x), i32(g.y) + k);
        let jm = cid(i32(g.x), i32(g.y) - k);
        a += (U1[2u * jp].rgb * tp + U1[2u * jm].rgb * tm) * TW[k];
        tp *= vis_open(jp);
        tm *= vis_open(jm);
    }
    U1[2u * (g.y * P.w + g.x) + 1u] = vec4<f32>(a, 1.0);
}

fn qdim() -> vec2<u32> { return vec2<u32>((P.w + 3u) / 4u, (P.h + 3u) / 4u); }

// 4x4 box downsample of the glow to quarter resolution
@compute @workgroup_size(16, 16)
fn bdown(@builtin(global_invocation_id) g: vec3<u32>) {
    let q = qdim();
    if (g.x >= q.x || g.y >= q.y) { return; }
    var a = vec3<f32>(0.0);
    var op = 0.0;
    for (var y = 0; y < 4; y++) {
        for (var x = 0; x < 4; x++) {
            let j = cid(i32(g.x * 4u) + x, i32(g.y * 4u) + y);
            a += U2[2u * j + 1u].rgb;
            op += vis_open(j);
        }
    }
    // a = transmission of this quarter-res block (a block that is half wall or more blocks the wide glow)
    U1[2u * (g.y * q.x + g.x)] = vec4<f32>(a / 16.0, clamp(op / 16.0 * 2.0 - 1.0, 0.0, 1.0));
}

// Gaussian, sigma 4.5 quarter-res cells (~18 cells: wide light spill)
var<private> WW: array<f32, 13> = array<f32, 13>(0.0887, 0.0866, 0.0806, 0.0715, 0.0605, 0.0488, 0.0375, 0.0275, 0.0192, 0.0128, 0.0081, 0.0049, 0.0028);

fn qid(x: i32, y: i32) -> u32 {
    let q = qdim();
    return u32(clamp(y, 0, i32(q.y) - 1)) * q.x + u32(clamp(x, 0, i32(q.x) - 1));
}

@compute @workgroup_size(16, 16)
fn bwideh(@builtin(global_invocation_id) g: vec3<u32>) {
    let q = qdim();
    if (g.x >= q.x || g.y >= q.y) { return; }
    let x = i32(g.x);
    let y = i32(g.y);
    let c0 = U1[2u * qid(x, y)];
    var a = c0.rgb * WW[0];
    var tp = 1.0;
    var tm = 1.0;
    for (var k = 1; k <= 12; k++) {
        let p = U1[2u * qid(x + k, y)];
        let m = U1[2u * qid(x - k, y)];
        a += (p.rgb * tp + m.rgb * tm) * WW[k];
        tp *= p.a;
        tm *= m.a;
    }
    let nq = q.x * q.y;
    U1[2u * (nq + g.y * q.x + g.x)] = vec4<f32>(a, c0.a);
}

@compute @workgroup_size(16, 16)
fn bwidev(@builtin(global_invocation_id) g: vec3<u32>) {
    let q = qdim();
    if (g.x >= q.x || g.y >= q.y) { return; }
    let nq = q.x * q.y;
    let x = i32(g.x);
    let y = i32(g.y);
    var a = U1[2u * (nq + qid(x, y))].rgb * WW[0];
    var tp = 1.0;
    var tm = 1.0;
    for (var k = 1; k <= 12; k++) {
        let p = U1[2u * (nq + qid(x, y + k))];
        let m = U1[2u * (nq + qid(x, y - k))];
        a += (p.rgb * tp + m.rgb * tm) * WW[k];
        tp *= p.a;
        tm *= m.a;
    }
    U1[2u * (2u * nq + g.y * q.x + g.x)] = vec4<f32>(a, 1.0);
}
