//! Preset engines (tile layouts only; gas state is initialised on the GPU).
use crate::gpu::{Op, H, W};

pub const WALL: u32 = 1;
const EMPTY_TILE: u32 = 0;
pub const FUEL_IN: u32 = 2;
pub const O2_IN: u32 = 3;
pub const AIR_IN: u32 = 4;
pub const FAN: u32 = 5;
pub const IGNITER: u32 = 6;
pub const VENT: u32 = 7;

pub const STEEL: u32 = 0;
pub const INCONEL: u32 = 4;
pub const COOLED_CU: u32 = 6;
pub const WOOD: u32 = 7;
pub const PROPELLANT: u32 = 8;
pub const EXPLOSIVE: u32 = 9;
pub const MATTER: u32 = 8;

pub struct Scene {
    pub meta: Vec<u32>,
    pub inlet_p: f32,
    pub o2_p: f32,
    /// edits applied right after loading (e.g. pre-filled gas)
    pub ops: Vec<Op>,
}

impl Scene {
    pub fn empty() -> Self {
        Scene { meta: vec![0; (W * H) as usize], inlet_p: 3.0, o2_p: 3.0, ops: vec![] }
    }

    pub fn put(&mut self, x: i32, y: i32, tile: u32, mat: u32, dir: u32) {
        if x >= 0 && y >= 0 && x < W as i32 && y < H as i32 {
            self.meta[(y as u32 * W + x as u32) as usize] = tile | (mat << 8) | (dir << 16);
        }
    }

    pub fn turbojet() -> Self {
        let mut s = Scene::empty();
        let cy = (H / 2) as i32;
        let half = 19;
        let (top, bot) = (cy - half, cy + half);
        let (x0, len) = (90, 210);
        let th = 4;
        for x in x0..=x0 + len {
            let hot = x >= x0 + 20 && x < x0 + 160;
            let m = if hot { INCONEL } else { STEEL };
            for t in 1..=th {
                s.put(x, top - t, WALL, m, 0);
                s.put(x, bot + t - 1, WALL, m, 0);
            }
        }
        // converging nozzle then straight throat
        let (n0, n1) = (x0 + 100, x0 + 155);
        for x in n0..=x0 + len {
            let h = if x < n1 { half as f32 - 11.0 * (x - n0) as f32 / (n1 - n0) as f32 } else { half as f32 - 11.0 };
            let h = h.round() as i32;
            for y in top..cy - h {
                s.put(x, y, WALL, INCONEL, 0);
            }
            for y in cy + h..bot {
                s.put(x, y, WALL, INCONEL, 0);
            }
        }
        for x in x0 + 4..x0 + 14 {
            for y in top..bot {
                s.put(x, y, FAN, 0, 0);
            }
        }
        for x in x0 + 40..x0 + 52 {
            s.put(x, top - 1, FUEL_IN, 0, 0);
            s.put(x, bot, FUEL_IN, 0, 0);
        }
        for x in x0 + 76..x0 + 79 {
            s.put(x, top - 1, IGNITER, 0, 0);
            s.put(x, bot, IGNITER, 0, 0);
        }
        // flame holder
        for k in 0..8 {
            s.put(x0 + 66 + k / 2, cy - 3 + k, WALL, INCONEL, 0);
            s.put(x0 + 66 + k / 2, cy + 3 - k, WALL, INCONEL, 0);
        }
        s.inlet_p = 3.0;
        s
    }

    pub fn rocket() -> Self {
        let mut s = Scene::empty();
        let cy = (H / 2) as i32;
        let th = 12;
        // chamber x 60..140 (half-height 22), converging to a throat at x=160 (half 5),
        // then a bell to x=220 (half 20): exit/throat area ratio 4 -> roughly Mach 3 exhaust
        let (x_face, x_conv, x_throat, x_exit) = (60, 140, 160, 220);
        let half_at = |x: i32| -> i32 {
            if x < x_conv {
                22
            } else if x < x_throat {
                let t = (x - x_conv) as f32 / (x_throat - x_conv) as f32;
                (22.0 - 17.0 * (t * std::f32::consts::FRAC_PI_2).sin()).round() as i32
            } else {
                let t = (x - x_throat) as f32 / (x_exit - x_throat) as f32;
                (5.0 + 15.0 * t.powf(0.6)).round() as i32
            }
        };
        for x in x_face..=x_exit {
            let h = half_at(x);
            for t in 0..th {
                // cooled copper liner on the hot side, steel jacket outside
                let m = if t < 2 { COOLED_CU } else { 3 }; // titanium structural jacket
                s.put(x, cy - h - 1 - t, WALL, m, 0);
                s.put(x, cy + h + t, WALL, m, 0);
            }
        }
        // injector plate behind the face
        for x in x_face - 5..x_face {
            for y in cy - 22 - th..cy + 22 + th {
                s.put(x, y, WALL, STEEL, 0);
            }
        }
        // showerhead injector: 1 methane row per 3 oxygen rows (about 4:1 O2:CH4 by mass, stoichiometric)
        for y in cy - 22..cy + 22 {
            let t = if (y - cy).rem_euclid(4) == 0 { FUEL_IN } else { O2_IN };
            s.put(x_face, y, t, 0, 0);
        }
        // igniters: a spark torch on the injector face (centre rows) lights the very first mixed gas, like a real
        // engine's ignition lead; late ignition of accumulated propellant is a "hard start" that bursts the case
        for y in cy - 2..=cy + 1 {
            s.put(x_face, y, IGNITER, 0, 0);
        }
        for x in x_face + 3..x_face + 6 {
            s.put(x, cy - 23, IGNITER, 0, 0);
            s.put(x, cy + 22, IGNITER, 0, 0);
        }
        // Regenerative cooling follows the real gas boundary: every wall cell within 2.5 cells of open gas
        // becomes cooled copper (a vertical liner leaves steel stair-steps in the flame on sloped nozzles).
        let (w, h) = (W as i32, H as i32);
        let is_wall = |m: &Vec<u32>, x: i32, y: i32| x >= 0 && y >= 0 && x < w && y < h && (m[(y * w + x) as usize] & 0xff) == WALL;
        let snapshot = s.meta.clone();
        for y in 0..h {
            for x in 0..w {
                if !is_wall(&snapshot, x, y) {
                    continue;
                }
                let mut near_gas = false;
                'scan: for dy in -2..=2i32 {
                    for dx in -2..=2i32 {
                        if dx * dx + dy * dy > 6 {
                            continue;
                        }
                        let (xx, yy) = (x + dx, y + dy);
                        if xx >= 0 && yy >= 0 && xx < w && yy < h {
                            let t = snapshot[(yy * w + xx) as usize] & 0xff;
                            if t == EMPTY_TILE || t == FUEL_IN || t == O2_IN {
                                near_gas = true;
                                break 'scan;
                            }
                        }
                    }
                }
                let idx = (y * w + x) as usize;
                let mat = if near_gas { COOLED_CU } else { 3 };
                s.meta[idx] = WALL | (mat << 8);
            }
        }
        s.inlet_p = 12.0;
        s.o2_p = 12.0;
        s
    }

    /// Three sealed vessels of different metals filled with a stoichiometric fuel/air
    /// mix and an igniter each: shows which wall material survives the deflagration.
    pub fn vessels() -> Self {
        let mut s = Scene::empty();
        let mats = [1u32, STEEL, 3u32]; // aluminium, steel, titanium
        let cy = (H / 2) as i32;
        for (k, &m) in mats.iter().enumerate() {
            let cx = 110 + k as i32 * 145;
            let (x0, x1, y0, y1) = (cx - 26, cx + 26, cy - 26, cy + 26);
            for x in x0..=x1 {
                for y in y0..=y1 {
                    let edge = x < x0 + 3 || x > x1 - 3 || y < y0 + 3 || y > y1 - 3;
                    if edge {
                        s.put(x, y, WALL, m, 0);
                    }
                }
            }
            s.put(cx, cy, IGNITER, 0, 0);
            s.ops.push(Op { kind: 2, shape: 1, x0: x0 + 3, y0: y0 + 3, x1: x1 - 3, y1: y1 - 3, r: 0, tile: 0, mat: 0, dir: 0, _p: [0; 2] });
        }
        s
    }

    /// Test: a long strip of near-stoichiometric methane/air lit at the left end.
    pub fn flame_test() -> Self {
        let mut s = Scene::empty();
        let op = |kind, x0, y0, x1, y1| Op { kind, shape: 1, x0, y0, x1, y1, r: 0, tile: 0, mat: 0, dir: 0, _p: [0; 2] };
        s.ops.push(op(2, 0, 0, 511, 287));
        s.ops.push(op(5, 10, 100, 16, 188));
        s
    }

    /// Showcase: a rich methane cloud lit at its base, which should roll up into a sooty fireball.
    pub fn fireball() -> Self {
        let mut s = Scene::empty();
        let disc = |kind, x, y, r| Op { kind, shape: 0, x0: x, y0: y, x1: x, y1: y, r, tile: 0, mat: 0, dir: 0, _p: [0; 2] };
        for _ in 0..3 {
            s.ops.push(disc(2, 256, 200, 40));
        }
        for x in 254..259 {
            s.put(x, 241, IGNITER, 0, 0);
        }
        // floor
        for x in 0..W as i32 {
            for y in (H as i32 - 6)..H as i32 {
                s.put(x, y, WALL, STEEL, 0);
            }
        }
        s
    }

    /// A vertical pipe leaking methane, lit at the lip: a sooty, buoyant diffusion flame.
    pub fn flare() -> Self {
        let mut s = Scene::empty();
        let h = H as i32;
        for x in 0..W as i32 {
            for y in h - 6..h {
                s.put(x, y, WALL, STEEL, 0);
            }
        }
        let (x0, x1, top) = (248, 264, h - 26);
        for y in top..h - 6 {
            for t in 0..3 {
                s.put(x0 + t, y, WALL, STEEL, 0);
                s.put(x1 - t, y, WALL, STEEL, 0);
            }
        }
        for x in x0 + 3..=x1 - 3 {
            s.put(x, h - 7, FUEL_IN, 0, 0);
        }
        s.put(x0 + 1, top - 1, IGNITER, 0, 0);
        s.put(x1 - 1, top - 1, IGNITER, 0, 0);
        s.inlet_p = 1.5;
        s
    }

    fn floor(&mut self) {
        let h = H as i32;
        for x in 0..W as i32 {
            for y in h - 6..h {
                self.put(x, y, WALL, STEEL, 0);
            }
        }
    }

    /// A small log fire: round logs (~8 cm) resting on each other with open air gaps, lit by tinder igniters
    /// at the bed. No enclosed slots (a closed 1-cell slit between logs traps pyrolysis gas and gives
    /// constant-volume pocket explosions) and no hot-gas torch (a 2700 K gas blob at constant density is a
    /// 9 atm pressure step). Wood pyrolyses, the volatiles burn anchored on the logs, the logs char away.
    pub fn campfire() -> Self {
        let mut s = Scene::empty();
        s.floor();
        let floor = H as i32 - 6; // first gas row above the floor is floor - 1
        let cx = (W / 2) as i32;
        let r = 5.6f32; // log radius in cells (~11 cm logs)
        let mut log = |s: &mut Scene, lx: f32, ly: f32| {
            for y in (ly - r - 1.0) as i32..=(ly + r + 1.0) as i32 {
                for x in (lx - r - 1.0) as i32..=(lx + r + 1.0) as i32 {
                    let (dx, dy) = (x as f32 + 0.5 - lx, y as f32 + 0.5 - ly);
                    if dx * dx + dy * dy <= r * r && y < floor {
                        s.put(x, y, WALL, WOOD, 0);
                    }
                }
            }
        };
        // bottom row: 4 logs with 4-cell gaps; second row: 3 logs in the notches; top: 2 logs
        let pitch = 2.0 * r + 4.0;
        let y0 = floor as f32 - r;
        for k in 0..4 {
            log(&mut s, cx as f32 + (k as f32 - 1.5) * pitch, y0);
        }
        for k in 0..3 {
            log(&mut s, cx as f32 + (k as f32 - 1.0) * pitch, y0 - 2.0 * r - 0.5);
        }
        for k in 0..2 {
            log(&mut s, cx as f32 + (k as f32 - 0.5) * pitch, y0 - 4.0 * r - 1.0);
        }
        // tinder/kindling: igniters packed against the bottom logs in the three central gaps (contact
        // heating chars and pyrolyses the log surface; the volatiles light right there)
        let idx = |x: i32, y: i32| (y as u32 * W + x as u32) as usize;
        let is_wood = |s: &Scene, x: i32, y: i32| s.meta[idx(x, y)] == (WALL | (WOOD << 8));
        for k in 0..3 {
            let gx = cx + ((k as f32 - 1.0) * pitch) as i32;
            // one igniter on each log flank at log-centre height (the gap stays open between them)
            let y = y0 as i32;
            for dir in [-1, 1] {
                let mut x = gx;
                while s.meta[idx(x + dir, y)] == 0 && (x - gx).abs() < 8 {
                    x += dir;
                }
                s.put(x, y, IGNITER, 0, 0);
            }
        }
        // fill 1-cell dead-end slits (log/floor contact wedges, gas trapped behind kindling): gas emitted into
        // them cannot escape and would build pocket pressure
        for _ in 0..6 {
            let mut fill = vec![];
            for y in floor - 30..floor {
                for x in cx - 50..cx + 50 {
                    if s.meta[idx(x, y)] != 0 {
                        continue;
                    }
                    let open = [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().filter(|(dx, dy)| s.meta[idx(x + dx, y + dy)] == 0).count();
                    if open <= 1 {
                        fill.push((x, y));
                    }
                }
            }
            if fill.is_empty() {
                break;
            }
            for (x, y) in fill {
                s.put(x, y, WALL, WOOD, 0);
            }
        }
        s
    }

    /// Solid rocket motor: steel case, hollow propellant grain, nozzle, igniter at the head end.
    pub fn solid_rocket() -> Self {
        let mut s = Scene::empty();
        let cy = (H / 2) as i32;
        let (x0, x1) = (70, 190);
        let case_h = 26;
        for x in x0..=x1 {
            for t in 0..4 {
                s.put(x, cy - case_h - t, WALL, STEEL, 0);
                s.put(x, cy + case_h + t, WALL, STEEL, 0);
            }
        }
        for x in x0 - 4..x0 {
            for y in cy - case_h - 3..=cy + case_h + 3 {
                s.put(x, y, WALL, STEEL, 0);
            }
        }
        // grain: propellant lining the case, with a central bore
        for x in x0..x1 - 6 {
            for y in cy - case_h + 1..cy + case_h {
                if (y - cy).abs() > 7 {
                    s.put(x, y, WALL, PROPELLANT, 0);
                }
            }
        }
        // converging-diverging nozzle
        for x in x1..x1 + 50 {
            let h = if x < x1 + 18 { 25 - (x - x1) } else { 7 + ((x - x1 - 18) as f32 * 0.45) as i32 };
            for t in 0..5 {
                s.put(x, cy - h - 1 - t, WALL, INCONEL, 0);
                s.put(x, cy + h + t, WALL, INCONEL, 0);
            }
        }
        for y in cy - 7..=cy + 7 {
            s.put(x0, y, IGNITER, 0, 0);
        }
        s
    }

    /// A titanium vault with an explosive charge against its wall.
    pub fn demolition() -> Self {
        let mut s = Scene::empty();
        s.floor();
        let h = H as i32;
        let (vx0, vx1, vy0) = (300, 420, h - 110);
        for x in vx0..=vx1 {
            for y in vy0..h - 6 {
                let edge = x < vx0 + 4 || x > vx1 - 4 || y < vy0 + 4;
                if edge {
                    s.put(x, y, WALL, 3, 0);
                }
            }
        }
        for x in vx0 - 14..vx0 {
            for y in h - 40..h - 6 {
                s.put(x, y, WALL, EXPLOSIVE, 0);
            }
        }
        s.put(vx0 - 15, h - 10, IGNITER, 0, 0);
        s
    }

    fn fill(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, tile: u32, mat: u32) {
        for x in x0..=x1 {
            for y in y0..=y1 {
                self.put(x, y, tile, mat, 0);
            }
        }
    }

    /// Chemistry bench: piles and pools of real substances on a steel floor.
    pub fn chem_lab() -> Self {
        let mut s = Scene::empty();
        s.floor();
        let fl = H as i32 - 7;
        // black powder: saltpeter + charcoal mixed (checkerboard = intimately mixed grains)
        for x in 30..80 {
            for y in fl - 18..=fl {
                s.put(x, y, MATTER, if (x + y) % 3 == 0 { 11 } else { 13 }, 0);
            }
        }
        // thermite: iron oxide + aluminium
        for x in 120..160 {
            for y in fl - 14..=fl {
                s.put(x, y, MATTER, if (x + y) % 2 == 0 { 15 } else { 17 }, 0);
            }
        }
        // kerosene pool in a steel tray, with water underneath
        s.fill(200, fl - 16, 202, fl, WALL, STEEL);
        s.fill(270, fl - 16, 272, fl, WALL, STEEL);
        s.fill(203, fl - 7, 269, fl, MATTER, 19);
        s.fill(203, fl - 13, 269, fl - 8, MATTER, 20);
        // nitroglycerin drop on a ledge
        s.fill(330, fl - 40, 360, fl - 38, WALL, STEEL);
        s.fill(340, fl - 44, 350, fl - 41, MATTER, 23);
        // a sand pile
        s.fill(420, fl - 25, 470, fl, MATTER, 10);
        // igniters
        s.put(29, fl, IGNITER, 0, 0);
        s.put(119, fl, IGNITER, 0, 0);
        s.put(236, fl - 14, IGNITER, 0, 0);
        s
    }

    /// Test: an aluminium plate over a methane flare: it should melt, drip, and re-solidify below.
    pub fn melt_test() -> Self {
        let mut s = Scene::flare();
        let top = H as i32 - 70;
        s.fill(236, top - 30, 276, top - 27, WALL, 1);
        s.ops.push(Op { kind: 5, shape: 1, x0: 236, y0: top - 30, x1: 276, y1: top - 27, r: 0, tile: 0, mat: 0, dir: 0, _p: [0; 2] });
        s
    }

    // ------------------------------------------------------------------------------------------
    // Chemistry showcase scenes (chem agent). Element ids: see `elements::MATTER_INFO` / matter.wgsl.

    /// Look a scene up by a short name (bench / screenshot hooks, `SHOWCASE`, `MATERIAL_SCENES`).
    pub fn by_name(name: &str) -> Option<Scene> {
        SHOWCASE.iter().find(|s| s.0 == name).map(|s| (s.3)())
            .or_else(|| MATERIAL_SCENES.iter().find(|s| s.0 == name).map(|s| (s.3)()))
    }

    /// Put a cast / pressed cell (META dir bit 0 = packed grain).
    fn cast(&mut self, x: i32, y: i32, el: u32) {
        self.put(x, y, MATTER, el, 1);
    }

    /// 3x5 digit glyph in steel (labels for the material board).
    fn digit(&mut self, d: u32, x0: i32, y0: i32) {
        const G: [[u8; 5]; 10] = [
            [7, 5, 5, 5, 7], [2, 6, 2, 2, 7], [7, 1, 7, 4, 7], [7, 1, 7, 1, 7], [5, 5, 7, 1, 1],
            [7, 4, 7, 1, 7], [7, 4, 7, 5, 7], [7, 1, 1, 1, 1], [7, 5, 7, 5, 7], [7, 5, 7, 1, 7],
        ];
        for (r, row) in G[d as usize].iter().enumerate() {
            for c in 0..3 {
                if row >> (2 - c) & 1 == 1 {
                    self.put(x0 + c, y0 + r as i32, WALL, STEEL, 0);
                }
            }
        }
    }

    /// Every matter element (ids 10..=91) as a swatch in its own steel tray, labelled with its id in steel digits,
    /// plus two cast propellant swatches. 12 columns x 7 rows; 40 x 40 cells each. Look-dev / screenshot scene.
    pub fn material_board() -> Self {
        let mut s = Scene::empty();
        let (cw, ch) = (42, 40);
        let (ox, oy) = (4, 6);
        for k in 0..84u32 {
            let (col, row) = ((k % 12) as i32, (k / 12) as i32);
            let x0 = ox + col * cw;
            let y0 = oy + row * ch;
            let floor_y = y0 + ch - 4;
            // tray: floor + two side walls
            for x in x0..x0 + 36 {
                s.put(x, floor_y, WALL, STEEL, 0);
                s.put(x, floor_y + 1, WALL, STEEL, 0);
            }
            for y in floor_y - 22..floor_y {
                s.put(x0, y, WALL, STEEL, 0);
                s.put(x0 + 35, y, WALL, STEEL, 0);
            }
            let id = 10 + k;
            // label above the tray
            let (d1, d2) = (id / 10, id % 10);
            if id < 92 {
                s.digit(d1, x0 + 13, y0 + 2);
                s.digit(d2, x0 + 18, y0 + 2);
            }
            let (fx0, fx1) = (x0 + 1, x0 + 34);
            let top = floor_y - 14;
            for x in fx0..=fx1 {
                for y in top..floor_y {
                    let h = ((x * 73856093) ^ (y * 19349663)) as u32 % 100;
                    if id < 92 {
                        s.put(x, y, MATTER, id, 0);
                    } else if id == 92 {
                        s.cast(x, y, if h < 65 { 13 } else { 14 }); // rocket candy, cast
                    } else {
                        s.cast(x, y, if h < 70 { 16 } else if h < 86 { 15 } else { 66 }); // APCP, cast
                    }
                }
            }
        }
        s
    }

    /// Cast-grain motor: steel tube, 65:35 KNO3/sugar grain (cast), igniter at the free face, nozzle.
    /// `kind` 0 end-burner, 1 core-burner (progressive), 2 end-burner with the nozzle plugged (CATO),
    /// 3 loose (uncast) black-powder-style fill of the same tube.
    fn motor(kind: u32) -> Self {
        let mut s = Scene::empty();
        let cy = (H / 2) as i32;
        let hh = 10; // inner half height
        let (xh, xg, xe) = (96, 102, 172); // head plate, grain start, grain end
        let (xn0, xn1, xt, xx) = (200, 214, 218, 240); // nozzle: start of convergence, throat start, throat end, exit
        let wall = 4;
        for x in xh..=xx {
            let half = if x < xn0 {
                hh
            } else if x < xn1 {
                hh - ((hh - 6) as f32 * (x - xn0) as f32 / (xn1 - xn0) as f32).round() as i32
            } else if x < xt {
                6
            } else {
                6 + ((5.0 * (x - xt) as f32 / (xx - xt) as f32).round() as i32)
            };
            // insulation: phenolic liner in the free chamber, tungsten insert from the convergence on (a bare steel
            // case melts in a 2500 K flow)
            let lin = 2;
            for l in 0..lin {
                let el = if x >= xn0 { 85 } else { 87 };
                s.put(x, cy - half - 1 - l, MATTER, el, 0);
                s.put(x, cy + half + l, MATTER, el, 0);
            }
            for t in 0..wall {
                s.put(x, cy - half - 1 - lin - t, WALL, STEEL, 0);
                s.put(x, cy + half + lin + t, WALL, STEEL, 0);
            }
            if kind == 2 && x >= xn1 && x < xt + 4 {
                for y in cy - half..cy + half {
                    s.put(x, y, WALL, STEEL, 0);
                }
            }
        }
        // head: steel plate with a phenolic liner on its inner face
        for x in xh..xg {
            for y in cy - hh - wall - 2..cy + hh + wall + 2 {
                let inner = y >= cy - hh && y < cy + hh && x >= xg - 2;
                if inner {
                    s.put(x, y, MATTER, 87, 0);
                } else {
                    s.put(x, y, WALL, STEEL, 0);
                }
            }
        }
        let end = if kind == 1 { xn0 - 6 } else { xe };
        let _ = wall;
        for x in xg..end {
            for y in cy - hh..cy + hh {
                if kind == 1 && (y - cy).abs() < 4 {
                    continue; // core bore
                }
                let h = ((x * 73856093) ^ (y * 19349663)) as u32 % 100;
                let el = if h < 65 { 13 } else { 14 };
                if kind == 3 {
                    s.put(x, y, MATTER, if h < 75 { 13 } else if h < 90 { 11 } else { 12 }, 0);
                } else {
                    s.cast(x, y, el);
                }
            }
        }
        // ignition: a one-shot torch against the free surface (core: inside the bore); a permanent igniter tile would
        // keep the gas at 3000 K for the whole burn
        let (tx, tr) = if kind == 1 { (end - 12, 2) } else { (end + 2, 3) };
        s.ops.push(Op { kind: 5, shape: 0, x0: tx, y0: cy, x1: tx, y1: cy, r: tr, tile: 0, mat: 0, dir: 0, _p: [0; 2] });
        s
    }
    pub fn motor_end() -> Self { Scene::motor(0) }
    pub fn motor_core() -> Self { Scene::motor(1) }
    pub fn motor_blocked() -> Self { Scene::motor(2) }
    pub fn motor_loose() -> Self { Scene::motor(3) }

    fn checker(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, mats: &[u32]) {
        for x in x0..=x1 {
            for y in y0..=y1 {
                let k = ((x * 7 + y * 3) as usize + (x * y) as usize % 5) % mats.len();
                // id 0 in the list leaves a gap of air
                if mats[k] == 0 { self.put(x, y, 0, 0, 0) } else { self.put(x, y, MATTER, mats[k], 0) }
            }
        }
    }

    fn hollow(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, t: i32, tile: u32, mat: u32, open_top: bool) {
        for x in x0..=x1 {
            for y in y0..=y1 {
                let edge = x < x0 + t || x > x1 - t || y > y1 - t || (!open_top && y < y0 + t);
                if edge {
                    self.put(x, y, tile, mat, 0);
                }
            }
        }
    }

    fn torch(&mut self, x: i32, y: i32, r: i32) {
        self.ops.push(Op { kind: 5, shape: 0, x0: x, y0: y, x1: x, y1: y, r, tile: 0, mat: 0, dir: 0, _p: [0; 2] });
    }

    /// Grease fire: a pan of oil on the hob has caught fire; a wax plug in the water
    /// cup above melts in the hot plume and drops water into the burning oil -> steam explosion fireball.
    pub fn kitchen() -> Self {
        let mut s = Scene::empty();
        s.floor();
        let fl = H as i32 - 7;
        // stove body
        s.fill(190, fl - 40, 310, fl, WALL, STEEL);
        // glass-ceramic hob plate; the oil has been left on full power and has just flashed into flame
        s.fill(218, fl - 44, 282, fl - 41, WALL, 5);
        let pb = fl - 46;
        s.fill(222, pb, 278, pb + 1, WALL, STEEL);
        s.fill(222, pb - 7, 223, pb - 1, WALL, STEEL);
        s.fill(277, pb - 7, 278, pb - 1, WALL, STEEL);
        s.fill(224, pb - 4, 276, pb - 1, MATTER, 33); // cooking oil
        for x in [228, 240, 252, 264, 274] {
            s.torch(x, pb - 7, 3);
        }
        // the "helpful" cup of water above the pan, plugged with wax
        let cb = 70;
        s.hollow(238, cb - 26, 262, cb, 2, WALL, STEEL, true);
        s.fill(247, cb - 1, 253, cb, MATTER, 35); // wax plug in the bottom
        s.fill(240, cb - 20, 260, cb - 2, MATTER, 19);
        s
    }

    /// Alkali metals in water: sodium fizzes and skates around, potassium bursts into lilac flame.
    pub fn sodium() -> Self {
        let mut s = Scene::empty();
        s.floor();
        let fl = H as i32 - 7;
        // glass tank
        s.hollow(110, fl - 70, 400, fl, 3, MATTER, 53, true);
        s.fill(113, fl - 45, 397, fl - 3, MATTER, 19);
        // lumps dropped from above
        for (k, &(x, m)) in [(150, 41), (210, 41), (270, 42), (330, 42), (370, 41)].iter().enumerate() {
            let y = 40 + (k as i32 % 2) * 12;
            s.fill(x, y, x + 3, y + 3, MATTER, m);
        }
        // a magnesium pile on the side: only reacts once it is burning
        s.fill(430, fl - 6, 450, fl, MATTER, 39);
        s
    }

    /// Fuse race: one fuse lights four piles: black powder, gunpowder premix, nitrocellulose and flash powder.
    pub fn flash() -> Self {
        let mut s = Scene::empty();
        s.floor();
        let fl = H as i32 - 7;
        let piles: [(i32, &[u32]); 4] = [(110, &[13, 13, 13, 11, 12]), (210, &[57]), (310, &[48]), (410, &[47, 47, 15])];
        // fuse along the floor, rising into each pile
        s.fill(40, fl, 440, fl, MATTER, 65);
        for (x, mats) in piles.iter() {
            for k in 0..14 {
                let w = 14 - k;
                s.checker(x - w, fl - 1 - k, x + w, fl - 1 - k, mats);
            }
        }
        s.put(39, fl, IGNITER, 0, 0);
        s
    }

    /// Thermite on a steel plate: a magnesium-ribbon fuse lights it; molten iron burns through the plate.
    pub fn thermite() -> Self {
        let mut s = Scene::empty();
        s.floor();
        let fl = H as i32 - 7;
        // two pedestals and a 4-cell steel plate across them
        s.fill(170, 169, 190, fl, WALL, STEEL);
        s.fill(330, 169, 350, fl, WALL, STEEL);
        s.fill(170, 166, 350, 168, WALL, STEEL);
        // thermite heap: rust + aluminium powder (3:1 by volume ~ stoichiometric by mass)
        for k in 0..16 {
            let w = 30 - k * 3 / 2;
            s.checker(260 - w, 165 - k, 260 + w, 165 - k, &[17, 17, 17, 15, 15]);
        }
        // magnesium ribbon from an igniter up onto the heap
        for k in 0..20 {
            s.put(222 + k, 165 - 2 - k / 3, MATTER, 39, 0);
            s.put(222 + k, 165 - 1 - k / 3, MATTER, 39, 0);
        }
        s.put(221, 164, IGNITER, 0, 0);
        // sand bed below catches the iron
        s.fill(191, fl - 8, 329, fl, MATTER, 10);
        s
    }

    /// Grain-silo dust explosion: a cloud of suspended flour reaches an igniter inside a steel silo with a thin roof.
    pub fn silo() -> Self {
        let mut s = Scene::empty();
        s.floor();
        let fl = H as i32 - 7;
        let (x0, x1, y0) = (170, 340, 50);
        s.hollow(x0, y0, x1, fl, 3, WALL, STEEL, true);
        s.fill(x0, y0, x1, y0 + 1, WALL, 1); // aluminium roof panel (the weak point)
        s.fill(x0 + 3, fl - 12, x1 - 3, fl - 3, MATTER, 64); // flour heap
        // suspended dust: about 1 grain in 20 cells (~ the most explosive concentration)
        for x in (x0 + 5..x1 - 4).step_by(4) {
            for y in (y0 + 6..fl - 14).step_by(5) {
                let j = ((x * 13 + y * 7) % 3) - 1;
                s.put(x + j, y, MATTER, 64, 0);
            }
        }
        s.put(x0 + 4, fl - 60, IGNITER, 0, 0); // a spark from a faulty conveyor motor
        s
    }

    /// Explosives range: dynamite, TNT and C4 lit by fuses (only dynamite explodes; TNT and C4 just burn),
    /// then a long fuse reaches a blasting cap pressed into C4.
    pub fn explosives() -> Self {
        let mut s = Scene::empty();
        s.floor();
        let fl = H as i32 - 7;
        // titanium blast walls between the bays
        for &x in [125, 250, 375].iter() {
            s.fill(x, fl - 90, x + 7, fl, WALL, 3);
        }
        let charge = |s: &mut Scene, x: i32, m: u32| s.fill(x, fl - 10, x + 14, fl, MATTER, m);
        charge(&mut s, 80, 51);  // dynamite
        charge(&mut s, 200, 49); // TNT
        charge(&mut s, 320, 50); // C4
        charge(&mut s, 450, 50); // C4 ...
        s.fill(448, fl - 3, 449, fl, MATTER, 52); // ... with a blasting cap
        // short fuses for the first three
        for &x in [80, 200, 320].iter() {
            s.fill(x - 30, fl, x - 1, fl, MATTER, 65);
            s.put(x - 31, fl, IGNITER, 0, 0);
        }
        // a long zig-zag fuse for the cap
        let mut path: Vec<(i32, i32)> = Vec::new();
        for k in 0..4 {
            let y = fl - 20 - k * 12;
            for x in 390..440 {
                path.push((if k % 2 == 0 { x } else { 829 - x }, y));
            }
        }
        for &(x, y) in path.iter() {
            s.put(x, y, MATTER, 65, 0);
        }
        for k in 0..3 {
            let x = if k % 2 == 0 { 439 } else { 390 };
            for y in fl - 20 - (k + 1) * 12..fl - 20 - k * 12 {
                s.put(x, y, MATTER, 65, 0);
            }
        }
        s.fill(439, fl - 19, 439, fl - 1, MATTER, 65);
        s.fill(440, fl, 447, fl, MATTER, 65);
        // shelves under the zig-zag fuse
        for k in 0..4 {
            let y = fl - 19 - k * 12;
            s.fill(392, y, 437, y, WALL, STEEL);
        }
        s.put(390, fl - 57, IGNITER, 0, 0);
        s
    }

    /// Hydrogen-peroxide monopropellant rocket: HTP drips onto a manganese dioxide catalyst bed,
    /// decomposes into ~1000 K steam and oxygen and blasts out of the nozzle (pointing down).
    pub fn peroxide() -> Self {
        let mut s = Scene::empty();
        let (cx, half) = (256, 22);
        // tank
        s.hollow(cx - half - 3, 20, cx + half + 3, 60, 3, WALL, STEEL, false);
        s.fill(cx - half, 23, cx + half, 54, MATTER, 45);
        // drip hole into the chamber
        for x in cx - 1..=cx {
            for y in 58..=60 {
                s.put(x, y, 0, 0, 0);
            }
        }
        // chamber
        s.hollow(cx - half - 5, 58, cx + half + 5, 140, 5, WALL, 3, true);
        for x in cx - 1..=cx {
            s.put(x, 58, 0, 0, 0);
            s.put(x, 59, 0, 0, 0);
            s.put(x, 60, 0, 0, 0);
        }
        // catalyst tray on a shelf (gaps at both ends let gas through)
        s.fill(cx - 14, 100, cx + 14, 101, WALL, STEEL);
        s.fill(cx - 14, 95, cx - 13, 99, WALL, STEEL);
        s.fill(cx + 13, 95, cx + 14, 99, WALL, STEEL);
        s.fill(cx - 12, 97, cx + 12, 99, MATTER, 46);
        // converging-diverging nozzle below the chamber
        for y in 138..190 {
            let h = if y < 158 { half - (y - 138) * 14 / 20 } else { 8 + (y - 158) / 3 };
            for t in 0..4 {
                s.put(cx - h - 1 - t, y, WALL, INCONEL, 0);
                s.put(cx + h + t, y, WALL, INCONEL, 0);
            }
        }
        s
    }

    /// Volcano: lava pours down a basalt slope onto snow, ice and a lake; steam explosions and new rock.
    pub fn volcano() -> Self {
        let mut s = Scene::empty();
        s.floor();
        let fl = H as i32 - 7;
        // mountain of stone with a crater
        for x in 40..330i32 {
            let hgt = (170 - ((x - 150).abs() as f32 * 1.05) as i32).max(0);
            for y in fl - hgt..=fl {
                s.put(x, y, MATTER, 56, 0);
            }
        }
        s.fill(135, fl - 175, 165, fl - 150, MATTER, 55); // lava lake in the crater
        for x in 140..160 {
            for y in fl - 150..fl - 120 {
                s.put(x, y, MATTER, 55, 0);
            }
        }
        // snow and ice on the right flank
        for x in 170..300 {
            let top = fl - (170 - ((x - 150) as f32 * 1.05) as i32).max(0);
            s.fill(x, top - 3, x, top - 1, MATTER, 27);
        }
        // the lake
        s.fill(330, fl - 3, 340, fl, MATTER, 56);
        s.fill(330, fl - 20, 505, fl, MATTER, 19);
        s.fill(330, fl - 22, 505, fl - 21, MATTER, 26);
        // a wooden hut on the shore
        s.hollow(470, fl - 60, 500, fl - 23, 2, WALL, WOOD, false);
        s
    }

    /// Cryogenics: liquid nitrogen poured onto a pond freezes it; dry ice smothers a kerosene fire.
    pub fn cryo() -> Self {
        let mut s = Scene::empty();
        s.floor();
        let fl = H as i32 - 7;
        // pond in a steel tub, LN2 dewar above with an open bottom
        s.hollow(60, fl - 30, 220, fl, 3, WALL, STEEL, true);
        s.fill(63, fl - 22, 217, fl - 3, MATTER, 19);
        s.hollow(120, 60, 160, 110, 3, WALL, STEEL, true);
        s.fill(123, 64, 157, 107, MATTER, 30);
        s.fill(136, 108, 144, 110, 0, 0); // hole in the dewar
        // kerosene fire in a tray, dry ice on a shelf above it, tipped in by a wax latch
        s.hollow(300, fl - 14, 420, fl, 3, WALL, STEEL, true);
        s.fill(303, fl - 7, 417, fl - 3, MATTER, 20);
        s.hollow(320, 70, 400, 100, 3, WALL, STEEL, true);
        s.fill(323, 76, 397, 97, MATTER, 58);
        s.fill(340, 98, 380, 100, MATTER, 35);
        for x in [305, 330, 360, 390, 415] {
            s.torch(x, fl - 9, 4);
        }
        s.put(304, fl - 8, IGNITER, 0, 0);
        s.put(416, fl - 8, IGNITER, 0, 0);
        s
    }

    /// Wet chemistry bench in glass beakers: magnesium in acid, acid + lye, elephant toothpaste
    /// (peroxide + MnO2), permanganate + glycerin (fire by itself after a few seconds), salt + water.
    pub fn acids() -> Self {
        let mut s = Scene::empty();
        s.floor();
        let fl = H as i32 - 7;
        let beaker = |s: &mut Scene, x: i32, w: i32, hgt: i32| {
            s.hollow(x, fl - hgt, x + w, fl, 2, MATTER, 53, true);
        };
        // 1: hydrochloric acid with magnesium dropped in; igniter above to pop the hydrogen
        beaker(&mut s, 30, 60, 50);
        s.fill(32, fl - 30, 88, fl - 2, MATTER, 44);
        s.fill(50, fl - 80, 66, fl - 76, MATTER, 39);
        s.put(58, fl - 95, IGNITER, 0, 0);
        // 2: acid poured onto lye: neutralisation makes hot salt water
        beaker(&mut s, 120, 60, 50);
        s.fill(122, fl - 16, 178, fl - 2, MATTER, 43);
        s.fill(130, fl - 70, 170, fl - 60, MATTER, 44);
        // 3: elephant toothpaste: 90 % peroxide onto manganese dioxide
        beaker(&mut s, 210, 60, 50);
        s.fill(212, fl - 5, 268, fl - 2, MATTER, 46);
        s.fill(220, fl - 90, 260, fl - 70, MATTER, 45);
        // 4: permanganate heap + glycerin
        s.fill(310, fl - 5, 350, fl, MATTER, 60);
        s.fill(322, fl - 30, 338, fl - 26, MATTER, 61);
        // 5: salt into water
        beaker(&mut s, 390, 80, 50);
        s.fill(392, fl - 30, 468, fl - 2, MATTER, 19);
        s.fill(410, fl - 70, 450, fl - 62, MATTER, 28);
        s
    }

    /// Flash-point ladder: six fuels in steel cups on a plate heated from below, a pilot flame above each rim.
    /// Volatile fuels (gasoline 230 K flash point, ethanol 286 K) light at once; kerosene, diesel,
    /// glycerin and cooking oil must first be heated towards their boiling range by the plate.
    pub fn fuels() -> Self {
        let mut s = Scene::empty();
        s.floor();
        let fl = H as i32 - 7;
        let plate = fl - 30;
        s.fill(20, plate, 491, plate + 3, WALL, STEEL);
        for x in (24..488).step_by(6) {
            s.put(x, plate + 4, IGNITER, 0, 0);
        }
        let fuels = [31u32, 21, 20, 32, 61, 33];
        for (k, f) in fuels.iter().enumerate() {
            let x = 40 + k as i32 * 75;
            s.hollow(x, plate - 30, x + 44, plate - 1, 2, WALL, STEEL, true);
            s.fill(x + 2, plate - 16, x + 42, plate - 3, MATTER, *f);
            s.put(x + 22, plate - 21, IGNITER, 0, 0);
        }
        s
    }

    /// Oxidiser comparison: sugar with five oxidisers (65:35 by volume), lit by one fuse.
    /// KNO3 (rocket candy), KClO4, ammonium perchlorate, KMnO4 and ammonium nitrate.
    pub fn oxidisers() -> Self {
        let mut s = Scene::empty();
        s.floor();
        let fl = H as i32 - 7;
        s.fill(30, fl, 480, fl, MATTER, 65);
        for (k, ox) in [13u32, 47, 16, 60, 18].iter().enumerate() {
            let x = 70 + k as i32 * 90;
            for j in 0..12 {
                let w = 13 - j;
                s.checker(x - w, fl - 1 - j, x + w, fl - 1 - j, &[*ox, *ox, 14]);
            }
        }
        s.put(29, fl, IGNITER, 0, 0);
        s
    }
}

/// Showcase scenes: (bench name, title, description, constructor). The UI can list these.
pub const SHOWCASE: [(&str, &str, &str, fn() -> Scene); 12] = [
    ("kitchen", "Grease fire", "A pan of oil has caught fire; the heat melts a wax plug and the water above drops in. Never throw water on a grease fire!", Scene::kitchen),
    ("sodium", "Alkali metals in water", "Sodium fizzes out hydrogen and skates on the surface; potassium bursts into flame", Scene::sodium),
    ("flash", "Fuse race", "One fuse, four piles: black powder, gunpowder, nitrocellulose and flash powder", Scene::flash),
    ("thermite", "Thermite vs steel", "A magnesium ribbon lights a thermite heap; molten iron burns through the steel plate", Scene::thermite),
    ("silo", "Silo dust explosion", "Suspended flour dust reaches a spark inside a grain silo", Scene::silo),
    ("explosives", "Explosives range", "TNT, C4 and dynamite lit by fuses - and C4 with a blasting cap", Scene::explosives),
    ("peroxide", "Peroxide rocket", "90% hydrogen peroxide drips onto a manganese dioxide catalyst: steam + oxygen thrust", Scene::peroxide),
    ("volcano", "Volcano", "Lava flows over snow into a lake: steam explosions, new rock, a burning hut", Scene::volcano),
    ("cryo", "Cryogenics", "Liquid nitrogen freezes a pond; dry ice smothers a kerosene fire", Scene::cryo),
    ("fuels", "Flash-point ladder", "Gasoline, ethanol, kerosene, diesel, glycerin and cooking oil on a hot plate under pilot flames: volatility decides who lights first", Scene::fuels),
    ("oxidisers", "Oxidiser comparison", "Sugar with KNO3, KClO4, ammonium perchlorate, KMnO4 and ammonium nitrate, lit by one fuse", Scene::oxidisers),
    ("acids", "Wet chemistry", "Magnesium in acid, neutralisation, elephant toothpaste, permanganate + glycerin, brine", Scene::acids),
];

/// Look-dev and cast-propellant test scenes (materials team): bench / SHOT_SCENE names.
pub const MATERIAL_SCENES: [(&str, &str, &str, fn() -> Scene); 5] = [
    ("material_board", "Material board", "Every powder, liquid and placeable solid in its own tray, labelled by element id", Scene::material_board),
    ("motor_end", "End-burning motor", "Cast KNO3/sugar end-burner in a steel tube with a nozzle: steady pressure for seconds", Scene::motor_end),
    ("motor_core", "Core-burning motor", "Cast grain with a central bore: burning surface grows, pressure rises progressively", Scene::motor_core),
    ("motor_cato", "Blocked nozzle (CATO)", "End-burner with the nozzle plugged: pressure climbs until the grain cracks or the case fails", Scene::motor_blocked),
    ("motor_loose", "Loose powder in a tube", "The same tube filled with loose black-powder-style mix: it deflagrates through its pores", Scene::motor_loose),
];
