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
    let sc = match which { "rocket" => scene::Scene::rocket(), "empty" => scene::Scene::empty(), "vessels" => scene::Scene::vessels(), "flame" => scene::Scene::flame_test(), "fireball" => scene::Scene::fireball(), "flare" => scene::Scene::flare(), "campfire" => scene::Scene::campfire(), "srm" => scene::Scene::solid_rocket(), "demo" => scene::Scene::demolition(), "chem" => scene::Scene::chem_lab(), "melt" => scene::Scene::melt_test(), "jet" => scene::Scene::turbojet(), n => scene::Scene::by_name(n).unwrap_or_else(scene::Scene::turbojet) };
    let mut sc = sc;
    // extra edits for tests: OPS="kind:shape:x0:y0:x1:y1:r:tile:mat;..." (see Op in gpu.rs)
    if let Ok(spec) = std::env::var("OPS") {
        for o in spec.split(';').filter(|o| !o.is_empty()) {
            let v: Vec<i32> = o.split(':').map(|x| x.trim().parse().expect("OPS: integer fields")).collect();
            let g = |k: usize| v.get(k).copied().unwrap_or(0);
            sc.ops.push(Op { kind: g(0) as u32, shape: g(1) as u32, x0: g(2), y0: g(3), x1: g(4), y1: g(5), r: g(6), tile: g(7) as u32, mat: g(8) as u32, dir: 0, _p: [0; 2] });
        }
    }
    let mut inp = FrameInput {
        steps: std::env::var("STEPS").ok().and_then(|v| v.parse().ok()).unwrap_or(20),
        ops: sc.ops.clone(),
        view,
        p_fuel: 1.0,
        p_o2: 1.0,
        p_air: 2.0,
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
            let (mut pi, mut vi) = (0usize, 0usize);
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
                tmax = tmax.max(p / (rho * rm));
                if p > pmax { pmax = p; pi = i; }
                if v > vmax { vmax = v; vi = i; }
            }
            if std::env::var("DUMP").is_ok() { for c in std::env::var("CELLS").ok().map(|v| v.split(',').filter_map(|p| { let mut it = p.split(':'); Some(it.next()?.parse::<usize>().ok()? + it.next()?.parse::<usize>().ok()? * W as usize) }).collect::<Vec<_>>()).unwrap_or(vec![0usize, 1, 2]) { println!("cell {c}: {:?}", &s[c*8..c*8+8]); } }
            if std::env::var("TILES").is_ok() {
                let mut c = std::collections::BTreeMap::new();
                for v in &m { if v & 0xff == 1 || v & 0xff == 8 { *c.entry((v & 0xff, (v >> 8) & 0xff)).or_insert(0) += 1; } }
                println!("  tiles {:?}", c);
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
            println!("   stats: wall heat {:.2}  stress {:.2}  burn {:.3}  thrust {:.2},{:.2}", st.max_wall_heat, st.max_stress, st.burn, st.thrust[0], st.thrust[1]);
            println!(
                "frame {f:4} wall {:5.2}s  Tmax {tmax:5.2} pmax {pmax:5.2} vmax {vmax:5.2} walls {nwall} bad {bad} pmax@({},{}) vmax@({},{})",
                t0.elapsed().as_secs_f32(), pi % W as usize, pi / W as usize, vi % W as usize, vi / W as usize
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
