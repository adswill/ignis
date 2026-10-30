//! Ignis design library: saved creations in `~/Library/Application Support/Ignis/designs/`.
//!
//! Each design is a `<id>.cmb` file plus a `<id>.png` thumbnail next to it.
//!
//! `.cmb` layout (all little endian):
//! ```text
//! b"CMB1" | W u32 | H u32 | p_fuel p_o2 p_air fan_pow (4 x f32) | W*H x u32 meta
//! [optional metadata chunk, ignored by old readers]
//! b"IGMD" | version u32 | payload_len u32 | payload | crc32(payload) u32
//!   payload v1: str name | str author | str description | u32 ntags, ntags x str | u64 created (unix s)
//!               | u64 modified (unix s) | u32 nops, nops x 12 u32 (raw `gpu::Op` words) | str source
//!   str = u32 byte length + UTF-8 bytes. Newer versions may append fields; readers stop where they understand.
//! ```
//! The module is self-contained (std + egui + rfd only) so it can be compiled from an example and dropped
//! into the crate with a single `mod library;` line. Colours come from a [`Palette`] that the app fills
//! from `elements.rs` (see [`Palette::set_wall`] / [`Palette::set_matter`] / [`Palette::set_tile`]).
#![allow(dead_code)]

use std::collections::HashMap;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Grid size of the current game (must match `gpu::W/H`; the format itself stores any size).
pub const GRID_W: u32 = 512;
pub const GRID_H: u32 = 288;
/// Thumbnail size written next to every design.
pub const THUMB_W: u32 = 256;
pub const THUMB_H: u32 = 144;

const MAGIC: &[u8; 4] = b"CMB1";
const CHUNK: &[u8; 4] = b"IGMD";
const CHUNK_VERSION: u32 = 1;
const HEADER: usize = 4 + 8 + 16;

// ============================================================================================
// Data types
// ============================================================================================

/// Human metadata stored in the optional trailing chunk.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DesignInfo {
    pub name: String,
    pub author: String,
    pub description: String,
    pub tags: Vec<String>,
    /// unix seconds
    pub created: u64,
    /// unix seconds
    pub modified: u64,
    /// where it came from: "user", "preset", "showcase", "import", ...
    pub source: String,
}

/// A whole design: the tile grid plus the feed parameters, metadata and start-up edits.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Design {
    pub w: u32,
    pub h: u32,
    /// p_fuel, p_o2, p_air, fan_pow (same order as app.rs `save()`)
    pub params: [f32; 4],
    /// W*H meta words: bits 0-7 tile, 8-15 material/element, 16-23 dir
    pub meta: Vec<u32>,
    pub info: DesignInfo,
    /// raw `gpu::Op` words (12 x u32 = 48 B each) applied right after loading, e.g. pre-filled gas or a torch
    pub ops: Vec<[u32; 12]>,
}

impl Design {
    pub fn new(w: u32, h: u32, meta: Vec<u32>, params: [f32; 4], info: DesignInfo) -> Self {
        Design { w, h, params, meta, info, ops: vec![] }
    }
}

/// One library entry as listed (the grid itself is not loaded).
#[derive(Clone, Debug)]
pub struct Entry {
    pub id: String,
    pub path: PathBuf,
    pub thumb_path: PathBuf,
    pub w: u32,
    pub h: u32,
    pub params: [f32; 4],
    pub info: DesignInfo,
    /// file modification time (unix s), used to invalidate cached thumbnails
    pub file_time: u64,
    pub bytes: u64,
}

/// Plain RGBA8 image.
#[derive(Clone, Debug, PartialEq)]
pub struct RgbaImage {
    pub w: u32,
    pub h: u32,
    pub data: Vec<u8>,
}

impl RgbaImage {
    pub fn new(w: u32, h: u32) -> Self {
        RgbaImage { w, h, data: vec![0; (w * h * 4) as usize] }
    }

    /// Box-filter down/up-scale into a `tw x th` image, letterboxed to keep the aspect ratio.
    pub fn fit(&self, tw: u32, th: u32, bg: [u8; 4]) -> RgbaImage {
        let mut out = RgbaImage::new(tw, th);
        for px in out.data.chunks_mut(4) {
            px.copy_from_slice(&bg);
        }
        if self.w == 0 || self.h == 0 {
            return out;
        }
        let s = (tw as f32 / self.w as f32).min(th as f32 / self.h as f32);
        let (dw, dh) = (((self.w as f32 * s).round() as u32).clamp(1, tw), ((self.h as f32 * s).round() as u32).clamp(1, th));
        let (ox, oy) = ((tw - dw) / 2, (th - dh) / 2);
        for y in 0..dh {
            let sy0 = (y as f32 / s) as u32;
            let sy1 = (((y + 1) as f32 / s) as u32).max(sy0 + 1).min(self.h);
            for x in 0..dw {
                let sx0 = (x as f32 / s) as u32;
                let sx1 = (((x + 1) as f32 / s) as u32).max(sx0 + 1).min(self.w);
                let mut acc = [0u32; 4];
                let mut n = 0u32;
                for sy in sy0.min(self.h - 1)..sy1 {
                    for sx in sx0.min(self.w - 1)..sx1 {
                        let i = ((sy * self.w + sx) * 4) as usize;
                        for c in 0..4 {
                            acc[c] += self.data[i + c] as u32;
                        }
                        n += 1;
                    }
                }
                let o = (((y + oy) * tw + x + ox) * 4) as usize;
                for c in 0..4 {
                    out.data[o + c] = (acc[c] / n.max(1)) as u8;
                }
            }
        }
        out
    }
}

pub fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// "2026-10-28" from unix seconds (UTC).
pub fn format_date(unix: u64) -> String {
    let days = (unix / 86400) as i64;
    // Howard Hinnant's civil_from_days
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// Turn a free-form tag string ("rocket, Solid motor  ,  #apcp") into normalised tags.
pub fn parse_tags(s: &str) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for t in s.split([',', ';', '#']) {
        let t = t.trim().to_lowercase();
        if !t.is_empty() && !v.contains(&t) {
            v.push(t);
        }
    }
    v
}

fn slug(name: &str) -> String {
    let mut s = String::new();
    let mut dash = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            s.push(c.to_ascii_lowercase());
            dash = false;
        } else if !dash && !s.is_empty() {
            s.push('-');
            dash = true;
        }
        if s.len() >= 40 {
            break;
        }
    }
    let s = s.trim_matches('-').to_string();
    if s.is_empty() {
        "design".into()
    } else {
        s
    }
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

// ============================================================================================
// .cmb encode / decode
// ============================================================================================

fn put_u32(b: &mut Vec<u8>, v: u32) {
    b.extend_from_slice(&v.to_le_bytes());
}
fn put_u64(b: &mut Vec<u8>, v: u64) {
    b.extend_from_slice(&v.to_le_bytes());
}
fn put_str(b: &mut Vec<u8>, s: &str) {
    let s = &s.as_bytes()[..s.len().min(1 << 16)];
    put_u32(b, s.len() as u32);
    b.extend_from_slice(s);
}

struct Rd<'a> {
    b: &'a [u8],
    p: usize,
}
impl<'a> Rd<'a> {
    fn bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        let e = self.p.checked_add(n)?;
        let s = self.b.get(self.p..e)?;
        self.p = e;
        Some(s)
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.bytes(4)?.try_into().ok()?))
    }
    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.bytes(8)?.try_into().ok()?))
    }
    fn f32(&mut self) -> Option<f32> {
        Some(f32::from_le_bytes(self.bytes(4)?.try_into().ok()?))
    }
    fn str(&mut self) -> Option<String> {
        let n = self.u32()? as usize;
        Some(String::from_utf8_lossy(self.bytes(n)?).into_owned())
    }
}

fn encode_chunk(info: &DesignInfo, ops: &[[u32; 12]]) -> Vec<u8> {
    let mut p = Vec::new();
    put_str(&mut p, &info.name);
    put_str(&mut p, &info.author);
    put_str(&mut p, &info.description);
    put_u32(&mut p, info.tags.len() as u32);
    for t in &info.tags {
        put_str(&mut p, t);
    }
    put_u64(&mut p, info.created);
    put_u64(&mut p, info.modified);
    put_u32(&mut p, ops.len() as u32);
    for op in ops {
        for w in op {
            put_u32(&mut p, *w);
        }
    }
    put_str(&mut p, &info.source);
    let mut b = CHUNK.to_vec();
    put_u32(&mut b, CHUNK_VERSION);
    put_u32(&mut b, p.len() as u32);
    let crc = crc32(&p);
    b.extend_from_slice(&p);
    put_u32(&mut b, crc);
    b
}

/// Parse a metadata chunk (starting at its magic). Missing trailing fields keep their defaults.
fn decode_chunk(b: &[u8]) -> Option<(DesignInfo, Vec<[u32; 12]>)> {
    let mut r = Rd { b, p: 0 };
    if r.bytes(4)? != CHUNK {
        return None;
    }
    let _ver = r.u32()?;
    let len = r.u32()? as usize;
    let payload = r.bytes(len)?;
    if let Some(crc) = r.u32() {
        if crc != crc32(payload) {
            return None;
        }
    }
    let mut r = Rd { b: payload, p: 0 };
    let mut info = DesignInfo::default();
    let mut ops = Vec::new();
    (|| -> Option<()> {
        info.name = r.str()?;
        info.author = r.str()?;
        info.description = r.str()?;
        let nt = r.u32()?.min(256);
        for _ in 0..nt {
            info.tags.push(r.str()?);
        }
        info.created = r.u64()?;
        info.modified = r.u64()?;
        let no = r.u32()?.min(4096);
        for _ in 0..no {
            let mut op = [0u32; 12];
            for w in op.iter_mut() {
                *w = r.u32()?;
            }
            ops.push(op);
        }
        info.source = r.str()?;
        Some(())
    })();
    Some((info, ops))
}

/// Serialise a design to `.cmb` bytes (old readers see a plain CMB1 file followed by extra bytes).
pub fn encode_cmb(d: &Design) -> Vec<u8> {
    let n = (d.w * d.h) as usize;
    let mut b = Vec::with_capacity(HEADER + n * 4 + 256);
    b.extend_from_slice(MAGIC);
    put_u32(&mut b, d.w);
    put_u32(&mut b, d.h);
    for v in d.params {
        b.extend_from_slice(&v.to_le_bytes());
    }
    for i in 0..n {
        put_u32(&mut b, d.meta.get(i).copied().unwrap_or(0));
    }
    b.extend_from_slice(&encode_chunk(&d.info, &d.ops));
    b
}

/// Parse `.cmb` bytes (with or without the metadata chunk).
pub fn decode_cmb(b: &[u8]) -> Result<Design, String> {
    let mut r = Rd { b, p: 0 };
    if r.bytes(4) != Some(&MAGIC[..]) {
        return Err("not an Ignis/Combustor design (bad magic)".into());
    }
    let (w, h) = (r.u32().ok_or("truncated header")?, r.u32().ok_or("truncated header")?);
    if w == 0 || h == 0 || w > 8192 || h > 8192 {
        return Err(format!("bad grid size {w}x{h}"));
    }
    let mut params = [0f32; 4];
    for p in params.iter_mut() {
        *p = r.f32().ok_or("truncated header")?;
    }
    let n = (w * h) as usize;
    let grid = r.bytes(n * 4).ok_or("file is truncated (grid incomplete)")?;
    let meta = grid.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
    let (info, ops) = decode_chunk(&b[r.p..]).unwrap_or_default();
    Ok(Design { w, h, params, meta, info, ops })
}

/// Read only the header + metadata chunk of a `.cmb` file (skips the grid).
fn peek_cmb(path: &Path) -> io::Result<(u32, u32, [f32; 4], DesignInfo)> {
    let mut f = std::fs::File::open(path)?;
    let mut h = [0u8; HEADER];
    f.read_exact(&mut h)?;
    if &h[0..4] != MAGIC {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "bad magic"));
    }
    let u = |o: usize| u32::from_le_bytes(h[o..o + 4].try_into().unwrap());
    let fl = |o: usize| f32::from_le_bytes(h[o..o + 4].try_into().unwrap());
    let (w, hh) = (u(4), u(8));
    let params = [fl(12), fl(16), fl(20), fl(24)];
    let grid_end = HEADER as u64 + w as u64 * hh as u64 * 4;
    let len = f.metadata()?.len();
    if len < grid_end {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "truncated"));
    }
    let mut info = DesignInfo::default();
    if len > grid_end && len - grid_end < (4 << 20) {
        f.seek(SeekFrom::Start(grid_end))?;
        let mut rest = Vec::new();
        f.read_to_end(&mut rest)?;
        if let Some((i, _)) = decode_chunk(&rest) {
            info = i;
        }
    }
    Ok((w, hh, params, info))
}

// ============================================================================================
// Palette + CPU thumbnail renderer
// ============================================================================================

/// Colours used by the CPU thumbnail renderer. `Default` holds the colours from `elements.rs` at the
/// time of writing; the app should refresh them from `elements::MATS` / `elements::all()` so new
/// elements get their real colour (see `ARCHITECTURE` integration notes in the report).
#[derive(Clone, Debug)]
pub struct Palette {
    /// wall (tile 1) colour per material id
    pub walls: [[u8; 3]; 256],
    /// matter (tile 8) colour per element id
    pub matter: [[u8; 3]; 256],
    /// colour per non-wall/non-matter tile id (fan, igniter, inlets, vent, ...)
    pub tiles: [[u8; 3]; 256],
    /// which matter ids are liquids (smooth shading instead of grain)
    pub liquid: [bool; 256],
    pub sky_top: [u8; 3],
    pub sky_bottom: [u8; 3],
}

fn fallback_rgb(id: u32) -> [u8; 3] {
    let mut h = id.wrapping_mul(0x9E37_79B9) ^ 0x5bd1_e995;
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    [90 + (h & 0x7f) as u8, 90 + ((h >> 8) & 0x7f) as u8, 90 + ((h >> 16) & 0x7f) as u8]
}

impl Default for Palette {
    fn default() -> Self {
        let mut p = Palette {
            walls: [[0; 3]; 256],
            matter: [[0; 3]; 256],
            tiles: [[0; 3]; 256],
            liquid: [false; 256],
            sky_top: [16, 19, 27],
            sky_bottom: [30, 33, 42],
        };
        for i in 0..256u32 {
            p.walls[i as usize] = fallback_rgb(i);
            p.matter[i as usize] = fallback_rgb(i + 1000);
            p.tiles[i as usize] = fallback_rgb(i + 2000);
        }
        let walls: [[u8; 3]; 10] = [
            [120, 125, 135],
            [194, 199, 209],
            [191, 110, 69],
            [148, 153, 173],
            [92, 110, 120],
            [224, 214, 194],
            [158, 102, 77],
            [115, 77, 41],
            [178, 158, 153],
            [217, 140, 64],
        ];
        for (i, c) in walls.iter().enumerate() {
            p.walls[i] = *c;
        }
        let matter: [(usize, [u8; 3], bool); 16] = [
            (10, [204, 179, 115], false),
            (11, [33, 31, 28], false),
            (12, [230, 214, 51], false),
            (13, [237, 237, 242], false),
            (14, [245, 237, 219], false),
            (15, [191, 196, 204], false),
            (16, [214, 230, 250], false),
            (17, [143, 51, 31], false),
            (18, [247, 245, 230], false),
            (19, [38, 89, 217], true),
            (20, [194, 140, 51], true),
            (21, [204, 219, 235], true),
            (22, [140, 191, 255], true),
            (23, [242, 230, 153], true),
            (24, [255, 115, 26], true),
            (25, [115, 112, 107], false),
        ];
        for (i, c, l) in matter {
            p.matter[i] = c;
            p.liquid[i] = l;
        }
        // tiles: 2 fuel in, 3 O2 in, 4 air in, 5 fan, 6 igniter, 7 vent
        p.tiles[2] = [115, 205, 40];
        p.tiles[3] = [50, 115, 255];
        p.tiles[4] = [150, 205, 230];
        p.tiles[5] = [240, 140, 40];
        p.tiles[6] = [230, 70, 40];
        p.tiles[7] = [110, 70, 140];
        p
    }
}

impl Palette {
    pub fn set_wall(&mut self, mat: u32, rgb: [u8; 3]) {
        self.walls[(mat & 255) as usize] = rgb;
    }
    pub fn set_matter(&mut self, id: u32, rgb: [u8; 3], liquid: bool) {
        self.matter[(id & 255) as usize] = rgb;
        self.liquid[(id & 255) as usize] = liquid;
    }
    pub fn set_tile(&mut self, tile: u32, rgb: [u8; 3]) {
        self.tiles[(tile & 255) as usize] = rgb;
    }
}

fn hash2(x: u32, y: u32) -> u32 {
    let mut h = x.wrapping_mul(374_761_393).wrapping_add(y.wrapping_mul(668_265_263));
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    h ^ (h >> 16)
}

fn shade(c: [u8; 3], f: f32) -> [f32; 3] {
    [c[0] as f32 * f, c[1] as f32 * f, c[2] as f32 * f]
}

/// Full-resolution (1 px per cell) render of a meta grid.
pub fn render_cells(w: u32, h: u32, meta: &[u32], pal: &Palette) -> RgbaImage {
    let mut img = RgbaImage::new(w, h);
    let at = |x: i32, y: i32| -> u32 {
        if x < 0 || y < 0 || x >= w as i32 || y >= h as i32 {
            0
        } else {
            meta.get((y as u32 * w + x as u32) as usize).copied().unwrap_or(0)
        }
    };
    let solid = |m: u32| matches!(m & 255, 1 | 6 | 8);
    for y in 0..h {
        let t = y as f32 / (h.max(2) - 1) as f32;
        let sky = [
            pal.sky_top[0] as f32 + (pal.sky_bottom[0] as f32 - pal.sky_top[0] as f32) * t,
            pal.sky_top[1] as f32 + (pal.sky_bottom[1] as f32 - pal.sky_top[1] as f32) * t,
            pal.sky_top[2] as f32 + (pal.sky_bottom[2] as f32 - pal.sky_top[2] as f32) * t,
        ];
        for x in 0..w {
            let m = at(x as i32, y as i32);
            let tile = m & 255;
            let mat = (m >> 8) & 255;
            let n = hash2(x, y);
            let jitter = ((n & 255) as f32 / 255.0 - 0.5) * 2.0;
            let up = at(x as i32, y as i32 - 1);
            let down = at(x as i32, y as i32 + 1);
            let c: [f32; 3] = match tile {
                0 => {
                    // faint grid every 16 cells, shadow under solids
                    let mut c = sky;
                    if x % 16 == 0 || y % 16 == 0 {
                        c = [c[0] + 5.0, c[1] + 5.0, c[2] + 6.0];
                    }
                    if solid(up) {
                        c = [c[0] * 0.7, c[1] * 0.7, c[2] * 0.7];
                    }
                    c
                }
                1 => {
                    // walls: brushed texture, lit top edge, dark bottom edge
                    let base = pal.walls[mat as usize];
                    let streak = ((hash2(y, 7) & 255) as f32 / 255.0 - 0.5) * 0.06;
                    let mut f = 1.0 + jitter * 0.04 + streak;
                    if !solid(up) {
                        f += 0.22;
                    } else if !solid(down) {
                        f -= 0.25;
                    }
                    shade(base, f)
                }
                8 => {
                    let base = pal.matter[mat as usize];
                    if pal.liquid[mat as usize] {
                        let mut f = 0.92 + jitter * 0.03;
                        if up & 255 == 0 {
                            f += 0.3; // bright surface line
                        }
                        let c = shade(base, f);
                        // let the background show a little through liquids
                        [c[0] * 0.85 + sky[0] * 0.15, c[1] * 0.85 + sky[1] * 0.15, c[2] * 0.85 + sky[2] * 0.15]
                    } else {
                        shade(base, 1.0 + jitter * 0.14)
                    }
                }
                5 => {
                    // fan: stripes along its push direction
                    let dir = (m >> 16) & 3;
                    let s = if dir % 2 == 0 { x } else { y };
                    shade(pal.tiles[5], if s % 3 == 0 { 0.65 } else { 1.0 })
                }
                6 => {
                    let glow = if n & 3 == 0 { 1.25 } else { 1.0 };
                    shade(pal.tiles[6], glow)
                }
                t => {
                    let base = pal.tiles[t as usize];
                    let c = shade(base, 0.9 + jitter * 0.05);
                    [c[0] * 0.8 + sky[0] * 0.2, c[1] * 0.8 + sky[1] * 0.2, c[2] * 0.8 + sky[2] * 0.2]
                }
            };
            let o = ((y * w + x) * 4) as usize;
            img.data[o] = c[0].clamp(0.0, 255.0) as u8;
            img.data[o + 1] = c[1].clamp(0.0, 255.0) as u8;
            img.data[o + 2] = c[2].clamp(0.0, 255.0) as u8;
            img.data[o + 3] = 255;
        }
    }
    img
}

/// Thumbnail (THUMB_W x THUMB_H) rendered on the CPU from a meta grid, cropped to the content.
pub fn render_thumb(w: u32, h: u32, meta: &[u32], pal: &Palette) -> RgbaImage {
    render_thumb_ops(w, h, meta, &[], pal)
}

/// Thumbnail of a whole design (grid + start-up ops such as gas fills and torches).
pub fn render_design_thumb(d: &Design, pal: &Palette) -> RgbaImage {
    render_thumb_ops(d.w, d.h, &d.meta, &d.ops, pal)
}

fn op_inside(op: &[u32; 12], x: i32, y: i32) -> bool {
    let (shape, x0, y0, x1, y1, r) = (op[1], op[2] as i32, op[3] as i32, op[4] as i32, op[5] as i32, op[6] as i32);
    match shape {
        0 => (x - x0).pow(2) + (y - y0).pow(2) <= r * r,
        1 | 2 => {
            let (lx, hx, ly, hy) = (x0.min(x1), x0.max(x1), y0.min(y1), y0.max(y1));
            let inb = x >= lx && x <= hx && y >= ly && y <= hy;
            inb && (shape == 1 || x - lx <= r || hx - x <= r || y - ly <= r || hy - y <= r)
        }
        _ => {
            let (dx, dy) = ((x1 - x0) as f32, (y1 - y0) as f32);
            let l2 = (dx * dx + dy * dy).max(1e-6);
            let t = ((((x - x0) as f32) * dx + ((y - y0) as f32) * dy) / l2).clamp(0.0, 1.0);
            let (px, py) = (x0 as f32 + t * dx - x as f32, y0 as f32 + t * dy - y as f32);
            px * px + py * py <= (r as f32 + 0.5).powi(2)
        }
    }
}

fn op_bbox(op: &[u32; 12]) -> (i32, i32, i32, i32) {
    let (x0, y0, x1, y1, r) = (op[2] as i32, op[3] as i32, op[4] as i32, op[5] as i32, op[6] as i32);
    if op[1] == 0 {
        (x0 - r, y0 - r, x0 + r, y0 + r)
    } else {
        (x0.min(x1) - r, y0.min(y1) - r, x0.max(x1) + r, y0.max(y1) + r)
    }
}

/// Paint start-up ops: placed tiles, gas fills as tinted haze, torches as glows.
fn overlay_ops(img: &mut RgbaImage, meta: &[u32], ops: &[[u32; 12]], pal: &Palette) {
    let (w, h) = (img.w as i32, img.h as i32);
    for op in ops {
        let kind = op[0];
        let (col, a): ([u8; 3], f32) = match kind {
            0 => match op[7] {
                1 => (pal.walls[(op[8] & 255) as usize], 1.0),
                8 => (pal.matter[(op[8] & 255) as usize], 1.0),
                t => (pal.tiles[(t & 255) as usize], 1.0),
            },
            2 => ([140, 215, 40], 0.22),
            3 => ([70, 140, 255], 0.22),
            4 => ([130, 160, 190], 0.12),
            5 => ([255, 140, 40], 0.85),
            6 => ([80, 200, 230], 0.2),
            _ => continue,
        };
        let (bx0, by0, bx1, by1) = op_bbox(op);
        let (cx, cy, rr) = (op[2] as i32 as f32, op[3] as i32 as f32, (op[6] as i32).max(1) as f32);
        for y in by0.max(0)..=by1.min(h - 1) {
            for x in bx0.max(0)..=bx1.min(w - 1) {
                if !op_inside(op, x, y) {
                    continue;
                }
                let i = (y * w + x) as usize;
                if kind != 0 && meta.get(i).copied().unwrap_or(0) & 255 != 0 {
                    continue; // gas edits only show in open gas
                }
                let mut a = a;
                if kind == 5 {
                    let d = (((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt() / rr).min(1.0);
                    a *= 1.0 - d * d;
                }
                let o = i * 4;
                for c in 0..3 {
                    let v = img.data[o + c] as f32 * (1.0 - a) + col[c] as f32 * a;
                    img.data[o + c] = v.clamp(0.0, 255.0) as u8;
                }
            }
        }
    }
}

/// Bounding box (x0, y0, x1, y1 inclusive) of everything that is not plain gas, if any.
fn content_bbox(w: u32, h: u32, meta: &[u32], ops: &[[u32; 12]]) -> Option<(i32, i32, i32, i32)> {
    let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    // rows / columns that are almost all solid (floors, frames) don't count as content
    let filled = |cells: &mut dyn Iterator<Item = u32>, n: u32| cells.filter(|m| m & 255 != 0).count() as u32 * 10 > n * 8;
    let row_full: Vec<bool> = (0..h).map(|y| filled(&mut (0..w).map(|x| meta.get((y * w + x) as usize).copied().unwrap_or(0)), w)).collect();
    let col_full: Vec<bool> = (0..w).map(|x| filled(&mut (0..h).map(|y| meta.get((y * w + x) as usize).copied().unwrap_or(0)), h)).collect();
    for y in 0..h as i32 {
        if row_full[y as usize] {
            continue;
        }
        for x in 0..w as i32 {
            if col_full[x as usize] {
                continue;
            }
            if meta.get((y as u32 * w + x as u32) as usize).copied().unwrap_or(0) & 255 != 0 {
                x0 = x0.min(x);
                x1 = x1.max(x);
                y0 = y0.min(y);
                y1 = y1.max(y);
            }
        }
    }
    for op in ops {
        if matches!(op[0], 0 | 2 | 3 | 5) {
            let b = op_bbox(op);
            x0 = x0.min(b.0);
            y0 = y0.min(b.1);
            x1 = x1.max(b.2);
            y1 = y1.max(b.3);
        }
    }
    (x1 >= x0).then_some((x0.max(0), y0.max(0), x1.min(w as i32 - 1), y1.min(h as i32 - 1)))
}

fn render_thumb_ops(w: u32, h: u32, meta: &[u32], ops: &[[u32; 12]], pal: &Palette) -> RgbaImage {
    let mut full = render_cells(w, h, meta, pal);
    overlay_ops(&mut full, meta, ops, pal);
    let bg = [pal.sky_top[0], pal.sky_top[1], pal.sky_top[2], 255];
    let Some((x0, y0, x1, y1)) = content_bbox(w, h, meta, ops) else {
        return full.fit(THUMB_W, THUMB_H, bg);
    };
    // pad 10 %, grow to the thumbnail aspect, never smaller than a quarter of the grid, clamp to the grid
    let aspect = THUMB_W as f32 / THUMB_H as f32;
    let (cx, cy) = ((x0 + x1) as f32 * 0.5, (y0 + y1) as f32 * 0.5);
    let mut cw = ((x1 - x0 + 1) as f32 * 1.2).max(w as f32 * 0.25);
    let mut ch = ((y1 - y0 + 1) as f32 * 1.2).max(h as f32 * 0.25);
    if cw / ch > aspect {
        ch = cw / aspect;
    } else {
        cw = ch * aspect;
    }
    cw = cw.min(w as f32);
    ch = ch.min(h as f32);
    let rx = (cx - cw * 0.5).clamp(0.0, w as f32 - cw).round() as u32;
    let ry = (cy - ch * 0.5).clamp(0.0, h as f32 - ch).round() as u32;
    let (cw, ch) = ((cw.round() as u32).min(w - rx).max(1), (ch.round() as u32).min(h - ry).max(1));
    let mut crop = RgbaImage::new(cw, ch);
    for y in 0..ch {
        let s = (((ry + y) * w + rx) * 4) as usize;
        crop.data[(y * cw * 4) as usize..((y + 1) * cw * 4) as usize].copy_from_slice(&full.data[s..s + (cw * 4) as usize]);
    }
    crop.fit(THUMB_W, THUMB_H, bg)
}

// ============================================================================================
// Tiny PNG codec (RGBA8 / RGB8, 8-bit, non-interlaced) with a fixed-Huffman LZ77 deflater and a
// full inflater. Only std; the crate has no direct png/flate dependency.
// ============================================================================================

const fn crc_table() -> [u32; 256] {
    let mut t = [0u32; 256];
    let mut n = 0;
    while n < 256 {
        let mut c = n as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        t[n] = c;
        n += 1;
    }
    t
}
static CRC_TABLE: [u32; 256] = crc_table();

pub fn crc32(b: &[u8]) -> u32 {
    crc32_cont(0, b)
}
fn crc32_cont(crc: u32, b: &[u8]) -> u32 {
    let mut c = !crc;
    for &x in b {
        c = CRC_TABLE[((c ^ x as u32) & 255) as usize] ^ (c >> 8);
    }
    !c
}

fn adler32(b: &[u8]) -> u32 {
    let (mut a, mut s) = (1u32, 0u32);
    for chunk in b.chunks(5552) {
        for &x in chunk {
            a += x as u32;
            s += a;
        }
        a %= 65521;
        s %= 65521;
    }
    (s << 16) | a
}

const LBASE: [u16; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LEXT: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DBASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DEXT: [u8; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];

struct BitW {
    out: Vec<u8>,
    buf: u64,
    n: u32,
}
impl BitW {
    fn put(&mut self, v: u32, bits: u32) {
        self.buf |= (v as u64) << self.n;
        self.n += bits;
        while self.n >= 8 {
            self.out.push(self.buf as u8);
            self.buf >>= 8;
            self.n -= 8;
        }
    }
    /// Huffman codes are sent MSB first.
    fn code(&mut self, code: u32, len: u32) {
        let mut r = 0;
        for i in 0..len {
            r |= ((code >> i) & 1) << (len - 1 - i);
        }
        self.put(r, len);
    }
    fn flush(mut self) -> Vec<u8> {
        if self.n > 0 {
            self.out.push(self.buf as u8);
        }
        self.out
    }
}

fn fixed_lit(w: &mut BitW, sym: u32) {
    match sym {
        0..=143 => w.code(0x30 + sym, 8),
        144..=255 => w.code(0x190 + sym - 144, 9),
        256..=279 => w.code(sym - 256, 7),
        _ => w.code(0xC0 + sym - 280, 8),
    }
}

/// zlib stream (fixed-Huffman deflate with hash-chain LZ77).
pub fn zlib_compress(data: &[u8]) -> Vec<u8> {
    let mut w = BitW { out: vec![0x78, 0x01], buf: 0, n: 0 };
    w.put(1, 1); // BFINAL
    w.put(1, 2); // fixed Huffman
    const HB: usize = 15;
    let mut head = vec![-1i32; 1 << HB];
    let mut prev = vec![-1i32; data.len()];
    let hash = |i: usize| -> usize {
        let v = (data[i] as u32) | (data[i + 1] as u32) << 8 | (data[i + 2] as u32) << 16;
        (v.wrapping_mul(2_654_435_761) >> (32 - HB)) as usize
    };
    let n = data.len();
    let mut i = 0;
    let insert = |i: usize, head: &mut Vec<i32>, prev: &mut Vec<i32>| {
        if i + 2 < n {
            let h = hash(i);
            prev[i] = head[h];
            head[h] = i as i32;
        }
    };
    while i < n {
        let (mut best, mut bdist) = (0usize, 0usize);
        if i + 2 < n {
            let mut j = head[hash(i)];
            let mut chain = 0;
            let maxl = (n - i).min(258);
            while j >= 0 && i - j as usize <= 32768 && chain < 64 {
                let ju = j as usize;
                if data[ju + best.min(maxl - 1)] == data[i + best.min(maxl - 1)] {
                    let mut l = 0;
                    while l < maxl && data[ju + l] == data[i + l] {
                        l += 1;
                    }
                    if l > best {
                        best = l;
                        bdist = i - ju;
                        if l == maxl {
                            break;
                        }
                    }
                }
                j = prev[ju];
                chain += 1;
            }
        }
        if best >= 3 {
            let li = LBASE.iter().rposition(|&b| b as usize <= best).unwrap();
            fixed_lit(&mut w, 257 + li as u32);
            w.put((best - LBASE[li] as usize) as u32, LEXT[li] as u32);
            let di = DBASE.iter().rposition(|&b| b as usize <= bdist).unwrap();
            w.code(di as u32, 5);
            w.put((bdist - DBASE[di] as usize) as u32, DEXT[di] as u32);
            for k in 0..best {
                insert(i + k, &mut head, &mut prev);
            }
            i += best;
        } else {
            fixed_lit(&mut w, data[i] as u32);
            insert(i, &mut head, &mut prev);
            i += 1;
        }
    }
    fixed_lit(&mut w, 256);
    let mut out = w.flush();
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

struct BitR<'a> {
    b: &'a [u8],
    p: usize,
    buf: u32,
    n: u32,
}
impl<'a> BitR<'a> {
    fn bits(&mut self, need: u32) -> Result<u32, String> {
        while self.n < need {
            let byte = *self.b.get(self.p).ok_or("deflate: out of input")?;
            self.p += 1;
            self.buf |= (byte as u32) << self.n;
            self.n += 8;
        }
        let v = if need == 0 { 0 } else { self.buf & ((1u32 << need) - 1) };
        self.buf = if need >= 32 { 0 } else { self.buf >> need };
        self.n -= need;
        Ok(v)
    }
}

struct Huff {
    count: [u16; 16],
    sym: Vec<u16>,
}
impl Huff {
    fn new(lens: &[u8]) -> Huff {
        let mut count = [0u16; 16];
        for &l in lens {
            count[l as usize] += 1;
        }
        count[0] = 0;
        let mut offs = [0u16; 16];
        for i in 1..16 {
            offs[i] = offs[i - 1] + count[i - 1];
        }
        let mut sym = vec![0u16; lens.len()];
        for (s, &l) in lens.iter().enumerate() {
            if l != 0 {
                sym[offs[l as usize] as usize] = s as u16;
                offs[l as usize] += 1;
            }
        }
        Huff { count, sym }
    }
    fn decode(&self, r: &mut BitR) -> Result<u16, String> {
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for len in 1..16 {
            code |= r.bits(1)? as i32;
            let c = self.count[len] as i32;
            if code - c < first {
                return self.sym.get((index + code - first) as usize).copied().ok_or_else(|| "deflate: bad code".into());
            }
            index += c;
            first += c;
            first <<= 1;
            code <<= 1;
        }
        Err("deflate: bad code".into())
    }
}

/// Inflate a zlib stream (all block types).
pub fn zlib_decompress(z: &[u8], size_hint: usize) -> Result<Vec<u8>, String> {
    if z.len() < 2 || (z[0] & 15) != 8 || ((z[0] as u16) << 8 | z[1] as u16) % 31 != 0 {
        return Err("zlib: bad header".into());
    }
    let mut r = BitR { b: &z[2..], p: 0, buf: 0, n: 0 };
    let mut out: Vec<u8> = Vec::with_capacity(size_hint);
    loop {
        let last = r.bits(1)?;
        let typ = r.bits(2)?;
        match typ {
            0 => {
                r.buf = 0;
                r.n = 0;
                let s = r.b.get(r.p..r.p + 4).ok_or("deflate: truncated")?;
                let len = u16::from_le_bytes([s[0], s[1]]) as usize;
                r.p += 4;
                out.extend_from_slice(r.b.get(r.p..r.p + len).ok_or("deflate: truncated")?);
                r.p += len;
            }
            1 | 2 => {
                let (lit, dist) = if typ == 1 {
                    let mut l = [0u8; 288];
                    for (i, v) in l.iter_mut().enumerate() {
                        *v = match i {
                            0..=143 => 8,
                            144..=255 => 9,
                            256..=279 => 7,
                            _ => 8,
                        };
                    }
                    (Huff::new(&l), Huff::new(&[5u8; 30]))
                } else {
                    let hlit = r.bits(5)? as usize + 257;
                    let hdist = r.bits(5)? as usize + 1;
                    let hclen = r.bits(4)? as usize + 4;
                    const ORD: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
                    let mut cl = [0u8; 19];
                    for &o in ORD.iter().take(hclen) {
                        cl[o] = r.bits(3)? as u8;
                    }
                    let ch = Huff::new(&cl);
                    let mut lens = vec![0u8; hlit + hdist];
                    let mut i = 0;
                    while i < hlit + hdist {
                        let s = ch.decode(&mut r)?;
                        let (v, rep) = match s {
                            0..=15 => (s as u8, 1),
                            16 => (*lens.get(i.wrapping_sub(1)).ok_or("deflate: repeat at start")?, 3 + r.bits(2)? as usize),
                            17 => (0, 3 + r.bits(3)? as usize),
                            _ => (0, 11 + r.bits(7)? as usize),
                        };
                        if i + rep > hlit + hdist {
                            return Err("deflate: too many lengths".into());
                        }
                        for _ in 0..rep {
                            lens[i] = v;
                            i += 1;
                        }
                    }
                    (Huff::new(&lens[..hlit]), Huff::new(&lens[hlit..]))
                };
                loop {
                    let s = lit.decode(&mut r)? as usize;
                    if s < 256 {
                        out.push(s as u8);
                    } else if s == 256 {
                        break;
                    } else {
                        let k = s - 257;
                        if k >= 29 {
                            return Err("deflate: bad length".into());
                        }
                        let len = LBASE[k] as usize + r.bits(LEXT[k] as u32)? as usize;
                        let d = dist.decode(&mut r)? as usize;
                        if d >= 30 {
                            return Err("deflate: bad distance".into());
                        }
                        let dd = DBASE[d] as usize + r.bits(DEXT[d] as u32)? as usize;
                        if dd > out.len() {
                            return Err("deflate: distance too far".into());
                        }
                        let st = out.len() - dd;
                        for j in 0..len {
                            let b = out[st + j];
                            out.push(b);
                        }
                    }
                    if out.len() > (64 << 20) {
                        return Err("deflate: output too large".into());
                    }
                }
            }
            _ => return Err("deflate: bad block type".into()),
        }
        if last == 1 {
            break;
        }
    }
    Ok(out)
}

fn png_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32_cont(crc32(kind), data);
    out.extend_from_slice(&crc.to_be_bytes());
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = a as i16 + b as i16 - c as i16;
    let (pa, pb, pc) = ((p - a as i16).abs(), (p - b as i16).abs(), (p - c as i16).abs());
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// Encode an RGBA8 image as PNG (adaptive per-row filters).
pub fn encode_png(img: &RgbaImage) -> Vec<u8> {
    let (w, h) = (img.w as usize, img.h as usize);
    let stride = w * 4;
    let mut raw = Vec::with_capacity((stride + 1) * h);
    let zero = vec![0u8; stride];
    let mut cand = vec![0u8; stride];
    let mut best = vec![0u8; stride];
    for y in 0..h {
        let row = &img.data[y * stride..(y + 1) * stride];
        let up = if y > 0 { &img.data[(y - 1) * stride..y * stride] } else { &zero[..] };
        let mut best_f = 0u8;
        let mut best_score = u64::MAX;
        for f in 0..5u8 {
            let mut score = 0u64;
            for i in 0..stride {
                let a = if i >= 4 { row[i - 4] } else { 0 };
                let b = up[i];
                let c = if i >= 4 { up[i - 4] } else { 0 };
                let v = match f {
                    0 => row[i],
                    1 => row[i].wrapping_sub(a),
                    2 => row[i].wrapping_sub(b),
                    3 => row[i].wrapping_sub(((a as u16 + b as u16) / 2) as u8),
                    _ => row[i].wrapping_sub(paeth(a, b, c)),
                };
                cand[i] = v;
                score += (v as i8).unsigned_abs() as u64;
            }
            if score < best_score {
                best_score = score;
                best_f = f;
                std::mem::swap(&mut best, &mut cand);
            }
        }
        raw.push(best_f);
        raw.extend_from_slice(&best);
    }
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&img.w.to_be_bytes());
    ihdr.extend_from_slice(&img.h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    png_chunk(&mut out, b"IHDR", &ihdr);
    png_chunk(&mut out, b"IDAT", &zlib_compress(&raw));
    png_chunk(&mut out, b"IEND", &[]);
    out
}

/// Decode an 8-bit RGBA / RGB / grey / grey+alpha non-interlaced PNG into RGBA8.
pub fn decode_png(b: &[u8]) -> Result<RgbaImage, String> {
    if b.len() < 8 || &b[..8] != b"\x89PNG\r\n\x1a\n" {
        return Err("not a PNG".into());
    }
    let mut p = 8;
    let (mut w, mut h, mut ct) = (0u32, 0u32, 0u8);
    let mut idat = Vec::new();
    while p + 8 <= b.len() {
        let len = u32::from_be_bytes(b[p..p + 4].try_into().unwrap()) as usize;
        let kind = &b[p + 4..p + 8];
        let data = b.get(p + 8..p + 8 + len).ok_or("png: truncated chunk")?;
        match kind {
            b"IHDR" => {
                if data.len() < 13 {
                    return Err("png: bad IHDR".into());
                }
                w = u32::from_be_bytes(data[0..4].try_into().unwrap());
                h = u32::from_be_bytes(data[4..8].try_into().unwrap());
                if data[8] != 8 || data[12] != 0 {
                    return Err("png: only 8-bit non-interlaced supported".into());
                }
                ct = data[9];
            }
            b"IDAT" => idat.extend_from_slice(data),
            b"IEND" => break,
            _ => {}
        }
        p += 12 + len;
    }
    let ch = match ct {
        0 => 1,
        2 => 3,
        4 => 2,
        6 => 4,
        _ => return Err("png: unsupported colour type".into()),
    };
    if w == 0 || h == 0 || w > 8192 || h > 8192 {
        return Err("png: bad size".into());
    }
    let stride = w as usize * ch;
    let raw = zlib_decompress(&idat, (stride + 1) * h as usize)?;
    if raw.len() < (stride + 1) * h as usize {
        return Err("png: image data too short".into());
    }
    let mut px = vec![0u8; stride * h as usize];
    for y in 0..h as usize {
        let f = raw[y * (stride + 1)];
        let src = &raw[y * (stride + 1) + 1..(y + 1) * (stride + 1)];
        for i in 0..stride {
            let a = if i >= ch { px[y * stride + i - ch] } else { 0 };
            let bb = if y > 0 { px[(y - 1) * stride + i] } else { 0 };
            let c = if y > 0 && i >= ch { px[(y - 1) * stride + i - ch] } else { 0 };
            px[y * stride + i] = match f {
                0 => src[i],
                1 => src[i].wrapping_add(a),
                2 => src[i].wrapping_add(bb),
                3 => src[i].wrapping_add(((a as u16 + bb as u16) / 2) as u8),
                4 => src[i].wrapping_add(paeth(a, bb, c)),
                _ => return Err("png: bad filter".into()),
            };
        }
    }
    let mut img = RgbaImage::new(w, h);
    for (i, o) in img.data.chunks_mut(4).enumerate() {
        let s = &px[i * ch..i * ch + ch];
        match ch {
            1 => o.copy_from_slice(&[s[0], s[0], s[0], 255]),
            2 => o.copy_from_slice(&[s[0], s[0], s[0], s[1]]),
            3 => o.copy_from_slice(&[s[0], s[1], s[2], 255]),
            _ => o.copy_from_slice(s),
        }
    }
    Ok(img)
}

// ============================================================================================
// Library (directory of designs)
// ============================================================================================

/// Default library folder: `$IGNIS_LIBRARY_DIR` if set, else `~/Library/Application Support/Ignis/designs`.
pub fn library_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("IGNIS_LIBRARY_DIR") {
        return PathBuf::from(d);
    }
    crate::settings::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("designs")
}

fn io_err(s: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, s.into())
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension("tmp~");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

/// A library rooted at a folder. The free functions below use [`Library::default`].
#[derive(Clone, Debug)]
pub struct Library {
    pub dir: PathBuf,
    pub palette: Palette,
}

impl Default for Library {
    fn default() -> Self {
        Library { dir: library_dir(), palette: Palette::default() }
    }
}

impl Library {
    pub fn at(dir: impl Into<PathBuf>) -> Self {
        Library { dir: dir.into(), palette: Palette::default() }
    }

    pub fn cmb_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.cmb"))
    }
    pub fn thumb_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.png"))
    }

    fn check_id(id: &str) -> io::Result<()> {
        if valid_id(id) {
            Ok(())
        } else {
            Err(io_err(format!("invalid design id {id:?}")))
        }
    }

    /// All designs, newest first.
    pub fn list(&self) -> Vec<Entry> {
        let mut v = Vec::new();
        let Ok(rd) = std::fs::read_dir(&self.dir) else { return v };
        for e in rd.flatten() {
            let path = e.path();
            if path.extension().and_then(|x| x.to_str()) != Some("cmb") {
                continue;
            }
            let Some(id) = path.file_stem().and_then(|s| s.to_str()).map(str::to_string) else { continue };
            if !valid_id(&id) {
                continue;
            }
            let Ok((w, h, params, mut info)) = peek_cmb(&path) else { continue };
            let md = e.metadata().ok();
            let file_time = md.as_ref().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
            if info.name.is_empty() {
                info.name = id.replace(['-', '_'], " ");
            }
            if info.modified == 0 {
                info.modified = file_time;
            }
            if info.created == 0 {
                info.created = info.modified;
            }
            v.push(Entry { thumb_path: self.thumb_path(&id), id, path, w, h, params, info, file_time, bytes: md.map(|m| m.len()).unwrap_or(0) });
        }
        v.sort_by(|a, b| b.info.modified.cmp(&a.info.modified).then_with(|| a.info.name.cmp(&b.info.name)));
        v
    }

    pub fn exists(&self, id: &str) -> bool {
        self.cmb_path(id).exists()
    }

    /// A free id derived from `name` ("Rocket engine" -> "rocket-engine", "rocket-engine-2", ...).
    pub fn unique_id(&self, name: &str) -> String {
        let base = slug(name);
        if !self.exists(&base) {
            return base;
        }
        (2..).map(|i| format!("{base}-{i}")).find(|id| !self.exists(id)).unwrap()
    }

    /// Write a design under `id` (overwrites) plus its thumbnail. `thumb`: an optional picture (e.g. a
    /// screenshot of the running sim); it is fitted to THUMB_W x THUMB_H. `None` renders one from the grid.
    pub fn write(&self, id: &str, d: &Design, thumb: Option<&RgbaImage>) -> io::Result<()> {
        Self::check_id(id)?;
        if d.meta.len() != (d.w * d.h) as usize {
            return Err(io_err(format!("meta has {} cells, expected {}", d.meta.len(), d.w * d.h)));
        }
        std::fs::create_dir_all(&self.dir)?;
        write_atomic(&self.cmb_path(id), &encode_cmb(d))?;
        let pal = &self.palette;
        let t = match thumb {
            Some(img) => img.fit(THUMB_W, THUMB_H, [pal.sky_top[0], pal.sky_top[1], pal.sky_top[2], 255]),
            None => render_design_thumb(d, pal),
        };
        write_atomic(&self.thumb_path(id), &encode_png(&t))
    }

    /// Save a new design (fresh id from the name); returns the id.
    pub fn save_design(&self, mut d: Design, thumb: Option<&RgbaImage>) -> io::Result<String> {
        let now = now_unix();
        if d.info.created == 0 {
            d.info.created = now;
        }
        d.info.modified = now;
        if d.info.name.trim().is_empty() {
            d.info.name = "Untitled design".into();
        }
        if d.info.source.is_empty() {
            d.info.source = "user".into();
        }
        let id = self.unique_id(&d.info.name);
        self.write(&id, &d, thumb)?;
        Ok(id)
    }

    /// Brief-style API: save `meta` (GRID_W x GRID_H) with `params` = [p_fuel, p_o2, p_air, fan_pow].
    /// `thumb_rgba`: optional (width, height, RGBA8 bytes) picture.
    pub fn save(&self, name: &str, meta: &[u32], params: [f32; 4], thumb_rgba: Option<(u32, u32, &[u8])>) -> io::Result<String> {
        let (w, h) = grid_dims(meta.len());
        let info = DesignInfo { name: name.to_string(), author: default_author(), ..Default::default() };
        let thumb = thumb_rgba.filter(|(tw, th, b)| b.len() >= (*tw * *th * 4) as usize).map(|(tw, th, b)| RgbaImage { w: tw, h: th, data: b[..(tw * th * 4) as usize].to_vec() });
        self.save_design(Design::new(w, h, meta.to_vec(), params, info), thumb.as_ref())
    }

    /// Overwrite an existing design's grid (keeps its created date, name etc. unless `info` is given).
    pub fn overwrite(&self, id: &str, meta: &[u32], params: [f32; 4], info: Option<DesignInfo>, thumb: Option<&RgbaImage>) -> io::Result<()> {
        let old = self.load(id)?;
        let (w, h) = grid_dims(meta.len());
        let mut info = info.unwrap_or(old.info);
        info.modified = now_unix();
        let d = Design { w, h, params, meta: meta.to_vec(), info, ops: old.ops };
        self.write(id, &d, thumb)
    }

    pub fn load(&self, id: &str) -> io::Result<Design> {
        Self::check_id(id)?;
        let b = std::fs::read(self.cmb_path(id))?;
        let mut d = decode_cmb(&b).map_err(io_err)?;
        if d.info.name.is_empty() {
            d.info.name = id.replace(['-', '_'], " ");
        }
        Ok(d)
    }

    /// Change only the metadata (name / author / description / tags) of a design.
    pub fn update_info(&self, id: &str, mut info: DesignInfo) -> io::Result<()> {
        let mut d = self.load(id)?;
        info.created = d.info.created;
        info.modified = now_unix();
        if info.source.is_empty() {
            info.source = d.info.source.clone();
        }
        d.info = info;
        Self::check_id(id)?;
        write_atomic(&self.cmb_path(id), &encode_cmb(&d))
    }

    pub fn duplicate(&self, id: &str) -> io::Result<String> {
        let mut d = self.load(id)?;
        d.info.name = format!("{} (copy)", d.info.name);
        d.info.created = 0;
        let thumb = std::fs::read(self.thumb_path(id)).ok().and_then(|b| decode_png(&b).ok());
        self.save_design(d, thumb.as_ref())
    }

    /// Move a design (and thumbnail) into `<dir>/.trash/` (recoverable, not destroyed).
    pub fn delete(&self, id: &str) -> io::Result<()> {
        Self::check_id(id)?;
        let trash = self.dir.join(".trash");
        std::fs::create_dir_all(&trash)?;
        let stamp = now_unix();
        std::fs::rename(self.cmb_path(id), trash.join(format!("{id}-{stamp}.cmb")))?;
        let t = self.thumb_path(id);
        if t.exists() {
            let _ = std::fs::rename(t, trash.join(format!("{id}-{stamp}.png")));
        }
        Ok(())
    }

    /// Copy a `.cmb` file from anywhere into the library; returns the new id. A `.png` next to it with the
    /// same stem is used as the thumbnail if it decodes.
    pub fn import(&self, path: &Path) -> io::Result<String> {
        let b = std::fs::read(path)?;
        let mut d = decode_cmb(&b).map_err(io_err)?;
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("imported").to_string();
        if d.info.name.trim().is_empty() {
            d.info.name = stem.replace(['-', '_'], " ");
        }
        if d.info.source.is_empty() {
            d.info.source = "import".into();
        }
        let thumb = std::fs::read(path.with_extension("png")).ok().and_then(|b| decode_png(&b).ok());
        let created = d.info.created;
        let id = self.save_design(d, thumb.as_ref())?;
        if created != 0 {
            // keep the original creation date
            let mut d = self.load(&id)?;
            d.info.created = created;
            write_atomic(&self.cmb_path(&id), &encode_cmb(&d))?;
        }
        Ok(id)
    }

    /// Copy a design out to `path` (`.cmb`); a `.png` path writes the thumbnail instead.
    pub fn export(&self, id: &str, path: &Path) -> io::Result<()> {
        Self::check_id(id)?;
        if path.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("png")) == Some(true) {
            let t = self.thumbnail(id)?;
            return std::fs::write(path, encode_png(&t));
        }
        std::fs::copy(self.cmb_path(id), path).map(|_| ())
    }

    /// Thumbnail for a design: the stored PNG, or (if missing/corrupt) rendered from the grid and cached.
    pub fn thumbnail(&self, id: &str) -> io::Result<RgbaImage> {
        if let Some(img) = std::fs::read(self.thumb_path(id)).ok().and_then(|b| decode_png(&b).ok()) {
            return Ok(img);
        }
        let d = self.load(id)?;
        let img = render_design_thumb(&d, &self.palette);
        let _ = write_atomic(&self.thumb_path(id), &encode_png(&img));
        Ok(img)
    }

    /// Re-render every thumbnail from its grid (e.g. after the palette changed).
    pub fn rerender_thumbnails(&self) -> usize {
        let mut n = 0;
        for e in self.list() {
            if let Ok(d) = self.load(&e.id) {
                if write_atomic(&e.thumb_path, &encode_png(&render_design_thumb(&d, &self.palette))).is_ok() {
                    n += 1;
                }
            }
        }
        n
    }

    /// Write built-in designs whose id is not present yet (`force` overwrites). Returns how many were written.
    /// Each item: (id, design). Use this on first launch so the library is never empty.
    pub fn seed(&self, items: &[(String, Design)], force: bool) -> io::Result<usize> {
        let mut n = 0;
        for (id, d) in items {
            if force || !self.exists(id) {
                self.write(id, d, None)?;
                n += 1;
            }
        }
        Ok(n)
    }

    /// Every tag in the library with its count, most used first.
    pub fn all_tags(entries: &[Entry]) -> Vec<(String, usize)> {
        let mut m: HashMap<String, usize> = HashMap::new();
        for e in entries {
            for t in &e.info.tags {
                *m.entry(t.clone()).or_default() += 1;
            }
        }
        let mut v: Vec<_> = m.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        v
    }
}

fn grid_dims(n: usize) -> (u32, u32) {
    if n == (GRID_W * GRID_H) as usize {
        (GRID_W, GRID_H)
    } else {
        // unknown size: assume 16:9
        let h = ((n as f64 * 9.0 / 16.0).sqrt()).round().max(1.0) as u32;
        (n as u32 / h, h)
    }
}

pub fn default_author() -> String {
    std::env::var("USER").unwrap_or_default()
}

// ---- free functions on the default library (the API named in the brief) ----

pub fn list() -> Vec<Entry> {
    Library::default().list()
}
pub fn save(name: &str, meta: &[u32], params: [f32; 4], thumb_rgba: Option<(u32, u32, &[u8])>) -> io::Result<String> {
    Library::default().save(name, meta, params, thumb_rgba)
}
pub fn load(id: &str) -> io::Result<Design> {
    Library::default().load(id)
}
pub fn delete(id: &str) -> io::Result<()> {
    Library::default().delete(id)
}
pub fn import(path: &Path) -> io::Result<String> {
    Library::default().import(path)
}
pub fn export(id: &str, path: &Path) -> io::Result<()> {
    Library::default().export(id, path)
}

/// Does a search query match an entry? Words are ANDed; `#tag` / `tag:x` match tags only.
pub fn matches(e: &Entry, query: &str) -> bool {
    let hay = format!("{} {} {} {} {}", e.info.name, e.info.author, e.info.description, e.info.tags.join(" "), e.id).to_lowercase();
    query.split_whitespace().all(|w| {
        let w = w.to_lowercase();
        if let Some(t) = w.strip_prefix('#').or_else(|| w.strip_prefix("tag:")) {
            e.info.tags.iter().any(|x| x.contains(t))
        } else {
            hay.contains(&w)
        }
    })
}

// ============================================================================================
// egui browser
// ============================================================================================

#[allow(unused_imports)]
pub use browser::{BrowserAction, LibraryBrowser};

pub mod browser {
    use super::*;
    use eframe::egui::{self, Align2, Color32, CornerRadius, FontId, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, TextureHandle, Vec2};

    const ACCENT: Color32 = Color32::from_rgb(255, 140, 50);
    const CARD: Color32 = Color32::from_rgb(34, 37, 45);
    const CARD_HOVER: Color32 = Color32::from_rgb(44, 48, 58);
    const DIM: Color32 = Color32::from_rgb(150, 155, 168);

    /// What the app must do after a frame of the browser.
    #[derive(Clone, Debug, PartialEq)]
    pub enum BrowserAction {
        /// Load this design into the sandbox: `library.load(&id)` and start it.
        Load(String),
        /// Save the current sandbox as a new design with this metadata
        /// (the app reads the grid from the GPU and calls `Library::save_design`, then `browser.refresh()`).
        SaveCurrent(DesignInfo),
        /// Overwrite design `id` with the current sandbox grid (`Library::overwrite`).
        Overwrite(String),
    }

    #[derive(Clone, Copy, PartialEq)]
    enum Sort {
        Newest,
        Oldest,
        Name,
    }

    #[derive(Default)]
    struct Form {
        name: String,
        author: String,
        description: String,
        tags: String,
    }

    pub struct LibraryBrowser {
        pub lib: Library,
        entries: Vec<Entry>,
        loaded: bool,
        search: String,
        tag: Option<String>,
        sort: Sort,
        textures: HashMap<String, (u64, TextureHandle)>,
        failed: HashMap<String, u64>,
        selected: Option<String>,
        confirm_delete: Option<String>,
        editing: Option<(String, Form)>,
        save_form: Form,
        show_save: bool,
        status: Option<(String, bool)>,
        /// thumbnail width of a grid card in points
        pub card_w: f32,
        /// allow the "Save current" panel (hide it e.g. in missions)
        pub allow_save: bool,
    }

    impl Default for LibraryBrowser {
        fn default() -> Self {
            Self::new(Library::default())
        }
    }

    impl LibraryBrowser {
        pub fn new(lib: Library) -> Self {
            LibraryBrowser {
                lib,
                entries: vec![],
                loaded: false,
                search: String::new(),
                tag: None,
                sort: Sort::Newest,
                textures: HashMap::new(),
                failed: HashMap::new(),
                selected: None,
                confirm_delete: None,
                editing: None,
                save_form: Form { author: default_author(), ..Default::default() },
                show_save: false,
                status: None,
                card_w: 192.0,
                allow_save: true,
            }
        }

        /// Re-scan the folder (call after saving from the app).
        pub fn refresh(&mut self) {
            self.entries = self.lib.list();
            self.loaded = true;
            let ids: Vec<&str> = self.entries.iter().map(|e| e.id.as_str()).collect();
            self.textures.retain(|k, _| ids.contains(&k.as_str()));
            if let Some(s) = &self.selected {
                if !ids.contains(&s.as_str()) {
                    self.selected = None;
                }
            }
        }

        pub fn entries(&self) -> &[Entry] {
            &self.entries
        }

        /// Select a design by id (e.g. the one just saved).
        pub fn select(&mut self, id: &str) {
            if !self.loaded {
                self.refresh();
            }
            self.selected = Some(id.to_string());
        }

        pub fn set_status(&mut self, msg: impl Into<String>, error: bool) {
            self.status = Some((msg.into(), error));
        }

        /// Open the "save current design" panel pre-filled with `name`.
        pub fn begin_save(&mut self, name: &str) {
            self.show_save = true;
            self.save_form.name = name.to_string();
        }

        /// The browser as a floating window. Returns an action for the app to perform.
        pub fn window(&mut self, ctx: &egui::Context, open: &mut bool) -> Option<BrowserAction> {
            let mut act = None;
            egui::Window::new("Design library")
                .open(open)
                .default_size([760.0, 560.0])
                .min_width(420.0)
                .resizable(true)
                .collapsible(false)
                .show(ctx, |ui| {
                    act = self.ui(ui);
                });
            act
        }

        fn texture(&mut self, ctx: &egui::Context, e: &Entry, budget: &mut u32) -> Option<egui::TextureId> {
            if let Some((t, tex)) = self.textures.get(&e.id) {
                if *t == e.file_time {
                    return Some(tex.id());
                }
            }
            if self.failed.get(&e.id) == Some(&e.file_time) || *budget == 0 {
                return self.textures.get(&e.id).map(|t| t.1.id());
            }
            *budget -= 1;
            match self.lib.thumbnail(&e.id) {
                Ok(img) => {
                    let ci = egui::ColorImage::from_rgba_unmultiplied([img.w as usize, img.h as usize], &img.data);
                    let tex = ctx.load_texture(format!("ignis-lib-{}", e.id), ci, egui::TextureOptions::LINEAR);
                    let id = tex.id();
                    self.textures.insert(e.id.clone(), (e.file_time, tex));
                    Some(id)
                }
                Err(_) => {
                    self.failed.insert(e.id.clone(), e.file_time);
                    None
                }
            }
        }

        fn visible(&self) -> Vec<usize> {
            let mut v: Vec<usize> = (0..self.entries.len())
                .filter(|&i| {
                    let e = &self.entries[i];
                    matches(e, &self.search) && self.tag.as_ref().is_none_or(|t| e.info.tags.contains(t))
                })
                .collect();
            match self.sort {
                Sort::Newest => v.sort_by(|&a, &b| self.entries[b].info.modified.cmp(&self.entries[a].info.modified)),
                Sort::Oldest => v.sort_by(|&a, &b| self.entries[a].info.created.cmp(&self.entries[b].info.created)),
                Sort::Name => v.sort_by_key(|&a| self.entries[a].info.name.to_lowercase()),
            }
            v
        }

        /// Draw the browser into `ui`. Returns an action for the app.
        pub fn ui(&mut self, ui: &mut egui::Ui) -> Option<BrowserAction> {
            if !self.loaded {
                self.refresh();
            }
            let mut act = None;
            let ctx = ui.ctx().clone();

            // ---- toolbar ----
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Search name, author, #tag...").desired_width(220.0));
                if !self.search.is_empty() && ui.small_button("x").on_hover_text("Clear search").clicked() {
                    self.search.clear();
                }
                egui::ComboBox::from_id_salt("ignis_lib_sort")
                    .width(90.0)
                    .selected_text(match self.sort {
                        Sort::Newest => "Newest",
                        Sort::Oldest => "Oldest",
                        Sort::Name => "A-Z",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.sort, Sort::Newest, "Newest");
                        ui.selectable_value(&mut self.sort, Sort::Oldest, "Oldest");
                        ui.selectable_value(&mut self.sort, Sort::Name, "A-Z");
                    });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Folder").on_hover_text("Show the library folder in Finder").clicked() {
                        let _ = std::fs::create_dir_all(&self.lib.dir);
                        let _ = std::process::Command::new("open").arg(&self.lib.dir).spawn();
                    }
                    if ui.button("Import...").on_hover_text("Add .cmb designs from anywhere").clicked() {
                        if let Some(paths) = rfd::FileDialog::new().add_filter("Ignis design", &["cmb"]).pick_files() {
                            let (mut ok, mut bad) = (0, 0);
                            let mut last = None;
                            for p in paths {
                                match self.lib.import(&p) {
                                    Ok(id) => {
                                        ok += 1;
                                        last = Some(id);
                                    }
                                    Err(_) => bad += 1,
                                }
                            }
                            self.refresh();
                            self.selected = last;
                            self.set_status(format!("Imported {ok} design(s){}", if bad > 0 { format!(", {bad} failed") } else { String::new() }), bad > 0);
                        }
                    }
                    if self.allow_save {
                        let b = egui::Button::new(RichText::new("Save current").color(Color32::BLACK)).fill(ACCENT);
                        if ui.add(b).on_hover_text("Save what is on the canvas into the library").clicked() {
                            self.show_save = !self.show_save;
                        }
                    }
                });
            });

            // ---- save panel ----
            if self.show_save && self.allow_save {
                egui::Frame::group(ui.style()).fill(Color32::from_rgb(28, 30, 37)).show(ui, |ui| {
                    ui.label(RichText::new("Save the current canvas").strong());
                    form_fields(ui, &mut self.save_form, "save");
                    ui.horizontal(|ui| {
                        let ok = !self.save_form.name.trim().is_empty();
                        if ui.add_enabled(ok, egui::Button::new("Save to library")).clicked() {
                            let f = &self.save_form;
                            act = Some(BrowserAction::SaveCurrent(DesignInfo {
                                name: f.name.trim().to_string(),
                                author: f.author.trim().to_string(),
                                description: f.description.trim().to_string(),
                                tags: parse_tags(&f.tags),
                                source: "user".into(),
                                ..Default::default()
                            }));
                            self.show_save = false;
                            self.save_form.name.clear();
                            self.save_form.description.clear();
                        }
                        if let Some(sel) = self.selected.clone() {
                            if ui.button("Overwrite selected").on_hover_text("Replace the selected design's layout with the canvas").clicked() {
                                act = Some(BrowserAction::Overwrite(sel));
                                self.show_save = false;
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            self.show_save = false;
                        }
                    });
                });
            }

            // ---- tag chips ----
            let tags = Library::all_tags(&self.entries);
            if !tags.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
                    if ui.selectable_label(self.tag.is_none(), format!("All {}", self.entries.len())).clicked() {
                        self.tag = None;
                    }
                    for (t, n) in tags.iter().take(24) {
                        let on = self.tag.as_deref() == Some(t.as_str());
                        if ui.selectable_label(on, format!("#{t} {n}")).clicked() {
                            self.tag = if on { None } else { Some(t.clone()) };
                        }
                    }
                });
            }

            if let Some((msg, err)) = &self.status {
                ui.colored_label(if *err { Color32::from_rgb(255, 110, 90) } else { Color32::from_rgb(140, 220, 140) }, msg);
            }
            ui.separator();

            // ---- detail panel for the selection (bottom) ----
            let sel_idx = self.selected.as_ref().and_then(|s| self.entries.iter().position(|e| &e.id == s));
            let detail_h = if sel_idx.is_some() { 150.0 } else { 0.0 };
            let vis = self.visible();
            let mut budget = 3u32; // thumbnails decoded per frame

            let grid_h = (ui.available_height() - detail_h).max(120.0);
            egui::ScrollArea::vertical().id_salt("ignis_lib_grid").max_height(grid_h).auto_shrink([false, false]).show(ui, |ui| {
                if vis.is_empty() {
                    ui.add_space(30.0);
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new(if self.entries.is_empty() { "The library is empty." } else { "Nothing matches." }).color(DIM).size(16.0));
                        if self.entries.is_empty() {
                            ui.label(RichText::new("Build something and press Save current.").color(DIM));
                        }
                    });
                    return;
                }
                let gap = 10.0;
                let avail = ui.available_width();
                let cols = (((avail + gap) / (self.card_w + gap)).floor() as usize).max(1);
                // stretch cards to fill the row
                let cw = ((avail - gap * (cols as f32 - 1.0)) / cols as f32).max(80.0).floor();
                let th = cw * THUMB_H as f32 / THUMB_W as f32;
                let ch = th + 46.0;
                for row in vis.chunks(cols) {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = gap;
                        for &i in row {
                            let e = self.entries[i].clone();
                            let tex = self.texture(&ctx, &e, &mut budget);
                            let (rect, resp) = ui.allocate_exact_size(Vec2::new(cw, ch), Sense::click());
                            if !ui.is_rect_visible(rect) {
                                continue;
                            }
                            let sel = self.selected.as_deref() == Some(e.id.as_str());
                            let p = ui.painter_at(rect.expand(2.0));
                            p.rect_filled(rect, CornerRadius::same(8), if resp.hovered() { CARD_HOVER } else { CARD });
                            let ir = Rect::from_min_size(rect.min + Vec2::new(4.0, 4.0), Vec2::new(cw - 8.0, th - 8.0 * th / cw));
                            match tex {
                                Some(t) => {
                                    p.image(t, ir, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                                }
                                None => {
                                    p.rect_filled(ir, CornerRadius::same(4), Color32::from_rgb(22, 24, 30));
                                    p.text(ir.center(), Align2::CENTER_CENTER, "...", FontId::proportional(14.0), DIM);
                                }
                            }
                            let tp = Pos2::new(rect.left() + 8.0, ir.bottom() + 5.0);
                            let clip = p.with_clip_rect(Rect::from_min_max(Pos2::new(rect.left(), ir.bottom()), Pos2::new(rect.right() - 6.0, rect.bottom())));
                            clip.text(tp, Align2::LEFT_TOP, &e.info.name, FontId::proportional(14.0), Color32::from_rgb(235, 236, 240));
                            let sub = match (e.info.author.is_empty(), e.info.tags.first()) {
                                (false, Some(t)) => format!("{}  ·  #{}", e.info.author, t),
                                (false, None) => e.info.author.clone(),
                                (true, Some(t)) => format!("#{t}  ·  {}", format_date(e.info.modified)),
                                (true, None) => format_date(e.info.modified),
                            };
                            clip.text(tp + Vec2::new(0.0, 19.0), Align2::LEFT_TOP, sub, FontId::proportional(11.5), DIM);
                            if sel {
                                p.rect_stroke(rect, CornerRadius::same(8), Stroke::new(2.0_f32, ACCENT), StrokeKind::Inside);
                            }
                            let resp = resp.on_hover_text(if e.info.description.is_empty() { e.info.name.clone() } else { e.info.description.clone() });
                            if resp.double_clicked() {
                                act = Some(BrowserAction::Load(e.id.clone()));
                            } else if resp.clicked() {
                                self.selected = Some(e.id.clone());
                                self.confirm_delete = None;
                                self.editing = None;
                            }
                        }
                    });
                    ui.add_space(gap);
                }
            });

            // ---- detail panel ----
            if let Some(i) = sel_idx {
                let e = self.entries[i].clone();
                ui.separator();
                ui.horizontal_top(|ui| {
                    if let Some((_, tex)) = self.textures.get(&e.id) {
                        let sz = Vec2::new(192.0, 108.0);
                        ui.add(egui::Image::new((tex.id(), sz)).corner_radius(CornerRadius::same(4)));
                    }
                    ui.vertical(|ui| {
                        if let Some((eid, form)) = &mut self.editing {
                            if *eid == e.id {
                                form_fields(ui, form, "edit");
                            }
                        } else {
                            ui.label(RichText::new(&e.info.name).strong().size(17.0));
                            let mut line = format!("{}  ·  created {}", if e.info.author.is_empty() { "unknown author" } else { &e.info.author }, format_date(e.info.created));
                            if !e.info.source.is_empty() && e.info.source != "user" {
                                line.push_str(&format!("  ·  {}", e.info.source));
                            }
                            ui.label(RichText::new(line).color(DIM).size(12.0));
                            if !e.info.description.is_empty() {
                                ui.label(&e.info.description);
                            }
                            if !e.info.tags.is_empty() {
                                ui.horizontal_wrapped(|ui| {
                                    for t in &e.info.tags {
                                        if ui.small_button(format!("#{t}")).clicked() {
                                            self.tag = Some(t.clone());
                                        }
                                    }
                                });
                            }
                        }
                        ui.horizontal(|ui| {
                            let editing_this = self.editing.as_ref().is_some_and(|(id, _)| *id == e.id);
                            if editing_this {
                                if ui.button("Apply").clicked() {
                                    let (_, f) = self.editing.take().unwrap();
                                    let info = DesignInfo {
                                        name: f.name.trim().to_string(),
                                        author: f.author.trim().to_string(),
                                        description: f.description.trim().to_string(),
                                        tags: parse_tags(&f.tags),
                                        ..Default::default()
                                    };
                                    match self.lib.update_info(&e.id, info) {
                                        Ok(()) => self.set_status("Details updated", false),
                                        Err(err) => self.set_status(format!("Update failed: {err}"), true),
                                    }
                                    self.refresh();
                                }
                                if ui.button("Cancel").clicked() {
                                    self.editing = None;
                                }
                                return;
                            }
                            let b = egui::Button::new(RichText::new("Load").strong().color(Color32::BLACK)).fill(ACCENT);
                            if ui.add(b).on_hover_text("Open this design in the sandbox (double-click a card also works)").clicked() {
                                act = Some(BrowserAction::Load(e.id.clone()));
                            }
                            if ui.button("Edit info").clicked() {
                                self.editing = Some((
                                    e.id.clone(),
                                    Form { name: e.info.name.clone(), author: e.info.author.clone(), description: e.info.description.clone(), tags: e.info.tags.join(", ") },
                                ));
                            }
                            if ui.button("Duplicate").clicked() {
                                match self.lib.duplicate(&e.id) {
                                    Ok(id) => {
                                        self.refresh();
                                        self.selected = Some(id);
                                        self.set_status("Duplicated", false);
                                    }
                                    Err(err) => self.set_status(format!("Duplicate failed: {err}"), true),
                                }
                            }
                            if ui.button("Export...").on_hover_text("Save a copy as .cmb (or .png for the thumbnail)").clicked() {
                                if let Some(p) = rfd::FileDialog::new()
                                    .add_filter("Ignis design", &["cmb"])
                                    .add_filter("Thumbnail", &["png"])
                                    .set_file_name(format!("{}.cmb", e.id))
                                    .save_file()
                                {
                                    match self.lib.export(&e.id, &p) {
                                        Ok(()) => self.set_status(format!("Exported to {}", p.display()), false),
                                        Err(err) => self.set_status(format!("Export failed: {err}"), true),
                                    }
                                }
                            }
                            if self.confirm_delete.as_deref() == Some(e.id.as_str()) {
                                let b = egui::Button::new(RichText::new("Really delete?").color(Color32::WHITE)).fill(Color32::from_rgb(170, 40, 30));
                                if ui.add(b).clicked() {
                                    match self.lib.delete(&e.id) {
                                        Ok(()) => self.set_status(format!("Deleted \"{}\" (moved to the library's .trash folder)", e.info.name), false),
                                        Err(err) => self.set_status(format!("Delete failed: {err}"), true),
                                    }
                                    self.confirm_delete = None;
                                    self.refresh();
                                }
                                if ui.button("Keep").clicked() {
                                    self.confirm_delete = None;
                                }
                            } else if ui.button("Delete").clicked() {
                                self.confirm_delete = Some(e.id.clone());
                            }
                        });
                    });
                });
            }
            if budget == 0 {
                ctx.request_repaint(); // more thumbnails to decode next frame
            }
            act
        }
    }

    fn form_fields(ui: &mut egui::Ui, f: &mut Form, salt: &str) {
        egui::Grid::new(format!("ignis_lib_form_{salt}")).num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
            ui.label("Name");
            ui.add(egui::TextEdit::singleline(&mut f.name).hint_text("My rocket").desired_width(260.0));
            ui.end_row();
            ui.label("Author");
            ui.add(egui::TextEdit::singleline(&mut f.author).desired_width(260.0));
            ui.end_row();
            ui.label("Tags");
            ui.add(egui::TextEdit::singleline(&mut f.tags).hint_text("rocket, solid motor").desired_width(260.0));
            ui.end_row();
            ui.label("Notes");
            ui.add(egui::TextEdit::multiline(&mut f.description).hint_text("What does it do? How to run it?").desired_rows(2).desired_width(260.0));
            ui.end_row();
        });
    }
}

// ============================================================================================
// tests (run through the library_seed example: `cargo run --release --example library_seed -- --selftest`)
// ============================================================================================

pub fn selftest() -> Result<(), String> {
    // PNG round trip
    let mut img = RgbaImage::new(37, 23);
    for (i, b) in img.data.iter_mut().enumerate() {
        *b = ((i * 7) ^ (i / 13)) as u8;
    }
    let png = encode_png(&img);
    let back = decode_png(&png)?;
    if back != img {
        return Err("png round trip mismatch".into());
    }
    // zlib round trip with long runs + randomness
    let mut data = vec![0u8; 70000];
    let mut s = 1u32;
    for (i, d) in data.iter_mut().enumerate() {
        s = s.wrapping_mul(1_103_515_245).wrapping_add(12345);
        *d = if (i / 500) % 2 == 0 { (s >> 24) as u8 } else { 42 };
    }
    if zlib_decompress(&zlib_compress(&data), 0)? != data {
        return Err("zlib round trip mismatch".into());
    }
    // cmb round trip
    let mut d = Design::new(8, 4, (0..32).collect(), [1.0, 2.0, 3.0, 4.0], DesignInfo {
        name: "Test ✓".into(),
        author: "me".into(),
        description: "line1\nline2".into(),
        tags: vec!["a".into(), "b c".into()],
        created: 5,
        modified: 6,
        source: "test".into(),
    });
    d.ops.push([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
    let b = encode_cmb(&d);
    if decode_cmb(&b)? != d {
        return Err("cmb round trip mismatch".into());
    }
    // old-format file (no chunk) still loads
    let old = &b[..HEADER + 32 * 4];
    let o = decode_cmb(old)?;
    if o.meta != d.meta || o.info != DesignInfo::default() {
        return Err("old cmb decode mismatch".into());
    }
    // corrupt chunk is ignored, grid still loads
    let mut bad = b.clone();
    let k = bad.len() - 10;
    bad[k] ^= 0xff;
    if decode_cmb(&bad)?.meta != d.meta {
        return Err("corrupt chunk broke grid".into());
    }
    if format_date(1_793_145_600) != "2026-10-28" {
        return Err(format!("date {}", format_date(1_793_145_600)));
    }
    if parse_tags(" Rocket, solid motor ,#APCP,rocket") != vec!["rocket", "solid motor", "apcp"] {
        return Err("parse_tags".into());
    }
    Ok(())
}
