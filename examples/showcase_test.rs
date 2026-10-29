//! Headless test + exporter for src/showcase.rs (showcase team).
//!   cargo run --release --example showcase_test -- export            # writes designs/<name>.cmb
//!   cargo run --release --example showcase_test -- run <name|all> [frames] [out_dir]
//! Prints stats like `ignis --bench` and saves a 1536x864 PPM screenshot per scene.
#![allow(dead_code, unused_imports)]

#[path = "../src/gpu.rs"]
mod gpu;
#[path = "../src/scene.rs"]
mod scene;
#[path = "../src/showcase.rs"]
mod showcase;

use eframe::wgpu;
use gpu::*;

fn export() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("designs");
    std::fs::create_dir_all(&dir).unwrap();
    for (i, (title, _, f)) in showcase::all().into_iter().enumerate() {
        let sc = f();
        let mut b = b"CMB1".to_vec();
        for v in [W, H] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        for v in [sc.inlet_p, sc.o2_p, 2.0f32, 1.0] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.extend_from_slice(bytemuck::cast_slice(&sc.meta));
        let p = dir.join(format!("{}.cmb", showcase::NAMES[i]));
        std::fs::write(&p, b).unwrap();
        println!("wrote {} ({title}, {} ops not stored in .cmb)", p.display(), sc.ops.len());
    }
}

fn run(name: &str, frames: u32, out: &str, device: &wgpu::Device, queue: &wgpu::Queue) {
    let fmt = wgpu::TextureFormat::Rgba8Unorm;
    let mut sim = GpuSim::new(device, fmt);
    let sc = showcase::by_name(name).or_else(|| scene::Scene::by_name(name)).expect("unknown scene");
    let mut inp = FrameInput {
        steps: 20,
        ops: sc.ops.clone(),
        view: 0,
        p_fuel: 1.0,
        p_o2: 1.0,
        p_air: 2.0,
        cam: [0.0, 0.0, W as f32, H as f32],
        gravity: GRAVITY,
        fan_pow: 1.0,
        ign_on: true,
        time: 0.0,
        frame: 0,
        scene: Some(sc.meta.clone()),
        meta_only: None,
        reset_gas: false,
        probe: Some((0, 0)),
    };
    let every: u32 = std::env::var("EVERY").ok().and_then(|v| v.parse().ok()).unwrap_or(50);
    let t0 = std::time::Instant::now();
    let mut peak = (0.0f32, 0.0f32, 0.0f32);
    let mut peak_f = (0u32, 0u32);
    let shots: Vec<u32> = std::env::var("SHOTS").ok().map(|v| v.split(',').filter_map(|x| x.parse().ok()).collect()).unwrap_or_default();
    for f in 0..frames {
        let mut enc = device.create_command_encoder(&Default::default());
        sim.frame(device, queue, &mut enc, &inp);
        queue.submit([enc.finish()]);
        device.poll(wgpu::Maintain::Poll);
        inp.scene = None;
        inp.ops.clear();
        inp.frame = f;
        let r = ((f as f32 + 1.0) * 20.0 * DT / 40.0).min(1.0);
        let r = r * r * (3.0 - 2.0 * r);
        inp.p_fuel = 1.0 + (sc.inlet_p - 1.0) * r;
        inp.p_o2 = 1.0 + (sc.o2_p - 1.0) * r;
        inp.time += 20.0 * DT;
        let st = sim.stats;
        if st.max_p > peak.0 { peak.0 = st.max_p; peak_f.0 = f; }
        if st.max_t > peak.1 { peak.1 = st.max_t; peak_f.1 = f; }
        peak.2 = peak.2.max(st.burn);
        if shots.contains(&f) {
            shot(&mut sim, &inp, name, &format!("{f}"), out, device, queue);
        }
        if f % every == 0 || f + 1 == frames {
            device.poll(wgpu::Maintain::Wait);
            let (s, m) = sim.read_state(device, queue);
            let (mut tmax, mut pmax, mut bad) = (0.0f32, 0.0f32, 0);
            for i in 0..(W * H) as usize {
                if matches!(m[i] & 0xff, 1 | 6 | 8) {
                    continue;
                }
                let c = &s[i * 8..i * 8 + 8];
                let rho = c[0] + c[1] + c[2] + c[3];
                if !rho.is_finite() || !c[6].is_finite() {
                    bad += 1;
                    continue;
                }
                let v2 = (c[4] * c[4] + c[5] * c[5]) / (rho * rho);
                let cv = c[0] * 5.827 + c[1] * 2.258 + c[2] * 2.580 + c[3] * 4.446;
                let rsum = c[0] * 1.80625 + c[1] * 0.903125 + c[2] * 1.032142857 + c[3] * 1.111538;
                let p = rsum / cv * (c[6] - 0.5 * v2 * rho);
                tmax = tmax.max(p / rsum);
                pmax = pmax.max(p);
            }
            if let Ok(spec) = std::env::var("WCELLS") {
                let wv = sim.read_wall(device, queue);
                for p in spec.split(',') {
                    let mut it = p.split(':');
                    let (x, y): (usize, usize) = (it.next().unwrap().parse().unwrap(), it.next().unwrap().parse().unwrap());
                    let k = y * W as usize + x;
                    let c = &s[k * 8..k * 8 + 8];
                    println!("  w({x},{y}) tile {} mat {} T {:.2} flag {:.2} code {:.1} rem {:.3} | gas {:.3?}", m[k] & 0xff, (m[k] >> 8) & 0xff, wv[k * 4], wv[k * 4 + 1], wv[k * 4 + 2], wv[k * 4 + 3], c);
                }
            }
            let walls = m.iter().filter(|&&x| x & 0xff == 1).count();
            let matter = m.iter().filter(|&&x| x & 0xff == 8).count();
            let st = sim.stats;
            println!(
                "[{name}] f{f:4} {:5.1}s Tmax {tmax:5.2} pmax {pmax:6.2} burn {:.3} wallheat {:.2} stress {:.2} walls {walls} matter {matter} bad {bad}",
                t0.elapsed().as_secs_f32(),
                st.burn,
                st.max_wall_heat,
                st.max_stress
            );
        }
    }
    println!("[{name}] PEAK (gpu stats) pmax {:.2} @f{} Tmax {:.2} @f{} burn {:.3}", peak.0, peak_f.0, peak.1, peak_f.1, peak.2);
    shot(&mut sim, &inp, name, "end", out, device, queue);
}

fn shot(sim: &mut GpuSim, inp_ref: &FrameInput, name: &str, tag: &str, out: &str, device: &wgpu::Device, queue: &wgpu::Queue) {
    let fmt = wgpu::TextureFormat::Rgba8Unorm;
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
    let mut inp = FrameInput { steps: 0, ops: vec![], scene: None, meta_only: None, probe: None, ..*inp_ref };
    sim.frame(device, queue, &mut enc, &inp);
    let _ = &mut inp;
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
    let buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (bpr * ph) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(bpr), rows_per_image: Some(ph) } },
        wgpu::Extent3d { width: pw, height: ph, depth_or_array_layers: 1 },
    );
    queue.submit([enc.finish()]);
    buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::Maintain::Wait);
    let data = buf.slice(..).get_mapped_range();
    let mut bytes = format!("P6\n{pw} {ph}\n255\n").into_bytes();
    for y in 0..ph {
        for x in 0..pw {
            let o = (y * bpr + x * 4) as usize;
            bytes.extend_from_slice(&data[o..o + 3]);
        }
    }
    std::fs::create_dir_all(out).unwrap();
    std::fs::write(format!("{out}/{name}_{tag}.ppm"), bytes).unwrap();
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    match a.get(1).map(|s| s.as_str()) {
        Some("export") => export(),
        Some("run") => {
            let which = a.get(2).cloned().unwrap_or("all".into());
            let frames: u32 = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(200);
            let out = a.get(4).cloned().unwrap_or("/tmp".into());
            let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
            let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).expect("adapter");
            let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default(), None)).expect("device");
            let names: Vec<&str> = if which == "all" { showcase::NAMES.to_vec() } else { which.split(',').collect() };
            for n in names {
                run(n, frames, &out, &device, &queue);
            }
        }
        _ => eprintln!("usage: showcase_test export | run <name|all|a,b> [frames] [out_dir]"),
    }
}
