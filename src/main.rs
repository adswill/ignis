#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
mod app;
mod elements;
mod missions;
mod gpu;
mod scene;
mod audio;
mod campaign;
mod campaign_bridge;
mod codex;
mod library;
mod settings;
mod showcase;
mod propulsion;
mod engine;
mod demos2;
mod tutorial;

use eframe::wgpu;
use gpu::*;

fn headless(args: &[String]) {
    let frames: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(200);
    let which = args.get(3).map(|s| s.as_str()).unwrap_or("jet");
    let shot = args.get(4).cloned();
    let view: u32 = args.get(5).and_then(|s| s.parse().ok()).unwrap_or(0);
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).expect("adapter");
    println!("adapter: {}", adapter.get_info().name);
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default(), None)).expect("device");
    let fmt = wgpu::TextureFormat::Rgba8Unorm;
    let mut sim = GpuSim::new(&device, fmt);
    // TSCALE=k: scene time-scale factor (fast-forward); ADT=0 disables the CFL-adaptive step
    if let Ok(v) = std::env::var("TSCALE") { sim.time_scale = v.parse().unwrap_or(1.0); }
    if std::env::var("ADT").map(|v| v == "0").unwrap_or(false) { sim.adaptive_dt = false; }
    let sc = match which { "rocket" => scene::Scene::rocket(), "empty" => scene::Scene::empty(), "vessels" => scene::Scene::vessels(), "flame" => scene::Scene::flame_test(), "fireball" => scene::Scene::fireball(), "flare" => scene::Scene::flare(), "campfire" => scene::Scene::campfire(), "srm" => scene::Scene::solid_rocket(), "demo" => scene::Scene::demolition(), "chem" => scene::Scene::chem_lab(), "melt" => scene::Scene::melt_test(), "jet" => scene::Scene::turbojet(), n => engine::by_name(n).or_else(|| demos2::by_name(n)).or_else(|| propulsion::by_name(n)).or_else(|| showcase::by_name(n)).or_else(|| scene::Scene::by_name(n)).unwrap_or_else(scene::Scene::turbojet) };
    let mut sc = sc;
    // extra edits for tests: OPS="kind:shape:x0:y0:x1:y1:r:tile:mat;..." (see Op in gpu.rs)
    if let Ok(spec) = std::env::var("OPS") {
        for o in spec.split(';').filter(|o| !o.is_empty()) {
            let v: Vec<i32> = o.split(':').map(|x| x.trim().parse().expect("OPS: integer fields")).collect();
            let g = |k: usize| v.get(k).copied().unwrap_or(0);
            sc.ops.push(Op { kind: g(0) as u32, shape: g(1) as u32, x0: g(2), y0: g(3), x1: g(4), y1: g(5), r: g(6), tile: g(7) as u32, mat: g(8) as u32, dir: 0, _p: [0; 2] });
        }
    }
    gpu::upload_engine(sc.engine.clone());
    let mut inp = FrameInput {
        steps: std::env::var("STEPS").ok().and_then(|v| v.parse().ok()).unwrap_or(20),
        ops: sc.ops.clone(),
        view,
        p_fuel: 1.0,
        p_o2: 1.0,
        p_air: sc.p_air,
        cam: [0.0, 0.0, W as f32, H as f32],
        gravity: std::env::var("GRAV").ok().and_then(|v| v.parse().ok()).unwrap_or(GRAVITY),
        fan_pow: std::env::var("FAN").ok().and_then(|v| v.parse().ok()).unwrap_or(1.0),
        ign_on: std::env::var("IGN").map(|v| v != "0").unwrap_or(true),
        time: 0.0,
        frame: 0,
        scene: Some(sc.meta.clone()),
        meta_only: None,
        reset_gas: false,
        probe: Some((0, 0)),
    };
    let t0 = std::time::Instant::now();
    let mut tot0: Option<(f64, f64)> = None;
    let ignis_totals = gpu::conserved_totals;
    for f in 0..frames {
        let mut enc = device.create_command_encoder(&Default::default());
        sim.frame(&device, &queue, &mut enc, &inp);
        queue.submit([enc.finish()]);
        // like the app's vsync: keep the CPU at most ~1 frame ahead so the adaptive-dt read-back is not seconds late
        if sim.adaptive_dt { device.poll(wgpu::Maintain::Wait); }
        inp.scene = None;
        inp.ops.clear();
        // OPS_EVERY: like OPS but applied every frame (e.g. a held torch)
        if let Ok(spec) = std::env::var("OPS_EVERY") {
            for o in spec.split(';').filter(|o| !o.is_empty()) {
                let v: Vec<i32> = o.split(':').map(|x| x.trim().parse().expect("OPS_EVERY: integer fields")).collect();
                let g = |k: usize| v.get(k).copied().unwrap_or(0);
                inp.ops.push(Op { kind: g(0) as u32, shape: g(1) as u32, x0: g(2), y0: g(3), x1: g(4), y1: g(5), r: g(6), tile: g(7) as u32, mat: g(8) as u32, dir: 0, _p: [0; 2] });
            }
        }
        inp.frame = f;
        let r = ((f as f32 + 1.0) * 20.0 * DT / 40.0).min(1.0);
        let r = r * r * (3.0 - 2.0 * r);
        let env_f = |k: &str, d: f32| std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d);
        inp.p_fuel = 1.0 + (env_f("PFUEL", sc.inlet_p) - 1.0) * r;
        inp.p_o2 = 1.0 + (env_f("PO2", sc.o2_p) - 1.0) * r;
        inp.time += 20.0 * DT;
        if f % std::env::var("EVERY").ok().and_then(|v| v.parse().ok()).unwrap_or(25u32) == 0 || f + 1 == frames {
            device.poll(wgpu::Maintain::Wait);
            let (s, m) = sim.read_state(&device, &queue);
            let (mut tmax, mut pmax, mut vmax, mut bad) = (0.0f32, 0.0f32, 0.0f32, 0);
            let (mut pi, mut vi, mut ti) = (0usize, 0usize, 0usize);
            for i in 0..(W * H) as usize {
                if matches!(m[i] & 0xff, 1 | 6) {
                    continue;
                }
                let c = &s[i * 8..i * 8 + 8];
                let rho = c[0] + c[1] + c[2] + c[3];
                if !(rho.is_finite()) || !c[6].is_finite() {
                    bad += 1;
                    continue;
                }
                let v = (c[4] * c[4] + c[5] * c[5]).sqrt() / rho;
                let cv = c[0] * 5.827 + c[1] * 2.258 + c[2] * 2.580 + c[3] * 4.446;
                let rsum = c[0] * 1.80625 + c[1] * 0.903125 + c[2] * 1.032142857 + c[3] * 1.111538;
                let p = rsum / cv * (c[6] - 0.5 * v * v * rho);
                let rm = (c[0] * 1.80625 + c[1] * 0.903125 + c[2] * 1.032142857 + c[3] * 1.111538) / rho;
                let tc = p / (rho * rm);
                if tc > tmax { tmax = tc; ti = i; }
                if p > pmax { pmax = p; pi = i; }
                if v > vmax { vmax = v; vi = i; }
            }
            if std::env::var("DUMP").is_ok() { for c in std::env::var("CELLS").ok().map(|v| v.split(',').filter_map(|p| { let mut it = p.split(':'); Some(it.next()?.parse::<usize>().ok()? + it.next()?.parse::<usize>().ok()? * W as usize) }).collect::<Vec<_>>()).unwrap_or(vec![0usize, 1, 2]) { println!("cell {c}: {:?}", &s[c*8..c*8+8]); } }
            if let Ok(spec) = std::env::var("TMAP") {
                let v: Vec<usize> = spec.split(',').filter_map(|t| t.parse().ok()).collect();
                if v.len() == 4 {
                    for y in (v[2]..v[3]).rev() {
                        let mut line = String::new();
                        for x in v[0]..v[1] {
                            let t = m[y * W as usize + x] & 0xff;
                            line.push(match t { 0 => '.', 1 => '#', 9 => 'P', 10 => 'v', 11 => 'o', 12 => 's', 13 => 'i', 14 => 'I', 15 => 'm', 8 => 'p', _ => '?' });
                        }
                        println!("{y:3} {line}");
                    }
                }
            }
            if std::env::var("TILES").is_ok() {
                let mut c = std::collections::BTreeMap::new();
                for v in &m { if v & 0xff == 1 || v & 0xff == 8 { *c.entry((v & 0xff, (v >> 8) & 0xff)).or_insert(0) += 1; } }
                println!("  tiles {:?}", c);
            }
            if let Ok(spec) = std::env::var("SUM") {
                // SUM=x0,x1,y0,y1 : total species mass in a box (gas tiles only)
                let v: Vec<usize> = spec.split(',').filter_map(|t| t.parse().ok()).collect();
                if v.len() == 4 {
                    let mut tot = [0.0f64; 4];
                    let (mut psum, mut ncell) = (0.0f64, 0usize);
                    let from_piston = std::env::var("SUMC").is_ok();
                    for y in v[2]..v[3] {
                        // with SUMC only the gas in front of the piston crown counts (the combustion chamber)
                        let mut start = v[0];
                        if from_piston {
                            for x in v[0]..v[1] {
                                if m[y * W as usize + x] & 0xff == 9 {
                                    start = x + 1;
                                }
                            }
                        }
                        for x in start..v[1] {
                            let k = y * W as usize + x;
                            if m[k] & 0xff == 0 || m[k] & 0xff == 11 {
                                for q in 0..4 {
                                    tot[q] += s[k * 8 + q] as f64;
                                }
                                let cc = &s[k * 8..k * 8 + 8];
                                let rho = (cc[0] + cc[1] + cc[2] + cc[3]).max(1e-4);
                                let cv = cc[0] * 5.827 + cc[1] * 2.258 + cc[2] * 2.580 + cc[3] * 4.446;
                                let rsum = cc[0] * 1.80625 + cc[1] * 0.903125 + cc[2] * 1.032142857 + cc[3] * 1.111538;
                                let ke = 0.5 * (cc[4] * cc[4] + cc[5] * cc[5]) / rho;
                                psum += (rsum / cv.max(1e-6) * (cc[6] - ke)) as f64;
                                ncell += 1;
                            }
                        }
                    }
                    println!("  sum fuel {:.3} o2 {:.3} n2 {:.3} prod {:.3} total {:.2} pavg {:.3} cells {}", tot[0], tot[1], tot[2], tot[3], tot.iter().sum::<f64>(), psum / (ncell.max(1) as f64), ncell);
                }
            }
            if let Ok(spec) = std::env::var("WCELLS") {
                let wv = sim.read_wall(&device, &queue);
                for p in spec.split(',') {
                    let mut it = p.split(':');
                    let (x, y): (usize, usize) = (it.next().unwrap().parse().unwrap(), it.next().unwrap().parse().unwrap());
                    let k = y * W as usize + x;
                    println!("  w({x},{y}) tile {} mat {} T {:.2} flag {:.2} code {:.1} rem {:.3}", m[k] & 0xff, (m[k] >> 8) & 0xff, wv[k * 4], wv[k * 4 + 1], wv[k * 4 + 2], wv[k * 4 + 3]);
                }
            }
            if std::env::var("FRONT").is_ok() {
                // flame front: right-most cell on the centre row whose gas is mostly burnt
                let row = 144usize;
                let mut front = 0;
                for x in 0..W as usize {
                    let c = &s[(row * W as usize + x) * 8..(row * W as usize + x) * 8 + 8];
                    let rho = c[0] + c[1] + c[2] + c[3];
                    if c[0] / rho < 0.02 && x > 20 && x < 490 { front = x; }
                }
                println!("front t={:.1} x={front}", inp.time);
            }
            if std::env::var("AXIS").is_ok() && f >= std::env::var("AXFROM").ok().and_then(|v| v.parse().ok()).unwrap_or(0u32) {
                // jet diagnostics: centre row profile (x, p atm, u, T, Mach) and the half-width where u > 25 % of the local axis speed
                let row: usize = std::env::var("ROW").ok().and_then(|v| v.parse().ok()).unwrap_or(144);
                let x0: usize = std::env::var("AX0").ok().and_then(|v| v.parse().ok()).unwrap_or(160);
                let step: usize = std::env::var("AXSTEP").ok().and_then(|v| v.parse().ok()).unwrap_or(10);
                let prim = |x: usize, y: usize| -> (f32, f32, f32, f32, f32) {
                    let c = &s[(y * W as usize + x) * 8..(y * W as usize + x) * 8 + 8];
                    let rho = c[0] + c[1] + c[2] + c[3];
                    let (u, v) = (c[4] / rho, c[5] / rho);
                    let cv = c[0] * 5.827 + c[1] * 2.258 + c[2] * 2.580 + c[3] * 4.446;
                    let rs = c[0] * 1.80625 + c[1] * 0.903125 + c[2] * 1.032142857 + c[3] * 1.111538;
                    let gm1 = rs / cv;
                    let p = gm1 * (c[6] - 0.5 * rho * (u * u + v * v));
                    let t = p / rs;
                    let cs = ((1.0 + gm1) * p / rho).sqrt();
                    (p, u, t, (u * u + v * v).sqrt() / cs, v)
                };
                let mut x = x0;
                while x < W as usize {
                    let (p, u, t, mach, _) = prim(x, row);
                    // half-width: scan up/down from the row until |u| < 25 % of axis speed
                    let mut hw = 0usize;
                    for d in 1..140usize {
                        let (_, ua, _, _, _) = if row + d < H as usize { prim(x, row + d) } else { (0.0, 0.0, 0.0, 0.0, 0.0) };
                        let (_, ub, _, _, _) = if row >= d { prim(x, row - d) } else { (0.0, 0.0, 0.0, 0.0, 0.0) };
                        if ua.abs() < 0.25 * u.abs() && ub.abs() < 0.25 * u.abs() { break; }
                        hw = d;
                    }
                    println!("axis f={f} x={x:3} p {p:6.2} u {u:6.2} T {t:5.2} M {mach:5.2} halfwidth {hw}");
                    x += step;
                }
            }
            let nwall = m.iter().filter(|&&x| x & 0xff == 1).count();
            {
                // conservation: (stored - initial - boundary inflow - physical sources) / initial
                let acc = sim.read_accounting(&device, &queue);
                let (mt, et) = ignis_totals(&s, &m);
                let (m0, e0) = *tot0.get_or_insert((mt - acc[0] - acc[2], et - acc[1] - acc[3]));
                println!(
                    "   conservation: dm {:+.4} %  dE {:+.4} %  (bnd in m {:+.1} E {:+.1}, sources m {:+.1} E {:+.1})",
                    (mt - m0 - acc[0] - acc[2]) / m0 * 100.0,
                    (et - e0 - acc[1] - acc[3]) / e0 * 100.0,
                    acc[0], acc[1], acc[2], acc[3]
                );
            }
            let st = sim.stats;
            println!("   stats: wall heat {:.2}  stress {:.2}  burn {:.3}  thrust {:.2},{:.2}  dt {:.4} cfl {:.2} speed {:.1} tsim {:.1} steps {}", st.max_wall_heat, st.max_stress, st.burn, st.thrust[0], st.thrust[1], st.dt, st.cfl, st.max_speed, st.sim_time, sim.steps_issued);
            println!(
                "frame {f:4} wall {:5.2}s  Tmax {tmax:5.2} pmax {pmax:5.2} vmax {vmax:5.2} walls {nwall} bad {bad} pmax@({},{}) vmax@({},{}) tmax@({},{})",
                t0.elapsed().as_secs_f32(), pi % W as usize, pi / W as usize, vi % W as usize, vi / W as usize, ti % W as usize, ti / W as usize
            );
        }
    }
    device.poll(wgpu::Maintain::Wait);
    println!("{} frames x20 steps in {:.2}s", frames, t0.elapsed().as_secs_f32());
    // SAVE=path: raw final state (u0 f32[8N], meta u32[N], wall f32[4N]) for bit-equivalence checks between builds
    if let Ok(path) = std::env::var("SAVE") {
        let (s, m) = sim.read_state(&device, &queue);
        let wv = sim.read_wall(&device, &queue);
        let mut out: Vec<u8> = Vec::new();
        out.extend_from_slice(bytemuck::cast_slice(&s));
        out.extend_from_slice(bytemuck::cast_slice(&m));
        out.extend_from_slice(bytemuck::cast_slice(&wv));
        std::fs::write(&path, out).expect("SAVE");
    }
    if let Some(path) = shot {
        let (pw, ph) = (W * 3, H * 3);
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: pw, height: ph, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: fmt,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let tv = tex.create_view(&Default::default());
        let mut enc = device.create_command_encoder(&Default::default());
        inp.steps = std::env::var("SHOT_STEPS").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
        sim.frame(&device, &queue, &mut enc, &inp);
        {
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &tv,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            sim.draw(&mut rp);
        }
        let bpr = (pw * 4).next_multiple_of(256);
        let out = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (bpr * ph) as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo { buffer: &out, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(bpr), rows_per_image: Some(ph) } },
            wgpu::Extent3d { width: pw, height: ph, depth_or_array_layers: 1 },
        );
        queue.submit([enc.finish()]);
        out.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        device.poll(wgpu::Maintain::Wait);
        let data = out.slice(..).get_mapped_range();
        use std::io::Write;
        let mut f = std::fs::File::create(&shot_path(&path)).unwrap();
        write!(f, "P6\n{} {}\n255\n", pw, ph).unwrap();
        let mut bytes = Vec::with_capacity((pw * ph * 3) as usize);
        for y in 0..ph {
            for x in 0..pw {
                let o = (y * bpr + x * 4) as usize;
                bytes.extend_from_slice(&data[o..o + 3]);
            }
        }
        f.write_all(&bytes).unwrap();
    }
}

fn shot_path(p: &str) -> String {
    p.to_string()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(|s| s.as_str()) == Some("--bench") {
        headless(&args);
        return;
    }
    let mut vp = eframe::egui::ViewportBuilder::default()
        .with_title("Ignis")
        .with_inner_size([1560.0, 900.0])
        .with_min_inner_size([1100.0, 680.0]);
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../assets/branding/icon_256.png")) {
        vp = vp.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: vp,
        vsync: true,
        ..Default::default()
    };
    eframe::run_native("Ignis", options, Box::new(|cc| Ok(Box::new(app::App::new(cc))))).expect("failed to start");
}
