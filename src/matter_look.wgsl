// Per-element material looks (chem team). Appended after vis.wgsl in the sim/matter/vis module (same bindings).
// matter_look(e, x, y) returns the display-space albedo of a condensed-matter cell. Every element has a style
// (LK_STYLE), a secondary colour (LK_C2) and parameters (LK_PAR = scale, contrast, sheen, variant); the primary
// colour is E_COL. Patterns are built from the cell coordinate only, so they stay crisp pixels at 1 cell and read as
// grains / crystals / flakes / ribbons when zoomed in. State overlays: cast (sintered smooth grain with layering),
// wet (darker, glossy), charred (burning fuels blacken as they are consumed).
//
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
    let xo = x + select(0u, sz / 2u, (by & 1u) == 1u);
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
    let by = y / 3u;
    let xo = x + select(0u, 1u, (by & 1u) == 1u);
    let bx = xo / 3u;
    let q = vec2<f32>((f32(xo % 3u) + 0.5) / 3.0 - 0.5, (f32(y % 3u) + 0.5) / 3.0 - 0.5) * 2.0;
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
    let n = ml_n(p * 0.14);
    var c = mix(c1, c2, n * 0.45);
    var shade = 1.0 + (n - 0.5) * par.y * 3.0;
    if (y % 6u == 0u) { shade *= 0.97; }
    let d = fract((p.x - p.y * 0.5) / 11.0);
    if (d < 0.1) { c = mix(c, vec3<f32>(1.0), 0.28 * min(par.z, 1.0)); }
    return c * shade;
}

fn ml_coal(c1: vec3<f32>, c2: vec3<f32>, par: vec4<f32>, x: u32, y: u32) -> vec3<f32> {
    let by = y / 3u;
    let xo = x + select(0u, 1u, (by & 1u) == 1u);
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
        default: { return c1; }
    }
}

fn matter_look(e_in: u32, x: u32, y: u32) -> vec3<f32> {
    let e = min(e_in, N_ELEM - 1u);
    let c1 = E_COL[e];
    let style = LK_STYLE[e];
    let p = vec2<f32>(f32(x), f32(y));
    if (style == 9u) {
        // liquids: flat body colour with a very slow tint drift (render.wgsl adds ripples, depth and highlights)
        return c1 * (1.0 + (ml_n(p * 0.07) - 0.5) * 0.06);
    }
    let c2 = LK_C2[e];
    let par = LK_PAR[e];
    var col = ml_pattern(style, c1, c2, par, x, y);
    let i = y * P.w + x;
    // cast / pressed grain: sintered, smooth, with faint pour layers and a satin gloss
    if ((META[i] & CAST_BIT) != 0u) {
        var sm = mix(c1, c2, 0.12) * (0.93 + 0.14 * ml_n(p * 0.16));
        if (y % 5u == 0u) { sm *= 0.94; }
        if (fract((p.x - p.y * 0.6) / 17.0) < 0.07) { sm = mix(sm, vec3<f32>(1.0), 0.12); }
        col = mix(col, sm, 0.78);
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
        if (wet && E_PHASE[e] != 2u) {
            col = col * vec3<f32>(0.72, 0.75, 0.8) + vec3<f32>(0.04 * f32(ml_h(x, y, 41u) > 0.88));
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

// ML-GEN-BEGIN (generated by scratchpad v2/chem2/genlook.py)
const LK_STYLE: array<u32, 92> = array<u32, 92>(
    0u, 0u, 0u, 0u, 0u, 0u, 0u, 0u, 0u, 0u,
    14u, 11u, 6u, 2u, 2u, 0u, 2u, 1u, 3u, 9u,
    9u, 9u, 9u, 9u, 9u, 0u, 15u, 0u, 2u, 9u,
    9u, 9u, 9u, 9u, 9u, 8u, 9u, 10u, 7u, 5u,
    5u, 8u, 8u, 9u, 9u, 9u, 0u, 2u, 7u, 4u,
    8u, 16u, 12u, 17u, 9u, 9u, 13u, 1u, 15u, 1u,
    2u, 9u, 8u, 9u, 0u, 18u, 9u, 9u, 1u, 4u,
    2u, 5u, 0u, 1u, 1u, 2u, 8u, 7u, 0u, 11u,
    9u, 9u, 9u, 9u, 9u, 12u, 4u, 7u, 11u, 7u,
    19u, 20u
);
const LK_C2: array<vec3<f32>, 92> = array<vec3<f32>, 92>(
    vec3<f32>(0.00, 0.00, 0.00), vec3<f32>(0.00, 0.00, 0.00), vec3<f32>(0.00, 0.00, 0.00),
    vec3<f32>(0.00, 0.00, 0.00), vec3<f32>(0.00, 0.00, 0.00), vec3<f32>(0.00, 0.00, 0.00),
    vec3<f32>(0.00, 0.00, 0.00), vec3<f32>(0.00, 0.00, 0.00), vec3<f32>(0.00, 0.00, 0.00),
    vec3<f32>(0.00, 0.00, 0.00), vec3<f32>(0.55, 0.44, 0.28), vec3<f32>(0.03, 0.03, 0.04),
    vec3<f32>(1.00, 0.95, 0.45), vec3<f32>(0.78, 0.85, 0.95), vec3<f32>(1.00, 1.00, 1.00),
    vec3<f32>(0.50, 0.52, 0.56), vec3<f32>(0.80, 0.88, 0.98), vec3<f32>(0.34, 0.10, 0.07),
    vec3<f32>(0.88, 0.88, 0.84), vec3<f32>(0.00, 0.00, 0.00), vec3<f32>(0.00, 0.00, 0.00),
    vec3<f32>(0.00, 0.00, 0.00), vec3<f32>(0.00, 0.00, 0.00), vec3<f32>(0.00, 0.00, 0.00),
    vec3<f32>(0.00, 0.00, 0.00), vec3<f32>(0.66, 0.64, 0.60), vec3<f32>(0.95, 0.98, 1.00),
    vec3<f32>(1.00, 1.00, 1.00), vec3<f32>(1.00, 1.00, 1.00), vec3<f32>(0.00, 0.00, 0.00),
    vec3<f32>(0.00, 0.00, 0.00), vec3<f32>(0.00, 0.00, 0.00), vec3<f32>(0.00, 0.00, 0.00),
    vec3<f32>(0.00, 0.00, 0.00), vec3<f32>(0.00, 0.00, 0.00), vec3<f32>(1.00, 0.98, 0.88),
    vec3<f32>(0.00, 0.00, 0.00), vec3<f32>(0.30, 0.31, 0.34), vec3<f32>(0.82, 0.80, 0.74),
    vec3<f32>(0.98, 0.99, 1.00), vec3<f32>(0.14, 0.14, 0.16), vec3<f32>(0.95, 0.96, 0.97),
    vec3<f32>(0.90, 0.90, 0.95), vec3<f32>(0.00, 0.00, 0.00), vec3<f32>(0.00, 0.00, 0.00),
    vec3<f32>(0.00, 0.00, 0.00), vec3<f32>(0.28, 0.25, 0.22), vec3<f32>(0.88, 0.92, 0.98),
    vec3<f32>(0.90, 0.88, 0.80), vec3<f32>(0.78, 0.62, 0.22), vec3<f32>(0.82, 0.80, 0.72),
    vec3<f32>(0.55, 0.15, 0.10), vec3<f32>(0.45, 0.38, 0.30), vec3<f32>(0.95, 1.00, 0.97),
    vec3<f32>(0.00, 0.00, 0.00), vec3<f32>(0.00, 0.00, 0.00), vec3<f32>(0.35, 0.35, 0.37),
    vec3<f32>(0.34, 0.34, 0.37), vec3<f32>(1.00, 1.00, 1.00), vec3<f32>(0.02, 0.02, 0.03),
    vec3<f32>(0.55, 0.15, 0.55), vec3<f32>(0.00, 0.00, 0.00), vec3<f32>(1.00, 1.00, 1.00),
    vec3<f32>(0.00, 0.00, 0.00), vec3<f32>(0.88, 0.86, 0.80), vec3<f32>(0.20, 0.18, 0.14),
    vec3<f32>(0.00, 0.00, 0.00), vec3<f32>(0.00, 0.00, 0.00), vec3<f32>(0.07, 0.12, 0.09),
    vec3<f32>(0.92, 0.95, 1.00), vec3<f32>(0.97, 0.95, 0.90), vec3<f32>(0.80, 0.82, 0.86),
    vec3<f32>(0.46, 0.52, 0.60), vec3<f32>(0.30, 0.05, 0.07), vec3<f32>(1.00, 1.00, 0.97),
    vec3<f32>(1.00, 0.98, 0.92), vec3<f32>(0.20, 0.20, 0.20), vec3<f32>(0.92, 0.92, 0.88),
    vec3<f32>(0.13, 0.13, 0.15), vec3<f32>(0.60, 0.42, 0.22), vec3<f32>(0.00, 0.00, 0.00),
    vec3<f32>(0.00, 0.00, 0.00), vec3<f32>(0.00, 0.00, 0.00), vec3<f32>(0.00, 0.00, 0.00),
    vec3<f32>(0.00, 0.00, 0.00), vec3<f32>(0.75, 0.77, 0.80), vec3<f32>(0.40, 0.40, 0.45),
    vec3<f32>(0.55, 0.33, 0.15), vec3<f32>(0.12, 0.11, 0.10), vec3<f32>(0.72, 0.78, 0.60),
    vec3<f32>(0.55, 0.42, 0.28), vec3<f32>(0.85, 0.92, 1.00)
);
// scale, contrast, sheen, variant
const LK_PAR: array<vec4<f32>, 92> = array<vec4<f32>, 92>(
    vec4<f32>(1.00, 0.20, 0.00, 0.0), vec4<f32>(1.00, 0.20, 0.00, 0.0), vec4<f32>(1.00, 0.20, 0.00, 0.0),
    vec4<f32>(1.00, 0.20, 0.00, 0.0), vec4<f32>(1.00, 0.20, 0.00, 0.0), vec4<f32>(1.00, 0.20, 0.00, 0.0),
    vec4<f32>(1.00, 0.20, 0.00, 0.0), vec4<f32>(1.00, 0.20, 0.00, 0.0), vec4<f32>(1.00, 0.20, 0.00, 0.0),
    vec4<f32>(1.00, 0.20, 0.00, 0.0), vec4<f32>(1.00, 0.40, 0.10, 0.0), vec4<f32>(1.00, 0.55, 0.12, 0.0),
    vec4<f32>(1.00, 0.25, 0.35, 0.0), vec4<f32>(1.00, 0.30, 0.70, 0.0), vec4<f32>(1.00, 0.25, 1.00, 0.0),
    vec4<f32>(1.00, 0.28, 0.55, 0.0), vec4<f32>(1.00, 0.30, 0.55, 0.0), vec4<f32>(1.00, 0.50, 0.00, 0.0),
    vec4<f32>(3.00, 0.30, 0.60, 0.0), vec4<f32>(1.00, 0.30, 0.00, 0.0), vec4<f32>(1.00, 0.30, 0.00, 0.0),
    vec4<f32>(1.00, 0.30, 0.00, 0.0), vec4<f32>(1.00, 0.30, 0.00, 0.0), vec4<f32>(1.00, 0.30, 0.00, 0.0),
    vec4<f32>(1.00, 0.30, 0.00, 0.0), vec4<f32>(1.00, 0.35, 0.00, 0.0), vec4<f32>(1.00, 0.30, 0.80, 0.0),
    vec4<f32>(1.00, 0.12, 1.00, 0.0), vec4<f32>(1.00, 0.22, 0.85, 1.0), vec4<f32>(1.00, 0.30, 0.00, 0.0),
    vec4<f32>(1.00, 0.30, 0.00, 0.0), vec4<f32>(1.00, 0.30, 0.00, 0.0), vec4<f32>(1.00, 0.30, 0.00, 0.0),
    vec4<f32>(1.00, 0.30, 0.00, 0.0), vec4<f32>(1.00, 0.30, 0.00, 0.0), vec4<f32>(1.00, 0.10, 0.50, 0.0),
    vec4<f32>(1.00, 0.30, 0.00, 0.0), vec4<f32>(1.00, 0.50, 0.90, 0.0), vec4<f32>(1.00, 0.20, 0.00, 0.0),
    vec4<f32>(1.00, 0.35, 1.00, 0.0), vec4<f32>(1.00, 0.40, 0.50, 0.0), vec4<f32>(1.00, 0.10, 0.90, 0.0),
    vec4<f32>(1.00, 0.10, 0.80, 0.0), vec4<f32>(1.00, 0.30, 0.00, 0.0), vec4<f32>(1.00, 0.30, 0.00, 0.0),
    vec4<f32>(1.00, 0.30, 0.00, 0.0), vec4<f32>(1.00, 0.30, 0.05, 0.0), vec4<f32>(1.00, 0.30, 0.70, 0.0),
    vec4<f32>(1.00, 0.25, 0.00, 1.0), vec4<f32>(1.00, 0.35, 0.30, 0.0), vec4<f32>(1.00, 0.08, 0.20, 0.0),
    vec4<f32>(1.00, 0.30, 0.10, 0.0), vec4<f32>(1.00, 0.20, 0.70, 0.0), vec4<f32>(1.00, 0.20, 1.00, 0.0),
    vec4<f32>(1.00, 0.30, 0.00, 0.0), vec4<f32>(1.00, 0.30, 0.00, 0.0), vec4<f32>(1.00, 0.45, 0.10, 0.0),
    vec4<f32>(1.00, 0.45, 0.90, 0.0), vec4<f32>(1.00, 0.35, 0.60, 1.0), vec4<f32>(1.00, 0.30, 0.20, 0.0),
    vec4<f32>(1.00, 0.40, 1.00, 0.0), vec4<f32>(1.00, 0.30, 0.00, 0.0), vec4<f32>(1.00, 0.06, 0.80, 0.0),
    vec4<f32>(1.00, 0.30, 0.00, 0.0), vec4<f32>(1.00, 0.10, 0.00, 0.0), vec4<f32>(1.00, 0.30, 0.00, 0.0),
    vec4<f32>(1.00, 0.30, 0.00, 0.0), vec4<f32>(1.00, 0.30, 0.00, 0.0), vec4<f32>(1.00, 0.40, 0.30, 0.0),
    vec4<f32>(1.00, 0.25, 0.90, 0.0), vec4<f32>(1.00, 0.20, 0.40, 0.0), vec4<f32>(1.00, 0.40, 0.60, 0.0),
    vec4<f32>(1.00, 0.25, 0.45, 0.0), vec4<f32>(1.00, 0.45, 0.15, 0.0), vec4<f32>(1.00, 0.18, 0.50, 0.0),
    vec4<f32>(1.00, 0.22, 0.55, 0.0), vec4<f32>(1.00, 0.08, 0.30, 0.0), vec4<f32>(1.00, 0.15, 0.00, 2.0),
    vec4<f32>(1.00, 0.35, 0.12, 0.0), vec4<f32>(1.00, 0.40, 0.00, 1.0), vec4<f32>(1.00, 0.30, 0.00, 0.0),
    vec4<f32>(1.00, 0.30, 0.00, 0.0), vec4<f32>(1.00, 0.30, 0.00, 0.0), vec4<f32>(1.00, 0.30, 0.00, 0.0),
    vec4<f32>(1.00, 0.30, 0.00, 0.0), vec4<f32>(1.00, 0.15, 1.00, 0.0), vec4<f32>(1.00, 0.35, 0.90, 1.0),
    vec4<f32>(1.00, 0.25, 0.00, 3.0), vec4<f32>(1.00, 0.40, 0.15, 0.0), vec4<f32>(1.00, 0.25, 0.20, 3.0),
    vec4<f32>(1.00, 0.30, 0.00, 0.0), vec4<f32>(1.00, 0.15, 0.50, 0.0)
);
// ML-GEN-END
