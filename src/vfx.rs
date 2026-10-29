//! Rendering / VFX GPU plumbing (owned by the VFX side; gpu.rs only holds a `Vfx` and calls `frame`/`draw`).
//!
//! Per frame, after the simulation and vis.wgsl's colorize/bloom passes:
//!   1. `noisegen` (first frame only): 256x256 tileable noise -> OUT tail -> `noise` texture
//!   2. `fx` (fx.wgsl): packs 8 cell-resolution HDR layers into OUT (rgba16f), copied into the `layers` texture array
//!   3. `spark_update` + `spark_spawn` (particles.wgsl): sparks/embers ring buffer (Fire view)
//! `draw`: render.wgsl full-screen composite, then additive instanced spark streaks.
use bytemuck::{Pod, Zeroable};
use eframe::wgpu;

use crate::gpu::{DT, H, W};

/// Period (sim time units) of the two-phase flow-advected texture coordinates used for fire/smoke detail.
const FX_PERIOD: f32 = 6.0;
const LAYERS: u32 = 11;
/// detail simulation up-res factor (detail.wgsl): DS x the sim grid (env VFX_DS=2..4 overrides)
fn detail_scale() -> u32 {
    std::env::var("VFX_DS").ok().and_then(|v| v.parse().ok()).unwrap_or(4u32).clamp(1, 4)
}
const NOISE: u32 = 256;
/// mip levels of the noise texture (band-limited, generated directly by `noisegen`)
const NOISE_MIPS: u32 = 4;
/// spark ring-buffer size (must match NP in particles.wgsl)
const NP: u32 = 32768;
const SPARK_BYTES: u64 = 32;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FxParams {
    dt: f32,
    ph0: f32,
    ph1: f32,
    /// bit 0 / bit 1: reset layer 0 / 1 this frame
    reset: u32,
    /// which half of the ping-pong uv buffer is the source (0/1)
    src: u32,
    /// accumulated simulation time (freezes while paused)
    stime: f32,
    _p: [u32; 2],
}

pub struct Vfx {
    fxparams: wgpu::Buffer,
    out: wgpu::Buffer,
    layers: wgpu::Texture,
    noise: wgpu::Texture,
    fxbg: wgpu::BindGroup,
    p_fx: wgpu::ComputePipeline,
    p_noise: wgpu::ComputePipeline,
    pbg: wgpu::BindGroup,
    p_spawn: wgpu::ComputePipeline,
    p_update: wgpu::ComputePipeline,
    render: wgpu::RenderPipeline,
    rbg2: [wgpu::BindGroup; 2],
    dbg: [wgpu::BindGroup; 2],
    p_detail: wgpu::ComputePipeline,
    dcur: usize,
    dinit: bool,
    ds: u32,
    no_detail: bool,
    sbg0: wgpu::BindGroup,
    sbg1: wgpu::BindGroup,
    sparks: wgpu::RenderPipeline,
    noise_done: bool,
    fx_t: f32,
    fx_src: u32,
}

fn buf_entry(binding: u32, vis: wgpu::ShaderStages, ty: wgpu::BufferBindingType) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: vis,
        ty: wgpu::BindingType::Buffer { ty, has_dynamic_offset: false, min_binding_size: None },
        count: None,
    }
}
fn ro(binding: u32, vis: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    buf_entry(binding, vis, wgpu::BufferBindingType::Storage { read_only: true })
}
fn rw(binding: u32, vis: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    buf_entry(binding, vis, wgpu::BufferBindingType::Storage { read_only: false })
}
fn uni(binding: u32, vis: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    buf_entry(binding, vis, wgpu::BufferBindingType::Uniform)
}
fn bg(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, res: &[wgpu::BindingResource]) -> wgpu::BindGroup {
    let entries: Vec<wgpu::BindGroupEntry> = res.iter().enumerate().map(|(k, r)| wgpu::BindGroupEntry { binding: k as u32, resource: r.clone() }).collect();
    device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout, entries: &entries })
}

impl Vfx {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        device: &wgpu::Device,
        target: wgpu::TextureFormat,
        params: &wgpu::Buffer,
        u0: &wgpu::Buffer,
        u1: &wgpu::Buffer,
        u2: &wgpu::Buffer,
        meta: &wgpu::Buffer,
        wall: &wgpu::Buffer,
    ) -> Self {
        let n = (W * H) as u64;
        let ds = detail_scale();
        let c = wgpu::ShaderStages::COMPUTE;
        let f = wgpu::ShaderStages::FRAGMENT;
        let vf = wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT;
        let fxparams = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fxparams"),
            size: std::mem::size_of::<FxParams>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sbuf = |label: &str, size: u64| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            })
        };
        let uvbuf = sbuf("fx uv", n * 32);
        let out = sbuf("fx out", (LAYERS as u64 * n + (NOISE * NOISE * 2) as u64) * 8);
        let parts = sbuf("sparks", NP as u64 * SPARK_BYTES);
        let ctr = sbuf("spark ctr", 16);
        let pprev = sbuf("spark pprev", n * 4);
        let layers = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("fx layers"),
            size: wgpu::Extent3d { width: W, height: H, depth_or_array_layers: LAYERS },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let noise = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("fx noise"),
            size: wgpu::Extent3d { width: NOISE, height: NOISE, depth_or_array_layers: 1 },
            mip_level_count: NOISE_MIPS,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let layers_view = layers.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() });
        let noise_view = noise.create_view(&Default::default());
        let s_clamp = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("fx clamp"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let s_rep = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("fx repeat"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        // ---- fx compute (fx.wgsl): 0 P, 1 FP, 2 U0, 3 META, 4 U2, 5 uv (rw), 6 U1, 7 WALL, 8 OUT (rw)
        let fxbgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fx bgl"),
            entries: &[uni(0, c), uni(1, c), ro(2, c), ro(3, c), ro(4, c), rw(5, c), ro(6, c), ro(7, c), rw(8, c)],
        });
        let fxbg = bg(
            device,
            &fxbgl,
            &[
                params.as_entire_binding(),
                fxparams.as_entire_binding(),
                u0.as_entire_binding(),
                meta.as_entire_binding(),
                u2.as_entire_binding(),
                uvbuf.as_entire_binding(),
                u1.as_entire_binding(),
                wall.as_entire_binding(),
                out.as_entire_binding(),
            ],
        );
        let fx_mod = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("fx"), source: wgpu::ShaderSource::Wgsl(include_str!("fx.wgsl").into()) });
        let fxpl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[&fxbgl], push_constant_ranges: &[] });
        let cp = |module: &wgpu::ShaderModule, layout: &wgpu::PipelineLayout, entry: &str| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: Some(layout),
                module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let p_fx = cp(&fx_mod, &fxpl, "fx");
        let p_noise = cp(&fx_mod, &fxpl, "noisegen");

        // ---- sparks compute (particles.wgsl): 0 P, 1 FP, 2 U0, 3 META, 4 WALL, 5 U2, 6 PARTS, 7 CTR, 8 PPREV
        let pbgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("spark bgl"),
            entries: &[uni(0, c), uni(1, c), ro(2, c), ro(3, c), ro(4, c), ro(5, c), rw(6, c), rw(7, c), rw(8, c)],
        });
        let pbg = bg(
            device,
            &pbgl,
            &[
                params.as_entire_binding(),
                fxparams.as_entire_binding(),
                u0.as_entire_binding(),
                meta.as_entire_binding(),
                wall.as_entire_binding(),
                u2.as_entire_binding(),
                parts.as_entire_binding(),
                ctr.as_entire_binding(),
                pprev.as_entire_binding(),
            ],
        );
        let p_mod = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("sparks"), source: wgpu::ShaderSource::Wgsl(include_str!("particles.wgsl").into()) });
        let ppl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[&pbgl], push_constant_ranges: &[] });
        let p_spawn = cp(&p_mod, &ppl, "spark_spawn");
        let p_update = cp(&p_mod, &ppl, "spark_update");

        // ---- main composite (render.wgsl): 0 P, 1 FP, 2 layers, 3 noise, 4 clamp sampler, 5 repeat sampler
        let tex = |binding: u32, dim: wgpu::TextureViewDimension| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: f,
            ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: dim, multisampled: false },
            count: None,
        };
        let smp = |binding: u32| wgpu::BindGroupLayoutEntry { binding, visibility: f, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None };
        let rbgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("render bgl"),
            entries: &[uni(0, f), uni(1, f), tex(2, wgpu::TextureViewDimension::D2Array), tex(3, wgpu::TextureViewDimension::D2), smp(4), smp(5), tex(6, wgpu::TextureViewDimension::D2), tex(7, wgpu::TextureViewDimension::D2)],
        });
        // ---- detail up-res simulation (detail.wgsl): two ping-pong DS x textures
        let dtex: Vec<wgpu::Texture> = (0..2)
            .map(|_| {
                device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("detail"),
                    size: wgpu::Extent3d { width: W * ds, height: H * ds, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba16Float,
                    usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
            })
            .collect();
        let dview: Vec<wgpu::TextureView> = dtex.iter().map(|t| t.create_view(&Default::default())).collect();
        // chemistry colour tracers (detail.wgsl CSRC/CDST, render.wgsl binding 7): same size, ping-ponged with the detail field
        let ctex: Vec<wgpu::Texture> = (0..2)
            .map(|_| {
                device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("chem tracers"),
                    size: wgpu::Extent3d { width: W * ds, height: H * ds, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba16Float,
                    usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
            })
            .collect();
        let cview: Vec<wgpu::TextureView> = ctex.iter().map(|t| t.create_view(&Default::default())).collect();
        let rbg2 = [0usize, 1].map(|k| {
            bg(
                device,
                &rbgl,
                &[
                    params.as_entire_binding(),
                    fxparams.as_entire_binding(),
                    wgpu::BindingResource::TextureView(&layers_view),
                    wgpu::BindingResource::TextureView(&noise_view),
                    wgpu::BindingResource::Sampler(&s_clamp),
                    wgpu::BindingResource::Sampler(&s_rep),
                    wgpu::BindingResource::TextureView(&dview[k]),
                    wgpu::BindingResource::TextureView(&cview[k]),
                ],
            )
        });
        let ctexe = |binding: u32, dim: wgpu::TextureViewDimension| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: c,
            ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: dim, multisampled: false },
            count: None,
        };
        let csmp = |binding: u32| wgpu::BindGroupLayoutEntry { binding, visibility: c, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None };
        let dbgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("detail bgl"),
            entries: &[
                uni(0, c),
                uni(1, c),
                ctexe(2, wgpu::TextureViewDimension::D2Array),
                ctexe(3, wgpu::TextureViewDimension::D2),
                csmp(4),
                csmp(5),
                ctexe(6, wgpu::TextureViewDimension::D2),
                wgpu::BindGroupLayoutEntry {
                    binding: 7,
                    visibility: c,
                    ty: wgpu::BindingType::StorageTexture { access: wgpu::StorageTextureAccess::WriteOnly, format: wgpu::TextureFormat::Rgba16Float, view_dimension: wgpu::TextureViewDimension::D2 },
                    count: None,
                },
                ctexe(8, wgpu::TextureViewDimension::D2),
                wgpu::BindGroupLayoutEntry {
                    binding: 9,
                    visibility: c,
                    ty: wgpu::BindingType::StorageTexture { access: wgpu::StorageTextureAccess::WriteOnly, format: wgpu::TextureFormat::Rgba16Float, view_dimension: wgpu::TextureViewDimension::D2 },
                    count: None,
                },
            ],
        });
        // dbg[k]: reads detail k, writes detail 1-k
        let dbg = [0usize, 1].map(|k| {
            bg(
                device,
                &dbgl,
                &[
                    params.as_entire_binding(),
                    fxparams.as_entire_binding(),
                    wgpu::BindingResource::TextureView(&layers_view),
                    wgpu::BindingResource::TextureView(&noise_view),
                    wgpu::BindingResource::Sampler(&s_clamp),
                    wgpu::BindingResource::Sampler(&s_rep),
                    wgpu::BindingResource::TextureView(&dview[k]),
                    wgpu::BindingResource::TextureView(&dview[1 - k]),
                    wgpu::BindingResource::TextureView(&cview[k]),
                    wgpu::BindingResource::TextureView(&cview[1 - k]),
                ],
            )
        });
        let d_mod = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("detail"), source: wgpu::ShaderSource::Wgsl(include_str!("detail.wgsl").into()) });
        let dpl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[&dbgl], push_constant_ranges: &[] });
        let p_detail = cp(&d_mod, &dpl, "detail");
        let ren_mod = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("render"), source: wgpu::ShaderSource::Wgsl(include_str!("render.wgsl").into()) });
        let rpl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[&rbgl], push_constant_ranges: &[] });
        let render = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("render"),
            layout: Some(&rpl),
            vertex: wgpu::VertexState { module: &ren_mod, entry_point: Some("vs"), buffers: &[], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState { module: &ren_mod, entry_point: Some("fs"), targets: &[Some(target.into())], compilation_options: Default::default() }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // ---- spark streaks (particles.wgsl pvs/pfs): group 0 = P, FP; group 1 = PARTS (ro); additive blend
        let sbgl0 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("spark draw bgl0"), entries: &[uni(0, vf), uni(1, vf)] });
        let sbgl1 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("spark draw bgl1"), entries: &[ro(0, wgpu::ShaderStages::VERTEX)] });
        let sbg0 = bg(device, &sbgl0, &[params.as_entire_binding(), fxparams.as_entire_binding()]);
        let sbg1 = bg(device, &sbgl1, &[parts.as_entire_binding()]);
        let spl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[&sbgl0, &sbgl1], push_constant_ranges: &[] });
        let add = wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add };
        let keep = wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add };
        let sparks = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sparks"),
            layout: Some(&spl),
            vertex: wgpu::VertexState { module: &p_mod, entry_point: Some("pvs"), buffers: &[], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &p_mod,
                entry_point: Some("pfs"),
                targets: &[Some(wgpu::ColorTargetState { format: target, blend: Some(wgpu::BlendState { color: add, alpha: keep }), write_mask: wgpu::ColorWrites::ALL })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        Vfx {
            fxparams,
            out,
            layers,
            noise,
            fxbg,
            p_fx,
            p_noise,
            pbg,
            p_spawn,
            p_update,
            render,
            rbg2,
            dbg,
            p_detail,
            dcur: 0,
            dinit: false,
            ds,
            // timing A/B only (fire then renders from a stale detail field)
            no_detail: std::env::var("VFX_NODETAIL").is_ok(),
            sbg0,
            sbg1,
            sparks,
            noise_done: false,
            fx_t: 0.0,
            fx_src: 0,
        }
    }

    /// Encode the per-frame VFX passes (after the simulation and the vis.wgsl passes).
    pub fn frame(&mut self, queue: &wgpu::Queue, enc: &mut wgpu::CommandEncoder, steps: u32, reset_all: bool) {
        let (gx, gy) = (W.div_ceil(16), H.div_ceil(16));
        let dtf = steps as f32 * DT;
        let t0 = self.fx_t;
        self.fx_t += dtf;
        let t1 = self.fx_t;
        let mut reset = 0u32;
        for k in 0..2 {
            let o = 0.5 * k as f32;
            if (t1 / FX_PERIOD + o).floor() != (t0 / FX_PERIOD + o).floor() {
                reset |= 1 << k;
            }
        }
        if reset_all || !self.dinit {
            reset = 7;
            self.dinit = true;
        }
        let fp = FxParams {
            dt: dtf,
            ph0: (t1 / FX_PERIOD).fract(),
            ph1: (t1 / FX_PERIOD + 0.5).fract(),
            reset,
            src: self.fx_src,
            stime: t1,
            _p: [0; 2],
        };
        self.fx_src ^= 1;
        queue.write_buffer(&self.fxparams, 0, bytemuck::bytes_of(&fp));
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("fx"), timestamp_writes: None });
            pass.set_bind_group(0, &self.fxbg, &[]);
            if !self.noise_done {
                pass.set_pipeline(&self.p_noise);
                pass.dispatch_workgroups(NOISE / 16, 480 / 16, 1);
            }
            pass.set_pipeline(&self.p_fx);
            pass.dispatch_workgroups(gx, gy, 1);
            pass.set_bind_group(0, &self.pbg, &[]);
            pass.set_pipeline(&self.p_update);
            pass.dispatch_workgroups(NP / 256, 1, 1);
            pass.set_pipeline(&self.p_spawn);
            pass.dispatch_workgroups(gx, gy, 1);
        }
        let n = (W * H) as u64;
        enc.copy_buffer_to_texture(
            wgpu::TexelCopyBufferInfo { buffer: &self.out, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(W * 8), rows_per_image: Some(H) } },
            wgpu::TexelCopyTextureInfo { texture: &self.layers, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::Extent3d { width: W, height: H, depth_or_array_layers: LAYERS },
        );
        if !self.noise_done {
            let mut off = LAYERS as u64 * n * 8;
            for lvl in 0..NOISE_MIPS {
                let wl = NOISE >> lvl;
                enc.copy_buffer_to_texture(
                    wgpu::TexelCopyBufferInfo { buffer: &self.out, layout: wgpu::TexelCopyBufferLayout { offset: off, bytes_per_row: Some(wl * 8), rows_per_image: Some(wl) } },
                    wgpu::TexelCopyTextureInfo { texture: &self.noise, mip_level: lvl, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                    wgpu::Extent3d { width: wl, height: wl, depth_or_array_layers: 1 },
                );
                off += (wl * wl) as u64 * 8;
            }
            self.noise_done = true;
        }
        // detail up-res sim (needs this frame's layers): only while the sim advances (frozen when paused)
        if (steps > 0 || reset & 4 != 0) && !self.no_detail {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("detail"), timestamp_writes: None });
            pass.set_bind_group(0, &self.dbg[self.dcur], &[]);
            pass.set_pipeline(&self.p_detail);
            pass.dispatch_workgroups((W * self.ds).div_ceil(8), (H * self.ds).div_ceil(8), 1);
            self.dcur ^= 1;
        }
    }

    pub fn draw(&self, rp: &mut wgpu::RenderPass<'_>) {
        rp.set_pipeline(&self.render);
        rp.set_bind_group(0, &self.rbg2[self.dcur], &[]);
        rp.draw(0..3, 0..1);
        rp.set_pipeline(&self.sparks);
        rp.set_bind_group(0, &self.sbg0, &[]);
        rp.set_bind_group(1, &self.sbg1, &[]);
        rp.draw(0..6, 0..NP);
    }
}
