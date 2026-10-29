//! Headless campaign checker: parses assets/campaign/campaign.json, builds every mission's layout
//! with its known-good solution, runs it on the GPU simulation and scores the objectives with the
//! same rules as app.rs (smoothing, holds, keeps, end conditions).
//!
//!   cargo build --release --example campaign_check
//!   ./target/release/examples/campaign_check [mission ids or order numbers...]
//!   env: BARE=1 (run without the solution: the mission must NOT pass), LIST=1 (just parse + print),
//!        TL=s (timeline print period in s, default 2), SHOT=dir (end-of-run screenshot per mission),
//!        MAXT=s (cap the test duration)
#[path = "../src/gpu.rs"]
#[allow(dead_code)]
mod gpu;
#[path = "../src/campaign.rs"]
mod campaign;

use campaign::*;
use eframe::wgpu;
use gpu::{FrameInput, GpuSim, Op, DT, GRAVITY};

const TPS: f32 = 32.0;
const STEPS: u32 = 20;
const KN_PER_UNIT: f32 = 1.013;
const KGS_PER_UNIT: f32 = 3.5;

fn to_op(o: &OpData) -> Op {
    Op { kind: o.kind, shape: o.shape, x0: o.x0, y0: o.y0, x1: o.x1, y1: o.y1, r: o.r, tile: o.tile, mat: o.mat, dir: o.dir, _p: [0; 2] }
}

#[derive(Default, Clone)]
struct Counts {
    used: Vec<u32>,
    used_walls: u32,
}

fn count(layout: &[u32], initial: &[u32]) -> Counts {
    let mut c = Counts { used: vec![0; 128], used_walls: 0 };
    for (i, &v) in layout.iter().enumerate() {
        let t = v & 0xff;
        if t == T_MATTER {
            if initial[i] != v {
                c.used[((v >> 8) & 0xff).min(127) as usize] += 1;
            }
        } else if t == T_WALL && initial[i] != v {
            let m = ((v >> 8) & 0xff).min(9);
            c.used[m as usize] += 1;
            if m < 7 {
                c.used_walls += 1;
            }
        }
    }
    c
}

fn leaks(layout: &[u32], initial: &[u32]) -> bool {
    let (w, h) = (W as usize, H as usize);
    let passable = |v: u32| {
        let t = v & 0xff;
        !(t == T_WALL && ((v >> 8) & 0xff) < 7 || t == T_IGNITER)
    };
    let mut seen = vec![false; w * h];
    let mut stack = vec![];
    for (i, &v) in initial.iter().enumerate() {
        if v & 0xff == T_WALL && (v >> 8) & 0xff == 9 {
            seen[i] = true;
            stack.push(i);
        }
    }
    if stack.is_empty() {
        return false;
    }
    while let Some(i) = stack.pop() {
        let (x, y) = (i % w, i / w);
        if x == 0 || y == 0 || x == w - 1 || y == h - 1 || layout[i] & 0xff == T_VENT {
            return true;
        }
        for j in [i - 1, i + 1, i - w, i + w] {
            if !seen[j] && passable(layout[j]) {
                seen[j] = true;
                stack.push(j);
            }
        }
    }
    false
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum St {
    Pending,
    Done,
    Failed,
}

#[derive(Clone, Copy)]
struct Obj {
    st: St,
    best: f32,
    timer: f32,
    gap: f32,
}

struct Live {
    thrust: f32,
    burn: f32,
    impulse: f32,
    lost_mat: [u32; 10],
    lost_struct: u32,
    leak: bool,
    counts: Counts,
    present: Vec<u32>,
}

fn value(l: &Live, m: Metric, st: &gpu::Stats) -> f32 {
    match m {
        Metric::Thrust => l.thrust,
        Metric::Burn => l.burn,
        Metric::Isp => {
            if l.burn > 0.05 {
                l.thrust * 1000.0 / (l.burn * 5.0 * 9.81)
            } else {
                0.0
            }
        }
        Metric::WallHeat => st.max_wall_heat * 100.0,
        Metric::PeakTemp => st.max_t * 300.0,
        Metric::PeakPressure => st.max_p,
        Metric::StructLost => l.lost_struct as f32,
        Metric::MatLost(k) => l.lost_mat[(k as usize).min(9)] as f32,
        Metric::Used(k) => l.counts.used[(k as usize).min(127)] as f32,
        Metric::UsedWalls => l.counts.used_walls as f32,
        Metric::Leak => l.leak as u32 as f32,
        Metric::Impulse => l.impulse,
        Metric::Count(k) => l.present[(k as usize).min(127)] as f32,
    }
}

fn present(meta: &[u32]) -> Vec<u32> {
    let mut c = vec![0u32; 128];
    for &v in meta {
        let t = v & 0xff;
        if t == T_MATTER || t == T_WALL {
            c[((v >> 8) & 0xff).min(127) as usize] += 1;
        }
    }
    c
}

fn frame_input(meta: Option<Vec<u32>>, ops: Vec<Op>, ign: bool, m: &MissionDef) -> FrameInput {
    FrameInput {
        steps: STEPS,
        ops,
        view: m.view,
        p_fuel: 1.0,
        p_o2: 1.0,
        p_air: 1.0,
        fan_pow: 0.0,
        gravity: if m.gravity { GRAVITY } else { 0.0 },
        cam: [0.0, 0.0, W as f32, H as f32],
        ign_on: ign,
        time: 0.0,
        frame: 0,
        scene: meta,
        meta_only: None,
        reset_gas: false,
        probe: Some((0, 0)),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let camp = match load_from_str(CAMPAIGN_JSON) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("PARSE ERROR: {e}");
            std::process::exit(2);
        }
    };
    println!("campaign: {} chapters, {} missions", camp.chapters.len(), camp.missions.len());
    for c in &camp.chapters {
        let n = camp.missions.iter().filter(|m| m.chapter == c.id).count();
        println!("  chapter {} {:<28} {n} missions", c.id, c.title);
    }
    let sel: Vec<&MissionDef> = camp
        .missions
        .iter()
        .filter(|m| args.is_empty() || args.iter().any(|a| *a == m.id || a.parse::<u32>().ok() == Some(m.order)))
        .collect();
    if std::env::var("LIST").is_ok() {
        for m in &camp.missions {
            let s = build_solution(m);
            let init = build_scene(m);
            let c = count(&s.meta, &init.meta);
            println!(
                "{:2} [{}] {:<22} {:<26} obj {} pal {} layout {} sol {} used_walls {} ops {}",
                m.order, m.chapter, m.id, m.title, m.objectives.len(), m.palette.len(), m.layout.len(), m.solution.len(), c.used_walls, s.ops.len()
            );
            for (mt, v) in &m.budgets {
                let have = match mt {
                    Metric::Used(k) => c.used[*k as usize] as f32,
                    Metric::UsedWalls => c.used_walls as f32,
                    _ => 0.0,
                };
                if have > *v {
                    println!("     !! solution over budget {} {have} > {v}", metric_key(*mt));
                }
            }
        }
        return;
    }
    let bare = std::env::var("BARE").is_ok();
    let tl: f32 = std::env::var("TL").ok().and_then(|v| v.parse().ok()).unwrap_or(2.0);
    let maxt: f32 = std::env::var("MAXT").ok().and_then(|v| v.parse().ok()).unwrap_or(1e9);

    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).expect("adapter");
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default(), None)).expect("device");
    let fmt = wgpu::TextureFormat::Rgba8Unorm;
    let mut sim = GpuSim::new(&device, fmt);
    let mut summary = vec![];

    for m in sel {
        let t_wall = std::time::Instant::now();
        let init = build_scene(m);
        let sc = if bare || m.live { init.clone() } else { build_solution(m) };
        let counts = count(&sc.meta, &init.meta);
        // budgets
        for (mt, v) in &m.budgets {
            let have = match mt {
                Metric::Used(k) => counts.used[*k as usize] as f32,
                Metric::UsedWalls => counts.used_walls as f32,
                _ => 0.0,
            };
            if have > *v {
                println!("  !! over budget {} {have} > {v}", metric_key(*mt));
            }
        }
        let frame = |sim: &mut GpuSim, inp: &FrameInput| {
            let mut enc = device.create_command_encoder(&Default::default());
            sim.frame(&device, &queue, &mut enc, inp);
            queue.submit([enc.finish()]);
        };
        // build phase: tare the thrust reading on the still layout (ign off, no feeds, no fans)
        let mut tare = [0.0f32; 2];
        if !m.live && std::env::var("NOTARE").is_err() {
            let mut inp = frame_input(Some(sc.meta.clone()), vec![], false, m);
            for f in 0..90 {
                frame(&mut sim, &inp);
                inp.scene = None;
                if f >= 30 {
                    device.poll(wgpu::Maintain::Wait);
                    let r = sim.stats.thrust;
                    tare[0] += r[0] / 60.0;
                    tare[1] += r[1] / 60.0;
                }
            }
        }
        // test
        let mut inp = frame_input(Some(sc.meta.clone()), sc.ops.iter().map(to_op).collect(), true, m);
        let dt_game = STEPS as f32 * DT / TPS;
        let mut l = Live {
            thrust: 0.0,
            burn: 0.0,
            impulse: 0.0,
            lost_mat: [0; 10],
            lost_struct: 0,
            leak: leaks(&sc.meta, &init.meta),
            counts: counts.clone(),
            present: present(&sc.meta),
        };
        let mut obj: Vec<Obj> = m.objectives.iter().map(|_| Obj { st: St::Pending, best: 0.0, timer: 0.0, gap: 0.0 }).collect();
        for (k, o) in m.objectives.iter().enumerate() {
            if let Goal::AtMost(mt, v) = o.goal {
                if !m.live {
                    let x = value(&l, mt, &gpu::Stats::default());
                    obj[k].best = x;
                    obj[k].st = if x <= v { St::Done } else { St::Failed };
                }
            }
        }
        let start_layout = sc.meta.clone();
        let mut t = 0.0f32;
        let mut sim_time = 0.0f32;
        let mut solved = !m.live || bare;
        let mut next_tl = 0.0;
        let mut f = 0u32;
        let mut peak = (0.0f32, 0.0f32, 0.0f32, 0.0f32); // thrust, burn, p, T
        let dur = m.duration.min(maxt);
        let mut end_reason = "time";
        loop {
            let ramp = {
                let x = (sim_time / 40.0).clamp(0.0, 1.0);
                x * x * (3.0 - 2.0 * x)
            };
            inp.p_fuel = 1.0 + (m.feeds.fuel - 1.0) * ramp;
            inp.p_o2 = 1.0 + (m.feeds.o2 - 1.0) * ramp;
            inp.p_air = 1.0 + (m.feeds.air - 1.0) * ramp;
            inp.fan_pow = m.feeds.fan;
            inp.ign_on = t < m.igniter_secs;
            inp.frame = f;
            inp.time = sim_time;
            frame(&mut sim, &inp);
            inp.scene = None;
            inp.meta_only = None;
            inp.ops.clear();
            sim_time += STEPS as f32 * DT;
            f += 1;
            device.poll(wgpu::Maintain::Wait);
            let st = sim.stats;
            if std::env::var("TRACE").is_ok() {
                println!("    f {f} t {t:.2} T {:.0} p {:.2} burn {:.3} thrust {:.1},{:.1}", st.max_t * 300.0, st.max_p, st.burn, st.thrust[0], st.thrust[1]);
            }
            // live solution
            if !solved && t >= m.solve_at {
                solved = true;
                let cur = sim.read_meta(&device, &queue);
                let full = build_solution(m);
                let mut nm = cur.clone();
                for i in 0..nm.len() {
                    if full.meta[i] != init.meta[i] {
                        nm[i] = full.meta[i];
                    }
                }
                l.counts = count(&nm, &init.meta);
                inp.meta_only = Some(nm);
                let n0 = init.ops.len();
                inp.ops = full.ops[n0..].iter().map(to_op).collect();
            }
            let raw = [st.thrust[0] - tare[0], st.thrust[1] - tare[1]];
            let thrust_now = (raw[0] * raw[0] + raw[1] * raw[1]).sqrt() * KN_PER_UNIT;
            let burn_now = st.burn * KGS_PER_UNIT;
            let a = 1.0 - (-dt_game / 0.35f32).exp();
            l.thrust += (thrust_now - l.thrust) * a;
            l.burn += (burn_now - l.burn) * a;
            t += dt_game;
            l.impulse += thrust_now * dt_game;
            if f % 14 == 0 {
                let lay = sim.read_meta(&device, &queue);
                let mut lm = [0u32; 10];
                let mut ls = 0;
                for (a, b) in start_layout.iter().zip(lay.iter()) {
                    if a & 0xff == T_WALL && (a & 0xffff) != (b & 0xffff) {
                        let k = ((a >> 8) & 0xff).min(9) as usize;
                        lm[k] += 1;
                        if k < 7 {
                            ls += 1;
                        }
                    }
                }
                l.lost_mat = lm;
                l.lost_struct = ls;
                l.present = present(&lay);
                if m.objectives.iter().any(|o| matches!(o.goal, Goal::Keep(Metric::Leak, _))) {
                    l.leak = leaks(&lay, &init.meta);
                }
                if m.live {
                    l.counts = count(&lay, &init.meta);
                }
            }
            peak.0 = peak.0.max(l.thrust);
            peak.1 = peak.1.max(l.burn);
            peak.2 = peak.2.max(st.max_p);
            peak.3 = peak.3.max(st.max_t * 300.0);
            // objectives (same rules as app.rs tick_mission); stats are one frame late, so the
            // first frames still carry the previous mission's peaks: skip them
            for (k, o) in m.objectives.iter().enumerate() {
                let s = &mut obj[k];
                if s.st != St::Pending || f < 4 {
                    continue;
                }
                match o.goal {
                    Goal::Reach(mt, v) | Goal::ReachBy(mt, v, _) => {
                        let x = value(&l, mt, &st);
                        s.best = s.best.max(x);
                        if x >= v {
                            s.st = St::Done;
                        } else if let Goal::ReachBy(_, _, by) = o.goal {
                            if t > by {
                                s.st = St::Failed;
                            }
                        }
                    }
                    Goal::Hold(mt, v, secs) => {
                        let x = value(&l, mt, &st);
                        if x >= v {
                            s.timer += dt_game;
                            s.gap = 0.0;
                        } else {
                            s.gap += dt_game;
                            if s.gap > 0.6 {
                                s.timer = 0.0;
                            }
                        }
                        s.best = s.best.max(x);
                        if s.timer >= secs {
                            s.st = St::Done;
                        }
                    }
                    Goal::Keep(mt, v) | Goal::AtMost(mt, v) => {
                        let x = value(&l, mt, &st);
                        s.best = s.best.max(x);
                        if x > v {
                            s.st = St::Failed;
                        }
                    }
                    Goal::HoldBelow(mt, v, secs) => {
                        let x = value(&l, mt, &st);
                        s.best = s.best.max(x);
                        if s.best > v * 3.0 && x <= v {
                            s.timer += dt_game;
                        } else if x > v {
                            s.timer = 0.0;
                        }
                        if s.timer >= secs {
                            s.st = St::Done;
                        }
                    }
                }
            }
            if t >= next_tl {
                next_tl += tl;
                if std::env::var("DIAG").is_ok() {
                    let (s, mm) = sim.read_state(&device, &queue);
                    let mut tmax = 0.0f32;
                    for i in 0..(W * H) as usize {
                        if matches!(mm[i] & 0xff, 1 | 6 | 8) { continue; }
                        let c = &s[i * 8..i * 8 + 8];
                        let rho = c[0] + c[1] + c[2] + c[3];
                        if !rho.is_finite() || rho <= 0.0 { continue; }
                        let v2 = (c[4] * c[4] + c[5] * c[5]) / (rho * rho);
                        let cv = c[0] * 5.827 + c[1] * 2.258 + c[2] * 2.580 + c[3] * 4.446;
                        let rsum = c[0] * 1.80625 + c[1] * 0.903125 + c[2] * 1.032142857 + c[3] * 1.111538;
                        let p = rsum / cv * (c[6] - 0.5 * v2 * rho);
                        tmax = tmax.max(p / rsum);
                    }
                    println!("    diag: state Tmax {:.0} K", tmax * 300.0);
                }
                let counts_s: String = m
                    .objectives
                    .iter()
                    .filter_map(|o| match o.goal.metric() {
                        Metric::Count(k) => Some(format!(" n{}={}", element_name(k), l.present[k as usize])),
                        _ => None,
                    })
                    .collect();
                let counts_s = counts_s
                    + &std::env::var("COUNTS")
                        .map(|v| v.split(',').filter_map(|n| element_id(n)).map(|k| format!(" {}={}", element_name(k), l.present[k as usize])).collect::<String>())
                        .unwrap_or_default();
                println!(
                    "    t {t:5.1}  thrust {:6.1} kN  burn {:5.2}  p {:5.2}  T {:5.0} K  wallheat {:4.0}%  lost {}  {:?}{counts_s}",
                    l.thrust,
                    l.burn,
                    st.max_p,
                    st.max_t * 300.0,
                    st.max_wall_heat * 100.0,
                    l.lost_mat.iter().enumerate().filter(|(_, &v)| v > 0).map(|(k, v)| format!("{}:{v}", element_name(k as u32))).collect::<Vec<_>>().join(","),
                    obj.iter().map(|o| match o.st { St::Pending => '.', St::Done => '+', St::Failed => 'x' }).collect::<String>()
                );
            }
            let required_failed = m.objectives.iter().zip(&obj).any(|(o, s)| o.required && s.st == St::Failed);
            let all_resolved = obj.iter().all(|s| s.st != St::Pending);
            let has_active = m.objectives.iter().any(|o| !matches!(o.goal, Goal::Keep(..) | Goal::AtMost(..)));
            let active_resolved = m.objectives.iter().zip(&obj).all(|(o, s)| matches!(o.goal, Goal::Keep(..) | Goal::AtMost(..)) || s.st != St::Pending);
            if required_failed {
                end_reason = "required failed";
            } else if all_resolved || (has_active && active_resolved) {
                end_reason = "resolved";
            }
            if required_failed || all_resolved || (has_active && active_resolved) || t >= dur {
                break;
            }
        }
        // at the end, keeps / at_mosts still pending count as met
        let met: Vec<bool> = m
            .objectives
            .iter()
            .zip(&obj)
            .map(|(o, s)| s.st == St::Done || (s.st == St::Pending && matches!(o.goal, Goal::Keep(..) | Goal::AtMost(..))))
            .collect();
        let stars = stars_for(m, &met);
        println!(
            "{} #{:2} {:<20} {} stars {}  ({end_reason} at t={t:.1}s, {:.1}s wall)  peak thrust {:.1} burn {:.2} p {:.1} T {:.0}",
            if stars > 0 { "PASS" } else { "FAIL" },
            m.order,
            m.id,
            "*".repeat(stars as usize),
            stars,
            t_wall.elapsed().as_secs_f32(),
            peak.0,
            peak.1,
            peak.2,
            peak.3
        );
        for (k, o) in m.objectives.iter().enumerate() {
            println!("      [{}] {:<44} best {:8.2}  {:?}", if o.required { "R" } else { "*" }, o.label(), obj[k].best, if met[k] { St::Done } else { obj[k].st });
        }
        summary.push(format!("{} #{:2} {:<22} {}", if stars > 0 { "PASS" } else { "FAIL" }, m.order, m.id, stars));
        if let Ok(dir) = std::env::var("SHOT") {
            shot(&device, &queue, &mut sim, &inp, &format!("{dir}/{:02}_{}.ppm", m.order, m.id));
        }
    }
    println!("---- summary{}", if bare { " (BARE: no solution)" } else { "" });
    for s in summary {
        println!("{s}");
    }
}

fn shot(device: &wgpu::Device, queue: &wgpu::Queue, sim: &mut GpuSim, inp: &FrameInput, path: &str) {
    let (pw, ph) = (W as u32 * 2, H as u32 * 2);
    let fmt = wgpu::TextureFormat::Rgba8Unorm;
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
    let mut i2 = inp_clone(inp);
    i2.steps = 0;
    sim.frame(device, queue, &mut enc, &i2);
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
    let out = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: (bpr * ph) as u64, usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        wgpu::TexelCopyBufferInfo { buffer: &out, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(bpr), rows_per_image: Some(ph) } },
        wgpu::Extent3d { width: pw, height: ph, depth_or_array_layers: 1 },
    );
    queue.submit([enc.finish()]);
    out.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::Maintain::Wait);
    let data = out.slice(..).get_mapped_range();
    let mut bytes = format!("P6\n{pw} {ph}\n255\n").into_bytes();
    for y in 0..ph {
        for x in 0..pw {
            let o = (y * bpr + x * 4) as usize;
            bytes.extend_from_slice(&data[o..o + 3]);
        }
    }
    let _ = std::fs::write(path, bytes);
}

fn inp_clone(i: &FrameInput) -> Box<FrameInput> {
    Box::new(FrameInput {
        steps: i.steps,
        ops: vec![],
        view: i.view,
        p_fuel: i.p_fuel,
        p_o2: i.p_o2,
        p_air: i.p_air,
        fan_pow: i.fan_pow,
        gravity: i.gravity,
        cam: i.cam,
        ign_on: i.ign_on,
        time: i.time,
        frame: i.frame,
        scene: None,
        meta_only: None,
        reset_gas: false,
        probe: i.probe,
    })
}
