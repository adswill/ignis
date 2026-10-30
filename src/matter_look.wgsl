// Per-element material looks (chem team). Appended after vis.wgsl in the sim/matter/vis module (same bindings).
// matter_look(e, x, y) returns the display-space albedo of a condensed-matter cell. Every element has a style
// (LK_STYLE), a secondary colour (LK_C2) and parameters (LK_PAR = scale, contrast, sheen, variant); the primary
// colour is E_COL. Patterns are built from the cell coordinate only, so they stay crisp pixels at 1 cell and read as
// grains / crystals / flakes / ribbons when zoomed in. State overlays: cast (sintered smooth grain with layering),
// wet (darker, glossy), charred (burning fuels blacken as they are consumed).
//
// (new: 21 needle, 22 pressed block, 23 spangle; style 9 liquids use par.w variants 0 thin, 1 viscous, 2 fizz, 3 fume, 4 gel, 5 molten)
// styles: 0 fine dust, 1 granule, 2 crystal, 3 prill, 4 flake, 5 turning/ribbon, 6 chunk, 7 fibre, 8 smooth/waxy,
// 9 liquid, 10 coal facets, 11 woody, 12 brushed metal, 13 rock, 14 mixed grains, 15 ice, 16 paper wrap,
// 17 glass, 18 cord, 19 brick, 20 aerogel

fn ml_h(x: u32, y: u32, s: u32) -> f32 { return mhash(x, y, s + 1000u); }
fn ml_n(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = p - i;
    let u = f * f * (3.0 - 2.0 * f);
    let x = u32(i32(i.x) + 4096);
    let y = u32(i32(i.y) + 4096);
    let a = ml_h(x, y, 71u);
    let b = ml_h(x + 1u, y, 71u);
    let c = ml_h(x, y + 1u, 71u);
    let d = ml_h(x + 1u, y + 1u, 71u);
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

fn ml_fine(c1: vec3<f32>, c2: vec3<f32>, par: vec4<f32>, x: u32, y: u32) -> vec3<f32> {
    let p = vec2<f32>(f32(x), f32(y));
    let n = ml_n(p * 0.45);
    let h = ml_h(x, y, 3u);
    var c = mix(c1, c2, smoothstep(0.35, 0.85, n) * par.y * 1.6);
    c *= 1.0 + (h - 0.5) * par.y * 1.4;
    let g = ml_h(x, y, 17u);
    if (g > 1.0 - 0.07 * par.z) { c = mix(c, vec3<f32>(1.0), 0.55 * min(par.z, 1.0)); }
    return c;
}

fn ml_granule(c1: vec3<f32>, c2: vec3<f32>, par: vec4<f32>, x: u32, y: u32, sz: u32) -> vec3<f32> {
    let by = y / sz;
    let xo = x + u32(ml_h(0u, by, 61u) * f32(sz * 3u));
    let bx = xo / sz;
    let h = ml_h(bx, by, 3u);
    let h2 = ml_h(bx, by, 11u);
    var c = mix(c1, c2, smoothstep(0.3, 1.0, h) * min(par.y * 1.7, 1.0));
    var shade = 1.0 + (h2 - 0.5) * par.y * 1.3;
    if (sz >= 2u) {
        let s = f32(sz);
        let q = vec2<f32>((f32(xo % sz) + 0.5) / s - 0.5, (f32(y % sz) + 0.5) / s - 0.5);
        shade *= 1.0 - 0.34 * smoothstep(0.5, 1.0, length(q) * 2.0);
        let hl = length(q - vec2<f32>(-0.17, -0.2)) * 3.2;
        c += vec3<f32>(par.z * 0.22 * (1.0 - smoothstep(0.0, 1.0, hl)) * select(0.35, 1.0, h2 > 0.5));
    } else if (ml_h(x, y, 29u) > 1.0 - 0.1 * par.z) {
        c = mix(c, vec3<f32>(1.0), 0.4 * min(par.z, 1.0));
    }
    return c * shade;
}

fn ml_crystal(c1: vec3<f32>, c2: vec3<f32>, par: vec4<f32>, x: u32, y: u32) -> vec3<f32> {
    let h = ml_h(x, y, 5u);
    let h2 = ml_h(x, y, 19u);
    let h3 = ml_h(x / 2u, y / 2u, 23u);
    var c = mix(c1, c2, h3);
    c *= 1.0 - par.y * 0.85 * (1.0 - h) * (1.0 - h);
    if (h2 > 1.0 - 0.1 * par.z) { c = mix(c, vec3<f32>(1.0), 0.85); }
    if (h2 < 0.05) { c *= 0.6; }
    if (par.w > 0.5 && ((x ^ y) & 1u) == 0u && h > 0.5) { c *= 1.07; } // cubic habit
    return c;
}

fn ml_prill(c1: vec3<f32>, c2: vec3<f32>, par: vec4<f32>, x: u32, y: u32) -> vec3<f32> {
    let sz = max(u32(par.x), 2u);
    let fs = f32(sz);
    let by = y / sz;
    let xo = x + u32(ml_h(0u, by, 61u) * fs * 3.0);   // row offsets are random, not a regular lattice
    let bx = xo / sz;
    let q = vec2<f32>((f32(xo % sz) + 0.5) / fs - 0.5, (f32(y % sz) + 0.5) / fs - 0.5) * 2.0;
    let r2 = dot(q, q);
    let h = ml_h(bx, by, 3u);
    var c = mix(c1, c2, h * par.y);
    c *= 0.72 + 0.4 * (1.0 - r2) + (ml_h(bx, by, 7u) - 0.5) * 0.12;
    c += vec3<f32>(par.z * 0.55 * exp(-9.0 * dot(q - vec2<f32>(-0.4, -0.45), q - vec2<f32>(-0.4, -0.45))));
    if (r2 > 1.15) { c *= 0.5; }
    return c;
}

fn ml_flake(c1: vec3<f32>, c2: vec3<f32>, par: vec4<f32>, x: u32, y: u32) -> vec3<f32> {
    let xo = x + (y * 5u) % 3u;
    let bx = xo / 3u;
    let lx = xo % 3u;
    let h = ml_h(bx, y, 3u);
    var c = mix(c1, c2, h * h * min(par.y * 2.0, 1.0));
    var shade = 0.84 + 0.3 * ml_h(bx, y, 7u);
    if (lx == 0u) { shade *= 0.82; }
    if (lx == 2u && ml_h(bx, y, 13u) > 0.5) { shade *= 1.1; }
    if (ml_h(x, y, 21u) > 1.0 - 0.07 * par.z) { c = mix(c, vec3<f32>(1.0), 0.5); }
    if (par.w > 0.5) { shade *= 0.75 + 0.5 * f32((x + 2u * y) % 4u == 0u); } // graphite: layered plates
    return c * shade;
}

fn ml_turning(c1: vec3<f32>, c2: vec3<f32>, par: vec4<f32>, x: u32, y: u32) -> vec3<f32> {
    let rx = x / 5u;
    let ry = y / 5u;
    let u = f32(x) + f32(y) * (0.5 + 1.0 * ml_h(rx, ry, 3u));
    let band = 0.5 + 0.5 * sin(u * 1.9 + 6.0 * ml_h(rx, ry, 9u));
    var c = mix(c1 * 0.65, c2, band);
    let g = ml_h(x, y, 21u);
    if (g > 1.0 - 0.08 * par.z) { c = vec3<f32>(1.0); }
    if (g < 0.07) { c *= 0.42; }
    return c * (0.9 + 0.2 * ml_h(x, y, 4u));
}

fn ml_chunk(c1: vec3<f32>, c2: vec3<f32>, par: vec4<f32>, x: u32, y: u32, sz: u32) -> vec3<f32> {
    let by = y / sz;
    let xo = x + select(0u, sz / 2u, (by & 1u) == 1u);
    let bx = xo / sz;
    let lx = xo % sz;
    let ly = y % sz;
    let h = ml_h(bx, by, 3u);
    var c = mix(c1, c2, h * min(par.y * 2.0, 1.0));
    var shade = 0.84 + 0.3 * ml_h(bx, by, 8u);
    if (lx == 0u || ly == 0u) { shade *= 0.74; }
    shade *= select(0.95, 1.08, lx + ly >= sz);
    if (ml_h(x, y, 21u) > 1.0 - 0.06 * par.z) { c = mix(c, vec3<f32>(1.0), 0.35); }
    return c * shade;
}

fn ml_fibre(c1: vec3<f32>, c2: vec3<f32>, par: vec4<f32>, x: u32, y: u32) -> vec3<f32> {
    let v = u32(par.w);
    let h = ml_h(x / 2u, y, 7u);
    var c = mix(c1, c2, h * 0.7);
    var shade = 0.9 + 0.2 * ml_h(0u, y, 5u);
    if (v == 0u) {
        if (ml_h(x / 3u, y, 31u) > 0.85) { shade *= 1.1; }                 // paper: horizontal fibre streaks
    } else if (v == 1u || v == 2u) {
        shade = 0.9 + 0.2 * ml_h(x, y, 3u);                              // fluffy fibres: speckled, soft
        if (ml_h(x, y, 33u) > 0.9) { shade *= 1.12; }
    } else if (v == 3u) {
        shade *= select(0.86, 1.1, ((x / 2u + y / 2u) & 1u) == 0u);     // woven cloth / laminate
        if (x % 2u == 0u) { shade *= 0.94; }
    }
    return c * shade;
}

fn ml_smooth(c1: vec3<f32>, c2: vec3<f32>, par: vec4<f32>, x: u32, y: u32) -> vec3<f32> {
    let p = vec2<f32>(f32(x), f32(y));
    let v = u32(par.w);
    let n = ml_n(p * 0.14);
    var c = mix(c1, c2, n * 0.45);
    var shade = 1.0 + (n - 0.5) * par.y * 3.0;
    if (v == 1u) {
        // plastic explosive putty: kneaded folds and thumbprint mottle
        let k = ml_n(p * 0.32);
        if (abs(k - 0.5) < 0.035) { shade *= 0.7; }
        shade *= 0.92 + 0.16 * ml_n(p * 0.8);
        c = mix(c, c2, 0.4 * ml_n(p * 0.12));
    } else if (v == 2u) {
        // soft alkali metal: knife-cut facets, dull tarnish patches
        let t = ml_n(p * 0.2);
        c = mix(c, c1 * vec3<f32>(0.78, 0.74, 0.66), smoothstep(0.55, 0.8, t) * 0.7);
        if (y % 5u == 1u) { shade *= 1.12; }
        if (ml_h(x, y, 3u) > 0.93) { c = mix(c, vec3<f32>(1.0), 0.45 * par.z); }
    } else if (v == 3u) {
        // wax: very smooth, broad soft highlights, faint translucent depth
        shade = 0.97 + 0.06 * n;
        let d = fract((p.x - p.y * 0.5) / 9.0);
        if (d < 0.22) { c = mix(c, vec3<f32>(1.0), 0.30 * par.z); }
    } else {
        if (y % 6u == 0u) { shade *= 0.97; }
        let d = fract((p.x - p.y * 0.5) / 11.0);
        if (d < 0.1) { c = mix(c, vec3<f32>(1.0), 0.28 * min(par.z, 1.0)); }
    }
    return c * shade;
}

fn ml_coal(c1: vec3<f32>, c2: vec3<f32>, par: vec4<f32>, x: u32, y: u32) -> vec3<f32> {
    let by = y / 3u;
    let xo = x + u32(ml_h(0u, by, 61u) * 9.0);
    let bx = xo / 3u;
    let h = ml_h(bx, by, 3u);
    let fac = ml_h(bx, by, 9u);
    var c = c1 * (0.7 + 0.6 * h);
    if (fac > 0.72) { c = mix(c, c2, (fac - 0.72) / 0.28 * min(par.z, 1.0)); }   // bright conchoidal facet
    if (xo % 3u == 0u || y % 3u == 0u) { c *= 0.65; }
    if (ml_h(x, y, 27u) > 0.965) { c = vec3<f32>(0.75, 0.77, 0.82); }          // glassy glint
    return c;
}

fn ml_woody(c1: vec3<f32>, c2: vec3<f32>, par: vec4<f32>, x: u32, y: u32) -> vec3<f32> {
    let v = par.w;
    let sh = ml_h(x, y / 4u, 3u);
    let bx = x / 3u;
    let by = y / 3u;
    var c = mix(c1, c2, sh * min(par.y * 2.0, 1.0));
    var shade = 0.82 + 0.3 * ml_h(bx, by, 8u);
    if (v < 0.5) {
        if (x % 3u == 0u && ml_h(bx, by, 5u) > 0.4) { shade *= 0.7; }
        if (ml_h(x, y, 21u) > 0.96) { c = vec3<f32>(0.34, 0.33, 0.31); }       // ash-grey flecks
    } else {
        shade = 0.88 + 0.24 * ml_h(x, y, 3u);                                  // sawdust: fine, pale
        if (ml_h(x, y, 21u) > 0.9) { c = c1 * 1.12; }
    }
    return c * shade;
}

fn ml_metal(c1: vec3<f32>, c2: vec3<f32>, par: vec4<f32>, x: u32, y: u32) -> vec3<f32> {
    var c = mix(c1, c2, ml_h(0u, y, 3u) * 0.5);
    c *= 0.88 + 0.22 * ml_h(x / 4u, y, 5u);
    if (y % 7u == 2u) { c = mix(c, vec3<f32>(1.0), 0.22 * par.z); }
    return c;
}

fn ml_rock(c1: vec3<f32>, c2: vec3<f32>, par: vec4<f32>, x: u32, y: u32) -> vec3<f32> {
    var c = ml_chunk(c1, c2, par, x, y, 5u);
    let sp = ml_h(x, y, 5u);
    if (sp < 0.1) { c = c2 * 0.7; }
    if (sp > 0.92) { c = c1 * 1.3; }
    return c;
}

fn ml_mixed(c1: vec3<f32>, c2: vec3<f32>, par: vec4<f32>, x: u32, y: u32) -> vec3<f32> {
    let q = ml_h(x, y, 3u);
    let q2 = ml_h(x, y, 8u);
    if (q < 0.12) { return c2 * (0.8 + 0.4 * q2); }
    if (q < 0.2) { return c1 * 0.78; }
    if (q > 0.975) { return vec3<f32>(0.98, 0.97, 0.93); }
    if (q > 0.92) { return vec3<f32>(0.9, 0.86, 0.74); }
    return c1 * (0.84 + 0.3 * q2);
}

fn ml_ice(c1: vec3<f32>, c2: vec3<f32>, par: vec4<f32>, x: u32, y: u32) -> vec3<f32> {
    let p = vec2<f32>(f32(x), f32(y));
    let n = ml_n(p * 0.22);
    var c = mix(c1, c2, n * 0.5) * (0.92 + 0.1 * ml_h(x, y, 3u));
    if (abs(n - 0.5) < 0.03) { c = mix(c, vec3<f32>(1.0), 0.65); }            // fracture planes
    if (ml_h(x, y, 13u) > 0.975) { c = vec3<f32>(1.0); }                      // trapped bubbles / frost
    if (par.w > 0.5) { c = mix(c, vec3<f32>(1.0), 0.18 * ml_n(p * 0.7)); }    // dry ice: frosted
    return c;
}

fn ml_wrap(c1: vec3<f32>, c2: vec3<f32>, par: vec4<f32>, x: u32, y: u32) -> vec3<f32> {
    var c = mix(c1, c2, 0.3 * ml_h(x / 2u, y, 3u));
    var shade = 0.92 + 0.14 * ml_h(0u, y, 5u);
    if (x % 8u == 0u) { shade *= 0.7; }                                       // glued seam / printed band
    if (y % 12u < 2u) { c = mix(c, vec3<f32>(0.9, 0.85, 0.7), 0.5); }          // label strip
    return c * shade;
}

fn ml_glass(c1: vec3<f32>, c2: vec3<f32>, par: vec4<f32>, x: u32, y: u32) -> vec3<f32> {
    var c = mix(c1, c2, 0.3) * (0.9 + 0.08 * ml_n(vec2<f32>(f32(x), f32(y)) * 0.3));
    if ((x + y) % 13u < 2u) { c = mix(c, vec3<f32>(1.0), 0.45); }              // diagonal glints
    if ((x + 2u * y) % 29u == 0u) { c *= 0.8; }
    return c;
}

fn ml_cord(c1: vec3<f32>, c2: vec3<f32>, par: vec4<f32>, x: u32, y: u32) -> vec3<f32> {
    var c = select(c2, c1, (x + y) % 4u < 2u);
    c *= 0.9 + 0.2 * ml_h(x, y, 3u);
    return c;
}

fn ml_brick(c1: vec3<f32>, c2: vec3<f32>, par: vec4<f32>, x: u32, y: u32) -> vec3<f32> {
    let row = y / 4u;
    let xo = x + select(0u, 4u, (row & 1u) == 1u);
    let bx = xo / 8u;
    var c = mix(c1, c2, ml_h(bx, row, 3u) * 0.8) * (0.9 + 0.2 * ml_h(x, y, 5u));
    if (xo % 8u == 0u || y % 4u == 0u) { c = vec3<f32>(0.66, 0.62, 0.55) * (0.9 + 0.15 * ml_h(x, y, 9u)); }
    return c;
}

fn ml_aero(c1: vec3<f32>, c2: vec3<f32>, par: vec4<f32>, x: u32, y: u32) -> vec3<f32> {
    let n = ml_n(vec2<f32>(f32(x), f32(y)) * 0.18);
    return mix(c1, c2, n) * (0.95 + 0.06 * ml_h(x, y, 3u));
}

fn ml_pattern(style: u32, c1: vec3<f32>, c2: vec3<f32>, par: vec4<f32>, x: u32, y: u32) -> vec3<f32> {
    switch (style) {
        case 0u: { return ml_fine(c1, c2, par, x, y); }
        case 1u: { return ml_granule(c1, c2, par, x, y, select(2u, 1u, par.x < 1.5)); }
        case 2u: { return ml_crystal(c1, c2, par, x, y); }
        case 3u: { return ml_prill(c1, c2, par, x, y); }
        case 4u: { return ml_flake(c1, c2, par, x, y); }
        case 5u: { return ml_turning(c1, c2, par, x, y); }
        case 6u: { return ml_chunk(c1, c2, par, x, y, 4u); }
        case 7u: { return ml_fibre(c1, c2, par, x, y); }
        case 8u: { return ml_smooth(c1, c2, par, x, y); }
        case 10u: { return ml_coal(c1, c2, par, x, y); }
        case 11u: { return ml_woody(c1, c2, par, x, y); }
        case 12u: { return ml_metal(c1, c2, par, x, y); }
        case 13u: { return ml_rock(c1, c2, par, x, y); }
        case 14u: { return ml_mixed(c1, c2, par, x, y); }
        case 15u: { return ml_ice(c1, c2, par, x, y); }
        case 16u: { return ml_wrap(c1, c2, par, x, y); }
        case 17u: { return ml_glass(c1, c2, par, x, y); }
        case 18u: { return ml_cord(c1, c2, par, x, y); }
        case 19u: { return ml_brick(c1, c2, par, x, y); }
        case 20u: { return ml_aero(c1, c2, par, x, y); }
        case 21u: { return ml_needle(c1, c2, par, x, y); }
        case 22u: { return ml_block(c1, c2, par, x, y); }
        case 23u: { return ml_spangle(c1, c2, par, x, y); }
        default: { return c1; }
    }
}

fn ml_needle(c1: vec3<f32>, c2: vec3<f32>, par: vec4<f32>, x: u32, y: u32) -> vec3<f32> {
    // needle crystals / filings: thin elongated slivers; orientation changes per 6x6 patch
    let fx = f32(x);
    let fy = f32(y);
    let o = ml_h(x / 6u, y / 6u, 51u);
    var dir = normalize(vec2<f32>(1.0, 0.25 + 1.3 * o));
    if (par.w > 0.5) {
        let a = o * 3.14159;
        dir = vec2<f32>(cos(a), sin(a));
    }
    let u = fx * dir.y - fy * dir.x;
    let v = fx * dir.x + fy * dir.y;
    let bu = floor(u / 1.3);
    let ub = u32(i32(bu) + 4096);
    let bv = floor((v + 9.0 * ml_h(ub, 0u, 53u)) / 6.0);
    let nid = ml_h(ub, u32(i32(bv) + 4096), 55u);
    var c = mix(c1, c2, nid * nid);
    var shade = 0.72 + 0.45 * nid;
    if (fract(u / 1.3) < 0.28) { shade *= 0.78; }
    c *= shade;
    if (ml_h(x, y, 57u) > 1.0 - 0.09 * par.z) { c = mix(c, vec3<f32>(1.0), 0.75); }
    return c;
}

fn ml_block(c1: vec3<f32>, c2: vec3<f32>, par: vec4<f32>, x: u32, y: u32) -> vec3<f32> {
    // pressed / cast block with chamfered seams and a stamped mark (TNT)
    let bx = x / 12u;
    let by = y / 7u;
    let lx = x % 12u;
    let ly = y % 7u;
    let h = ml_h(bx, by, 3u);
    var c = c1 * (0.9 + 0.16 * h) * (0.96 + 0.07 * ml_h(x, y, 5u));
    if (lx == 0u || ly == 0u) { c = c2 * 0.85; }
    else if (lx == 1u || ly == 1u) { c *= 1.12; }
    else if (lx == 11u || ly == 6u) { c *= 0.8; }
    if (lx >= 4u && lx <= 6u && ly >= 2u && ly <= 4u && h > 0.35) {
        let bit = (u32(h * 977.0) >> ((ly - 2u) * 3u + (lx - 4u))) & 1u;
        if (bit == 1u || (lx == 5u && ly == 3u)) { c = c2; }
    }
    return c;
}

fn ml_spangle(c1: vec3<f32>, c2: vec3<f32>, par: vec4<f32>, x: u32, y: u32) -> vec3<f32> {
    // crystalline spangle (zinc): large Voronoi crystallites with individual brightness
    let p = vec2<f32>(f32(x), f32(y)) / 5.0;
    let ip = floor(p);
    var best = 9.0;
    var second = 9.0;
    var bid = 0.0;
    for (var j = -1; j <= 1; j++) {
        for (var i = -1; i <= 1; i++) {
            let g = ip + vec2<f32>(f32(i), f32(j));
            let ux = u32(i32(g.x) + 4096);
            let uy = u32(i32(g.y) + 4096);
            let pt = g + vec2<f32>(ml_h(ux, uy, 81u), ml_h(ux, uy, 83u));
            let dd = length(pt - p);
            if (dd < best) { second = best; best = dd; bid = ml_h(ux, uy, 85u); }
            else if (dd < second) { second = dd; }
        }
    }
    var c = mix(c1, c2, bid) * (0.72 + 0.4 * bid);
    if (second - best < 0.09) { c *= 0.55; }
    if (ml_h(x, y, 87u) > 1.0 - 0.05 * par.z) { c = mix(c, vec3<f32>(1.0), 0.6); }
    return c;
}

fn ml_liquid(c1: vec3<f32>, c2: vec3<f32>, par: vec4<f32>, x: u32, y: u32, e: u32) -> vec3<f32> {
    let p = vec2<f32>(f32(x), f32(y));
    let v = u32(par.w);
    var c = c1 * (1.0 + (ml_n(p * vec2<f32>(0.07, 0.12)) - 0.5) * 0.10);
    // refraction streaks (thin clear liquids) -- soft diagonal lighter bands
    let band = fract((p.x * 0.9 + p.y * 0.45) / 13.0);
    c = mix(c, c2, 0.22 * par.z * smoothstep(0.0, 0.12, band) * (1.0 - smoothstep(0.12, 0.26, band)));
    if (v == 1u) {            // viscous: slow, wide gloss swirls
        c = mix(c, c2, 0.28 * ml_n(p * vec2<f32>(0.04, 0.2)));
    } else if (v == 2u) {     // fizzing (peroxide): bubbles
        if (ml_h(x, y, 5u) > 0.96) { c = mix(c, vec3<f32>(1.0), 0.75); }
    } else if (v == 3u) {     // fuming / frosting: mist patches and specks
        c = mix(c, vec3<f32>(1.0), 0.26 * ml_n(p * 0.12 + vec2<f32>(3.1, 7.7)));
        if (ml_h(x, y, 9u) > 0.975) { c = mix(c, vec3<f32>(1.0), 0.6); }
    } else if (v == 4u) {     // gel with trapped bubbles
        let bb = ml_h(x / 3u, y / 3u, 11u);
        if (bb > 0.86) {
            let q = vec2<f32>(f32(x % 3u) - 1.0, f32(y % 3u) - 1.0);
            c = mix(c, c2, select(0.0, 0.8, dot(q, q) < 1.5)) * select(0.85, 1.0, dot(q, q) < 1.5);
        }
    } else if (v == 5u) {     // molten: mottled hot surface, lava gets dark crust
        let t = ml_n(p * 0.17);
        c = mix(c1, c2, t);
        if (e == 55u && t < 0.34) { c = vec3<f32>(0.24, 0.05, 0.02) * (0.7 + 0.6 * ml_h(x, y, 3u)); }
    }
    return c;
}

fn matter_look(e_in: u32, x: u32, y: u32) -> vec3<f32> {
    let e = min(e_in, N_ELEM - 1u);
    let c1 = LK_C1[e];
    let c2 = LK_C2[e];
    let par = LK_PAR[e];
    let style = LK_STYLE[e];
    let i = y * P.w + x;
    var col = vec3<f32>(0.0);
    if (style == 9u) {
        col = ml_liquid(c1, c2, par, x, y, e);
    } else {
        // per-element phase shift of the cell coordinate so equal styles never line up
        col = ml_pattern(style, c1, c2, par, x + e * 53u, y + e * 29u);
    }
    let p = vec2<f32>(f32(x), f32(y));
    // cast / pressed grain: sintered, smooth, with faint pour layers and a satin gloss
    if (style != 9u && (META[i] & CAST_BIT) != 0u) {
        var sm = mix(c1, c2, 0.12) * (0.93 + 0.14 * ml_n(p * 0.16));
        if (y % 5u == 0u) { sm *= 0.94; }
        if (fract((p.x - p.y * 0.6) / 17.0) < 0.07) { sm = mix(sm, vec3<f32>(1.0), 0.12); }
        col = mix(col, sm * vec3<f32>(1.05, 0.97, 0.88), 0.78);
    }
    // wet: damp powders go darker and glossy
    if (x > 0u && y > 0u && x + 1u < P.w && y + 1u < P.h) {
        var wet = false;
        let nb = array<u32, 4>(i - 1u, i + 1u, i - P.w, i + P.w);
        for (var k = 0u; k < 4u; k++) {
            if (tile_of(nb[k]) == MATTER) {
                let ne = elem_of(nb[k]);
                if (ne == 19u || ne == 29u) { wet = true; }
            }
        }
        if (wet && style != 9u) {
            col = col * vec3<f32>(0.72, 0.75, 0.8) + vec3<f32>(0.04 * f32(ml_h(x, y, 41u) > 0.88));
        }
        // pile / depth cue: lit top edge, darker toward the bottom, shaded flanks (powders), meniscus (liquids)
        var d = 0u;
        for (var k = 1u; k <= 8u; k++) {
            if (y < k) { break; }
            if (tile_of(i - k * P.w) == MATTER) { d = k; } else { break; }
        }
        if (style == 9u) {
            if (d == 0u) { col = mix(col, vec3<f32>(1.0), 0.38); }
            else if (d == 1u) { col *= 1.10; }
            col *= 1.0 - 0.028 * f32(min(d, 8u));
        } else {
            if (d == 0u) { col *= 1.22; } else if (d == 1u) { col *= 1.07; }
            col *= 1.0 - 0.022 * f32(min(d, 8u));
            let tl = tile_of(i - 1u);
            let tr = tile_of(i + 1u);
            if (tl == 0u || tr == 0u) { col *= 0.86; }
        }
    }
    // charred: burning fuels blacken as they are consumed
    let rem = WALL[i].w;
    if (rem < 0.999 && E_TIGN_AIR[e] < 90.0) {
        let ch = smoothstep(0.0, 0.75, 1.0 - rem);
        let ash = vec3<f32>(0.07, 0.065, 0.06) + vec3<f32>(0.12) * f32(ml_h(x, y, 43u) > 0.8) * ch;
        col = mix(col, ash, ch);
    }
    return col;
}

// ML-GEN-BEGIN (generated by scratchpad v2/mat/spec.py)
const LK_STYLE: array<u32, 110> = array<u32, 110>(0u, 0u, 0u, 0u, 0u, 0u, 0u, 0u, 0u, 0u, 14u, 11u, 6u, 21u, 2u, 0u, 3u, 1u, 3u, 9u, 9u, 9u, 9u, 9u, 9u, 0u, 15u, 0u, 2u, 9u, 9u, 9u, 9u, 9u, 9u, 8u, 9u, 10u, 7u, 5u, 21u, 8u, 8u, 9u, 9u, 9u, 0u, 4u, 7u, 22u, 8u, 16u, 12u, 17u, 9u, 9u, 13u, 1u, 15u, 0u, 2u, 9u, 8u, 9u, 0u, 18u, 9u, 9u, 1u, 4u, 1u, 5u, 23u, 0u, 1u, 0u, 8u, 7u, 0u, 11u, 9u, 9u, 9u, 9u, 9u, 12u, 4u, 7u, 11u, 7u, 19u, 20u, 9u, 9u, 9u, 9u, 13u, 0u, 0u, 6u, 13u, 1u, 6u, 0u, 0u, 12u, 12u, 9u, 9u, 9u);
const LK_C1: array<vec3<f32>, 110> = array<vec3<f32>, 110>(
    vec3<f32>(0.50, 0.50, 0.50),
    vec3<f32>(0.50, 0.50, 0.50),
    vec3<f32>(0.50, 0.50, 0.50),
    vec3<f32>(0.50, 0.50, 0.50),
    vec3<f32>(0.50, 0.50, 0.50),
    vec3<f32>(0.50, 0.50, 0.50),
    vec3<f32>(0.50, 0.50, 0.50),
    vec3<f32>(0.50, 0.50, 0.50),
    vec3<f32>(0.50, 0.50, 0.50),
    vec3<f32>(0.50, 0.50, 0.50),
    vec3<f32>(0.82, 0.70, 0.44),
    vec3<f32>(0.27, 0.19, 0.13),
    vec3<f32>(0.96, 0.86, 0.12),
    vec3<f32>(0.76, 0.85, 0.99),
    vec3<f32>(0.98, 0.92, 0.76),
    vec3<f32>(0.55, 0.62, 0.74),
    vec3<f32>(0.76, 0.78, 0.96),
    vec3<f32>(0.68, 0.28, 0.10),
    vec3<f32>(0.97, 0.88, 0.84),
    vec3<f32>(0.16, 0.50, 0.98),
    vec3<f32>(0.92, 0.56, 0.14),
    vec3<f32>(0.50, 0.88, 0.92),
    vec3<f32>(0.42, 0.66, 1.00),
    vec3<f32>(0.80, 0.90, 0.36),
    vec3<f32>(1.00, 0.48, 0.08),
    vec3<f32>(0.62, 0.62, 0.60),
    vec3<f32>(0.60, 0.85, 0.99),
    vec3<f32>(0.86, 0.93, 1.00),
    vec3<f32>(0.94, 0.95, 0.97),
    vec3<f32>(0.22, 0.70, 0.62),
    vec3<f32>(0.80, 0.90, 0.96),
    vec3<f32>(0.97, 0.92, 0.52),
    vec3<f32>(0.56, 0.46, 0.12),
    vec3<f32>(0.92, 0.66, 0.06),
    vec3<f32>(0.84, 0.34, 0.08),
    vec3<f32>(0.96, 0.90, 0.66),
    vec3<f32>(0.99, 0.94, 0.76),
    vec3<f32>(0.05, 0.05, 0.06),
    vec3<f32>(0.93, 0.86, 0.72),
    vec3<f32>(0.80, 0.83, 0.86),
    vec3<f32>(0.26, 0.22, 0.20),
    vec3<f32>(0.86, 0.83, 0.66),
    vec3<f32>(0.64, 0.64, 0.84),
    vec3<f32>(0.72, 0.88, 0.68),
    vec3<f32>(0.60, 0.92, 0.26),
    vec3<f32>(0.62, 0.80, 1.00),
    vec3<f32>(0.24, 0.17, 0.30),
    vec3<f32>(0.82, 0.95, 0.88),
    vec3<f32>(0.92, 0.88, 0.64),
    vec3<f32>(0.88, 0.74, 0.40),
    vec3<f32>(0.78, 0.78, 0.68),
    vec3<f32>(0.80, 0.20, 0.14),
    vec3<f32>(0.80, 0.50, 0.28),
    vec3<f32>(0.70, 0.88, 0.90),
    vec3<f32>(1.00, 0.60, 0.20),
    vec3<f32>(0.95, 0.26, 0.05),
    vec3<f32>(0.42, 0.42, 0.45),
    vec3<f32>(0.22, 0.24, 0.30),
    vec3<f32>(0.84, 0.92, 1.00),
    vec3<f32>(0.08, 0.17, 0.30),
    vec3<f32>(0.36, 0.05, 0.42),
    vec3<f32>(0.90, 0.84, 0.72),
    vec3<f32>(0.12, 0.62, 0.56),
    vec3<f32>(0.24, 0.80, 0.66),
    vec3<f32>(0.95, 0.89, 0.76),
    vec3<f32>(0.56, 0.40, 0.24),
    vec3<f32>(0.56, 0.38, 0.30),
    vec3<f32>(0.86, 0.56, 0.28),
    vec3<f32>(0.08, 0.17, 0.11),
    vec3<f32>(0.97, 0.95, 0.68),
    vec3<f32>(0.80, 0.76, 0.66),
    vec3<f32>(0.30, 0.38, 0.52),
    vec3<f32>(0.52, 0.64, 0.70),
    vec3<f32>(0.56, 0.05, 0.16),
    vec3<f32>(0.97, 0.86, 0.90),
    vec3<f32>(0.99, 0.96, 0.68),
    vec3<f32>(0.13, 0.13, 0.14),
    vec3<f32>(0.96, 0.97, 1.00),
    vec3<f32>(0.24, 0.25, 0.31),
    vec3<f32>(0.86, 0.66, 0.38),
    vec3<f32>(0.56, 0.94, 0.76),
    vec3<f32>(0.74, 0.70, 0.96),
    vec3<f32>(0.98, 0.80, 0.28),
    vec3<f32>(0.92, 0.80, 0.86),
    vec3<f32>(0.62, 0.12, 0.06),
    vec3<f32>(0.34, 0.36, 0.44),
    vec3<f32>(0.34, 0.35, 0.40),
    vec3<f32>(0.54, 0.30, 0.14),
    vec3<f32>(0.24, 0.11, 0.07),
    vec3<f32>(0.80, 0.88, 0.58),
    vec3<f32>(0.88, 0.72, 0.42),
    vec3<f32>(0.58, 0.74, 0.97),
    vec3<f32>(0.80, 0.90, 1.00),
    vec3<f32>(0.70, 0.82, 0.95),
    vec3<f32>(0.90, 0.78, 0.42),
    vec3<f32>(0.84, 0.93, 0.93),
    vec3<f32>(0.84, 0.82, 0.74),
    vec3<f32>(0.92, 0.91, 0.86),
    vec3<f32>(0.96, 0.96, 0.98),
    vec3<f32>(0.93, 0.90, 0.82),
    vec3<f32>(0.58, 0.58, 0.60),
    vec3<f32>(0.43, 0.31, 0.20),
    vec3<f32>(0.97, 0.97, 0.97),
    vec3<f32>(0.28, 0.20, 0.16),
    vec3<f32>(0.62, 0.64, 0.68),
    vec3<f32>(0.16, 0.17, 0.19),
    vec3<f32>(0.82, 0.67, 0.26),
    vec3<f32>(0.12, 0.11, 0.10),
    vec3<f32>(0.45, 0.85, 0.45),
    vec3<f32>(0.62, 0.45, 0.14)
);
const LK_C2: array<vec3<f32>, 110> = array<vec3<f32>, 110>(
    vec3<f32>(0.50, 0.50, 0.50),
    vec3<f32>(0.50, 0.50, 0.50),
    vec3<f32>(0.50, 0.50, 0.50),
    vec3<f32>(0.50, 0.50, 0.50),
    vec3<f32>(0.50, 0.50, 0.50),
    vec3<f32>(0.50, 0.50, 0.50),
    vec3<f32>(0.50, 0.50, 0.50),
    vec3<f32>(0.50, 0.50, 0.50),
    vec3<f32>(0.50, 0.50, 0.50),
    vec3<f32>(0.50, 0.50, 0.50),
    vec3<f32>(0.58, 0.44, 0.26),
    vec3<f32>(0.42, 0.30, 0.20),
    vec3<f32>(0.80, 0.62, 0.06),
    vec3<f32>(1.00, 1.00, 1.00),
    vec3<f32>(1.00, 0.98, 0.88),
    vec3<f32>(0.78, 0.84, 0.93),
    vec3<f32>(0.92, 0.94, 1.00),
    vec3<f32>(0.44, 0.14, 0.05),
    vec3<f32>(0.88, 0.74, 0.68),
    vec3<f32>(0.50, 0.80, 1.00),
    vec3<f32>(1.00, 0.78, 0.40),
    vec3<f32>(0.82, 1.00, 1.00),
    vec3<f32>(0.80, 0.92, 1.00),
    vec3<f32>(0.95, 1.00, 0.60),
    vec3<f32>(1.00, 0.85, 0.40),
    vec3<f32>(0.34, 0.34, 0.34),
    vec3<f32>(0.90, 0.99, 1.00),
    vec3<f32>(0.66, 0.80, 0.95),
    vec3<f32>(0.78, 0.82, 0.88),
    vec3<f32>(0.50, 0.88, 0.80),
    vec3<f32>(0.97, 1.00, 1.00),
    vec3<f32>(1.00, 1.00, 0.78),
    vec3<f32>(0.74, 0.62, 0.22),
    vec3<f32>(1.00, 0.86, 0.34),
    vec3<f32>(0.96, 0.62, 0.22),
    vec3<f32>(1.00, 0.96, 0.80),
    vec3<f32>(1.00, 1.00, 0.92),
    vec3<f32>(0.55, 0.58, 0.66),
    vec3<f32>(0.80, 0.72, 0.58),
    vec3<f32>(0.98, 0.99, 1.00),
    vec3<f32>(0.56, 0.50, 0.46),
    vec3<f32>(0.96, 0.92, 0.78),
    vec3<f32>(0.86, 0.86, 0.98),
    vec3<f32>(0.90, 0.98, 0.86),
    vec3<f32>(0.82, 1.00, 0.50),
    vec3<f32>(0.90, 0.96, 1.00),
    vec3<f32>(0.38, 0.28, 0.46),
    vec3<f32>(1.00, 1.00, 1.00),
    vec3<f32>(0.78, 0.70, 0.48),
    vec3<f32>(0.42, 0.30, 0.12),
    vec3<f32>(0.56, 0.56, 0.46),
    vec3<f32>(0.95, 0.85, 0.60),
    vec3<f32>(0.96, 0.76, 0.46),
    vec3<f32>(0.86, 0.97, 0.98),
    vec3<f32>(1.00, 0.90, 0.55),
    vec3<f32>(1.00, 0.60, 0.14),
    vec3<f32>(0.26, 0.26, 0.30),
    vec3<f32>(0.42, 0.46, 0.56),
    vec3<f32>(0.58, 0.78, 0.96),
    vec3<f32>(0.18, 0.36, 0.50),
    vec3<f32>(0.62, 0.14, 0.62),
    vec3<f32>(1.00, 0.96, 0.88),
    vec3<f32>(0.40, 0.88, 0.78),
    vec3<f32>(0.60, 1.00, 0.88),
    vec3<f32>(0.88, 0.80, 0.66),
    vec3<f32>(0.20, 0.14, 0.08),
    vec3<f32>(0.74, 0.54, 0.44),
    vec3<f32>(1.00, 0.78, 0.50),
    vec3<f32>(0.22, 0.36, 0.24),
    vec3<f32>(1.00, 1.00, 0.90),
    vec3<f32>(0.62, 0.58, 0.50),
    vec3<f32>(0.55, 0.65, 0.80),
    vec3<f32>(0.82, 0.90, 0.92),
    vec3<f32>(0.28, 0.02, 0.08),
    vec3<f32>(0.88, 0.68, 0.78),
    vec3<f32>(0.92, 0.86, 0.50),
    vec3<f32>(0.30, 0.30, 0.34),
    vec3<f32>(0.82, 0.85, 0.92),
    vec3<f32>(0.36, 0.38, 0.46),
    vec3<f32>(0.70, 0.50, 0.26),
    vec3<f32>(0.84, 1.00, 0.92),
    vec3<f32>(0.90, 0.90, 1.00),
    vec3<f32>(1.00, 0.94, 0.58),
    vec3<f32>(1.00, 0.94, 0.96),
    vec3<f32>(0.86, 0.30, 0.12),
    vec3<f32>(0.52, 0.54, 0.62),
    vec3<f32>(0.78, 0.80, 0.86),
    vec3<f32>(0.34, 0.18, 0.08),
    vec3<f32>(0.40, 0.18, 0.08),
    vec3<f32>(0.62, 0.74, 0.42),
    vec3<f32>(0.70, 0.52, 0.28),
    vec3<f32>(0.86, 0.93, 1.00),
    vec3<f32>(0.78, 0.88, 1.00),
    vec3<f32>(0.70, 0.82, 0.95),
    vec3<f32>(0.85, 0.74, 0.40),
    vec3<f32>(0.82, 0.92, 0.92),
    vec3<f32>(0.62, 0.58, 0.50),
    vec3<f32>(0.80, 0.78, 0.72),
    vec3<f32>(0.88, 0.88, 0.90),
    vec3<f32>(0.80, 0.76, 0.66),
    vec3<f32>(0.45, 0.45, 0.46),
    vec3<f32>(0.35, 0.25, 0.15),
    vec3<f32>(0.88, 0.88, 0.88),
    vec3<f32>(0.30, 0.22, 0.18),
    vec3<f32>(0.72, 0.74, 0.80),
    vec3<f32>(0.18, 0.19, 0.22),
    vec3<f32>(0.65, 0.50, 0.15),
    vec3<f32>(0.12, 0.11, 0.10),
    vec3<f32>(0.40, 0.80, 0.40),
    vec3<f32>(0.55, 0.38, 0.10)
);
// scale, contrast, sheen, variant
const LK_PAR: array<vec4<f32>, 110> = array<vec4<f32>, 110>(
    vec4<f32>(1.00, 0.30, 0.00, 0.0),
    vec4<f32>(1.00, 0.30, 0.00, 0.0),
    vec4<f32>(1.00, 0.30, 0.00, 0.0),
    vec4<f32>(1.00, 0.30, 0.00, 0.0),
    vec4<f32>(1.00, 0.30, 0.00, 0.0),
    vec4<f32>(1.00, 0.30, 0.00, 0.0),
    vec4<f32>(1.00, 0.30, 0.00, 0.0),
    vec4<f32>(1.00, 0.30, 0.00, 0.0),
    vec4<f32>(1.00, 0.30, 0.00, 0.0),
    vec4<f32>(1.00, 0.30, 0.00, 0.0),
    vec4<f32>(1.00, 0.40, 0.10, 0.0),
    vec4<f32>(1.00, 0.60, 0.10, 0.0),
    vec4<f32>(1.00, 0.40, 0.60, 0.0),
    vec4<f32>(1.00, 0.50, 0.80, 0.0),
    vec4<f32>(1.00, 0.30, 1.00, 1.0),
    vec4<f32>(1.00, 0.35, 0.90, 0.0),
    vec4<f32>(3.00, 0.50, 0.90, 0.0),
    vec4<f32>(2.00, 0.50, 0.10, 0.0),
    vec4<f32>(2.00, 0.40, 0.50, 0.0),
    vec4<f32>(1.00, 0.30, 0.70, 0.0),
    vec4<f32>(1.00, 0.30, 0.60, 0.0),
    vec4<f32>(1.00, 0.30, 0.60, 0.0),
    vec4<f32>(1.00, 0.30, 0.60, 3.0),
    vec4<f32>(1.00, 0.30, 0.80, 1.0),
    vec4<f32>(1.00, 0.30, 0.00, 5.0),
    vec4<f32>(1.00, 0.80, 0.00, 0.0),
    vec4<f32>(1.00, 0.30, 0.00, 0.0),
    vec4<f32>(1.00, 0.12, 0.70, 0.0),
    vec4<f32>(1.00, 0.60, 0.60, 1.0),
    vec4<f32>(1.00, 0.30, 0.60, 0.0),
    vec4<f32>(1.00, 0.30, 0.50, 3.0),
    vec4<f32>(1.00, 0.30, 0.60, 0.0),
    vec4<f32>(1.00, 0.30, 0.80, 1.0),
    vec4<f32>(1.00, 0.30, 1.00, 1.0),
    vec4<f32>(1.00, 0.30, 0.50, 4.0),
    vec4<f32>(1.00, 0.15, 0.80, 3.0),
    vec4<f32>(1.00, 0.30, 0.80, 1.0),
    vec4<f32>(1.00, 0.40, 1.00, 0.0),
    vec4<f32>(1.00, 0.30, 0.00, 0.0),
    vec4<f32>(1.00, 0.40, 1.30, 0.0),
    vec4<f32>(1.00, 0.60, 0.80, 1.0),
    vec4<f32>(1.00, 0.30, 0.80, 2.0),
    vec4<f32>(1.00, 0.30, 0.80, 2.0),
    vec4<f32>(1.00, 0.30, 0.60, 1.0),
    vec4<f32>(1.00, 0.30, 0.50, 3.0),
    vec4<f32>(1.00, 0.30, 0.60, 2.0),
    vec4<f32>(1.00, 0.50, 0.00, 0.0),
    vec4<f32>(1.00, 0.30, 0.70, 0.0),
    vec4<f32>(1.00, 0.30, 0.00, 1.0),
    vec4<f32>(1.00, 0.40, 0.50, 0.0),
    vec4<f32>(1.00, 0.30, 0.40, 1.0),
    vec4<f32>(1.00, 0.30, 0.00, 0.0),
    vec4<f32>(1.00, 0.40, 1.00, 0.0),
    vec4<f32>(1.00, 0.30, 0.00, 0.0),
    vec4<f32>(1.00, 0.30, 0.00, 5.0),
    vec4<f32>(1.00, 0.30, 0.00, 5.0),
    vec4<f32>(1.00, 0.40, 0.20, 0.0),
    vec4<f32>(2.00, 0.60, 1.00, 0.0),
    vec4<f32>(1.00, 0.30, 0.00, 1.0),
    vec4<f32>(1.00, 0.60, 0.00, 0.0),
    vec4<f32>(1.00, 0.50, 0.90, 0.0),
    vec4<f32>(1.00, 0.30, 1.00, 1.0),
    vec4<f32>(1.00, 0.30, 1.00, 0.0),
    vec4<f32>(1.00, 0.30, 0.90, 1.0),
    vec4<f32>(1.00, 0.10, 0.00, 0.0),
    vec4<f32>(1.00, 0.30, 0.00, 0.0),
    vec4<f32>(1.00, 0.30, 0.60, 1.0),
    vec4<f32>(1.00, 0.30, 1.00, 1.0),
    vec4<f32>(1.00, 0.50, 0.10, 0.0),
    vec4<f32>(1.00, 0.30, 1.00, 0.0),
    vec4<f32>(2.00, 0.50, 0.30, 0.0),
    vec4<f32>(1.00, 0.40, 0.50, 0.0),
    vec4<f32>(1.00, 0.50, 0.80, 0.0),
    vec4<f32>(1.00, 0.50, 0.10, 0.0),
    vec4<f32>(2.00, 0.40, 0.90, 0.0),
    vec4<f32>(1.00, 0.25, 0.40, 0.0),
    vec4<f32>(1.00, 0.30, 1.00, 0.0),
    vec4<f32>(1.00, 0.30, 0.00, 2.0),
    vec4<f32>(1.00, 0.25, 0.00, 0.0),
    vec4<f32>(1.00, 0.40, 0.00, 1.0),
    vec4<f32>(1.00, 0.30, 0.60, 0.0),
    vec4<f32>(1.00, 0.30, 0.60, 0.0),
    vec4<f32>(1.00, 0.30, 0.60, 3.0),
    vec4<f32>(1.00, 0.30, 0.60, 0.0),
    vec4<f32>(1.00, 0.30, 0.50, 3.0),
    vec4<f32>(1.00, 0.40, 1.00, 0.0),
    vec4<f32>(1.00, 0.40, 0.90, 1.0),
    vec4<f32>(1.00, 0.30, 0.00, 3.0),
    vec4<f32>(1.00, 0.40, 0.00, 0.0),
    vec4<f32>(1.00, 0.30, 0.00, 3.0),
    vec4<f32>(1.00, 0.30, 0.00, 0.0),
    vec4<f32>(1.00, 0.15, 0.00, 0.0),
    vec4<f32>(1.00, 0.25, 0.50, 0.0),
    vec4<f32>(1.00, 0.25, 0.50, 0.0),
    vec4<f32>(1.00, 0.25, 0.50, 0.0),
    vec4<f32>(1.00, 0.20, 0.40, 0.0),
    vec4<f32>(1.00, 0.40, 0.20, 0.0),
    vec4<f32>(1.00, 0.25, 0.30, 0.0),
    vec4<f32>(1.00, 0.25, 0.50, 0.0),
    vec4<f32>(1.00, 0.30, 0.20, 0.0),
    vec4<f32>(1.00, 0.35, 0.20, 0.0),
    vec4<f32>(1.00, 0.50, 0.30, 0.0),
    vec4<f32>(1.00, 0.30, 0.10, 0.0),
    vec4<f32>(1.00, 0.30, 0.00, 0.0),
    vec4<f32>(1.00, 0.30, 0.40, 0.0),
    vec4<f32>(1.00, 0.40, 0.90, 0.0),
    vec4<f32>(1.00, 0.40, 0.90, 0.0),
    vec4<f32>(1.00, 0.20, 0.30, 1.0),
    vec4<f32>(1.00, 0.20, 0.40, 0.0),
    vec4<f32>(1.00, 0.20, 0.30, 1.0)
);
// ML-GEN-END
