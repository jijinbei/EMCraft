//! The pipelines and the passes of a frame.
//!
//! A frame is drawn offscreen at the 3D view's size times its supersampling, `samples`-fold
//! multisampled, with a depth and stencil buffer:
//!
//! 1. the ground, the opaque triangles (depth written), the lines (depth tested, not written,
//!    moved towards the eye so edges on faces show);
//! 2. the see-through triangles into the accumulation and revealage targets, against the opaque
//!    depth (weighted blended order-independent transparency, McGuire and Bavoil, *Weighted
//!    Blended Order-Independent Transparency*, JCGT 2(2), 2013): nothing is sorted;
//! 3. their weighted average laid over the frame; then their depth written where they are
//!    nearer than the opaque, marking the stencil, and the lines drawn again only there and only
//!    where they lie in front of them, so an edge on a see-through face is drawn over it, not
//!    washed out by it, as the CPU renderer's back-to-front order draws it.
//!
//! The resolved frame is then shown in the window's pass, each pixel the mean of its
//! supersamples.

use std::num::NonZeroU64;

use eframe::wgpu;
use eframe::wgpu::util::DeviceExt;

use super::gbuffer;
use super::pack::{LINE, Packed, TRI_VERTEX, UNIFORM_BYTES, Uniforms};
use super::{Frame, Gpu3dError};

/// The frame's colour: the bytes' scale, blended there as the CPU renderer blends.
pub const FRAME: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth24PlusStencil8;
const ACCUM: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const REVEAL: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;
/// Bytes of the show pass's uniform.
const SHOW_BYTES: u64 = 128;

pub(super) const SCENE_WGSL: &str = include_str!("scene.wgsl");
pub(super) const COMPOSITE_WGSL: &str = concat!(include_str!("full.wgsl"), include_str!("composite.wgsl"));
pub(super) const SHOW_WGSL: &str = concat!(include_str!("full.wgsl"), include_str!("show.wgsl"));

/// What the device offers the renderer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Caps {
    /// Samples per pixel (4 where every target format takes them, else 1).
    pub samples: u32,
    /// Float targets blend: order-independent transparency (else see-through faces blend in
    /// the scene's order).
    pub oit: bool,
    /// The largest texture side.
    pub max_side: u32,
    /// The largest buffer (bytes).
    pub max_buffer: u64,
}

impl Caps {
    /// What `adapter` and `device` allow.
    pub fn of(adapter: &wgpu::Adapter, device: &wgpu::Device) -> Caps {
        let f = |format| adapter.get_texture_format_features(format).flags;
        let four = [FRAME, DEPTH, ACCUM, REVEAL].iter().all(|&t| f(t).sample_count_supported(4));
        let oit = [ACCUM, REVEAL].iter().all(|&t| f(t).contains(wgpu::TextureFormatFeatureFlags::BLENDABLE));
        let limits = device.limits();
        Caps { samples: if four { 4 } else { 1 }, oit, max_side: limits.max_texture_dimension_2d, max_buffer: limits.max_buffer_size }
    }
}

struct Targets {
    size: [u32; 2],
    color: wgpu::TextureView,
    /// The single-sample frame the window shows (the colour itself without multisampling).
    resolved: Option<wgpu::TextureView>,
    depth: wgpu::TextureView,
    accum: Option<(wgpu::TextureView, Option<wgpu::TextureView>)>,
    reveal: Option<(wgpu::TextureView, Option<wgpu::TextureView>)>,
    composite: Option<wgpu::BindGroup>,
}

struct SceneBuffers {
    id: u64,
    opaque: Option<(wgpu::Buffer, u32)>,
    glass: Option<(wgpu::Buffer, u32)>,
    lines: Option<(wgpu::Buffer, u32)>,
}

pub struct Renderer {
    caps: Caps,
    /// The window's format is sRGB: the shown colour is decoded first.
    srgb: bool,
    uniform: wgpu::Buffer,
    uniform_bind: wgpu::BindGroup,
    ground: wgpu::RenderPipeline,
    opaque: wgpu::RenderPipeline,
    lines: wgpu::RenderPipeline,
    /// See-through triangles: into the OIT targets, or blended into the frame.
    glass: wgpu::RenderPipeline,
    glass_depth: wgpu::RenderPipeline,
    front_lines: wgpu::RenderPipeline,
    composite: Option<(wgpu::RenderPipeline, wgpu::BindGroupLayout)>,
    show: wgpu::RenderPipeline,
    show_layout: wgpu::BindGroupLayout,
    show_uniform: wgpu::Buffer,
    targets: Option<Targets>,
    scene: Option<SceneBuffers>,
    g: gbuffer::Pipelines,
    /// What lies under each pixel, at the window's resolution.
    g_targets: Option<gbuffer::Targets>,
    /// The show pass's bindings, for the frame's and the per-pixel targets' sizes.
    show_bind: Option<([u32; 4], wgpu::BindGroup)>,
    /// The frame drawn last ([`Frame::key`]), which the per-pixel targets hold.
    pub last: Option<u64>,
}

/// Runs `make` under error scopes: a validation or out-of-memory error is returned, not raised.
pub fn scoped<T>(device: &wgpu::Device, make: impl FnOnce() -> T) -> Result<T, String> {
    let scopes = [wgpu::ErrorFilter::OutOfMemory, wgpu::ErrorFilter::Internal, wgpu::ErrorFilter::Validation].map(|f| device.push_error_scope(f));
    let out = make();
    let mut failed = None;
    for scope in scopes.into_iter().rev() {
        if let Some(e) = super::ready(scope.pop()).flatten() {
            failed.get_or_insert(e.to_string());
        }
    }
    match failed {
        Some(e) => Err(e),
        None => Ok(out),
    }
}

fn tri_layout() -> wgpu::VertexBufferLayout<'static> {
    const A: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4, 2 => Unorm8x4, 3 => Uint32];
    wgpu::VertexBufferLayout { array_stride: TRI_VERTEX as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &A }
}

fn line_layout() -> wgpu::VertexBufferLayout<'static> {
    const A: [wgpu::VertexAttribute; 3] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Unorm8x4];
    wgpu::VertexBufferLayout { array_stride: LINE as u64, step_mode: wgpu::VertexStepMode::Instance, attributes: &A }
}

fn alpha_over() -> wgpu::BlendState {
    wgpu::BlendState::ALPHA_BLENDING
}

fn depth(write: bool, compare: wgpu::CompareFunction, stencil: wgpu::StencilState) -> Option<wgpu::DepthStencilState> {
    Some(wgpu::DepthStencilState { format: DEPTH, depth_write_enabled: Some(write), depth_compare: Some(compare), stencil, bias: Default::default() })
}

fn stencil(compare: wgpu::CompareFunction, pass: wgpu::StencilOperation) -> wgpu::StencilState {
    let face = wgpu::StencilFaceState { compare, fail_op: wgpu::StencilOperation::Keep, depth_fail_op: wgpu::StencilOperation::Keep, pass_op: pass };
    wgpu::StencilState { front: face, back: face, read_mask: 0xff, write_mask: 0xff }
}

fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: false },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn uniform_entry(binding: u32, visibility: wgpu::ShaderStages, size: u64) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: NonZeroU64::new(size) },
        count: None,
    }
}

impl Renderer {
    /// The pipelines for a window of `window` format (its pass single-sampled, no depth).
    pub fn new(device: &wgpu::Device, caps: Caps, window: wgpu::TextureFormat) -> Result<Renderer, Gpu3dError> {
        scoped(device, || Renderer::make(device, caps, window)).map_err(Gpu3dError::Pipeline)
    }

    fn make(device: &wgpu::Device, caps: Caps, window: wgpu::TextureFormat) -> Renderer {
        let module = |label, src: &str| {
            device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(label), source: wgpu::ShaderSource::Wgsl(src.into()) })
        };
        let scene = module("emcraft_view3d", SCENE_WGSL);
        let composite_module = module("emcraft_view3d_composite", COMPOSITE_WGSL);
        let show_module = module("emcraft_view3d_show", SHOW_WGSL);
        let uniform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("emcraft_view3d"),
            entries: &[uniform_entry(0, wgpu::ShaderStages::VERTEX_FRAGMENT, UNIFORM_BYTES as u64)],
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("emcraft_view3d"),
            size: UNIFORM_BYTES as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let uniform_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("emcraft_view3d"),
            layout: &uniform_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("emcraft_view3d"),
            bind_group_layouts: &[Some(&uniform_layout)],
            immediate_size: 0,
        });
        let multisample = wgpu::MultisampleState { count: caps.samples, mask: !0, alpha_to_coverage_enabled: false };
        let none = wgpu::StencilState::default();
        let frame = |blend: Option<wgpu::BlendState>, write_mask| [Some(wgpu::ColorTargetState { format: FRAME, blend, write_mask })];
        let pipeline = |label: &str,
                        vs: &str,
                        buffers: &[Option<wgpu::VertexBufferLayout<'_>>],
                        fs: &str,
                        targets: &[Option<wgpu::ColorTargetState>],
                        depth_stencil: Option<wgpu::DepthStencilState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState { module: &scene, entry_point: Some(vs), buffers, compilation_options: Default::default() },
                primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: None, ..Default::default() },
                depth_stencil,
                multisample,
                fragment: Some(wgpu::FragmentState { module: &scene, entry_point: Some(fs), targets, compilation_options: Default::default() }),
                multiview_mask: None,
                cache: None,
            })
        };
        use wgpu::CompareFunction::{Always, Equal, Less, LessEqual};
        let all = wgpu::ColorWrites::ALL;
        let ground = pipeline("emcraft_view3d_ground", "vs_ground", &[], "fs_ground", &frame(None, all), depth(false, Always, none.clone()));
        let opaque =
            pipeline("emcraft_view3d_opaque", "vs_tri", &[Some(tri_layout())], "fs_opaque", &frame(None, all), depth(true, Less, none.clone()));
        let lines = pipeline(
            "emcraft_view3d_lines",
            "vs_line",
            &[Some(line_layout())],
            "fs_line",
            &frame(Some(alpha_over()), all),
            depth(false, LessEqual, none.clone()),
        );
        let glass = if caps.oit {
            let add =
                wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add };
            let keep = wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Zero,
                dst_factor: wgpu::BlendFactor::OneMinusSrc,
                operation: wgpu::BlendOperation::Add,
            };
            let targets = [
                Some(wgpu::ColorTargetState { format: ACCUM, blend: Some(wgpu::BlendState { color: add, alpha: add }), write_mask: all }),
                Some(wgpu::ColorTargetState {
                    format: REVEAL,
                    blend: Some(wgpu::BlendState { color: keep, alpha: keep }),
                    write_mask: wgpu::ColorWrites::RED,
                }),
            ];
            pipeline("emcraft_view3d_oit", "vs_tri", &[Some(tri_layout())], "fs_oit", &targets, depth(false, LessEqual, none.clone()))
        } else {
            pipeline(
                "emcraft_view3d_glass",
                "vs_tri",
                &[Some(tri_layout())],
                "fs_blend",
                &frame(Some(alpha_over()), all),
                depth(false, LessEqual, none.clone()),
            )
        };
        let glass_depth = pipeline(
            "emcraft_view3d_glass_depth",
            "vs_tri",
            &[Some(tri_layout())],
            "fs_none",
            &frame(None, wgpu::ColorWrites::empty()),
            depth(true, Less, stencil(Always, wgpu::StencilOperation::Replace)),
        );
        let front_lines = pipeline(
            "emcraft_view3d_front_lines",
            "vs_line",
            &[Some(line_layout())],
            "fs_line",
            &frame(Some(alpha_over()), all),
            depth(false, LessEqual, stencil(Equal, wgpu::StencilOperation::Keep)),
        );
        let composite = caps.oit.then(|| {
            let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("emcraft_view3d_composite"),
                entries: &[texture_entry(0), texture_entry(1)],
            });
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("emcraft_view3d_composite"),
                bind_group_layouts: &[Some(&bgl)],
                immediate_size: 0,
            });
            let p = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("emcraft_view3d_composite"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &composite_module,
                    entry_point: Some("vs"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                primitive: Default::default(),
                depth_stencil: depth(false, Always, wgpu::StencilState::default()),
                multisample,
                fragment: Some(wgpu::FragmentState {
                    module: &composite_module,
                    entry_point: Some("fs_composite"),
                    targets: &frame(Some(alpha_over()), all),
                    compilation_options: Default::default(),
                }),
                multiview_mask: None,
                cache: None,
            });
            (p, bgl)
        });
        let show_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("emcraft_view3d_show"),
            entries: &[
                texture_entry(0),
                uniform_entry(1, wgpu::ShaderStages::FRAGMENT, SHOW_BYTES),
                texture_entry(2),
                texture_entry(3),
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Uint,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let show_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("emcraft_view3d_show"),
            bind_group_layouts: &[Some(&show_layout)],
            immediate_size: 0,
        });
        let show = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("emcraft_view3d_show"),
            layout: Some(&show_pipeline_layout),
            vertex: wgpu::VertexState { module: &show_module, entry_point: Some("vs"), buffers: &[], compilation_options: Default::default() },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &show_module,
                entry_point: Some("fs_show"),
                targets: &[Some(wgpu::ColorTargetState { format: window, blend: None, write_mask: all })],
                compilation_options: Default::default(),
            }),
            multiview_mask: None,
            cache: None,
        });
        let show_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("emcraft_view3d_show"),
            size: SHOW_BYTES,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let g = gbuffer::Pipelines::new(device, &scene, &layout, &[Some(tri_layout())]);
        Renderer {
            g,
            g_targets: None,
            show_bind: None,
            last: None,
            caps,
            srgb: window.is_srgb(),
            uniform,
            uniform_bind,
            ground,
            opaque,
            lines,
            glass,
            glass_depth,
            front_lines,
            composite,
            show,
            show_layout,
            show_uniform,
            targets: None,
            scene: None,
        }
    }

    fn upload(&mut self, device: &wgpu::Device, p: &Packed) -> Result<(), Gpu3dError> {
        if self.scene.as_ref().is_some_and(|s| s.id == p.id) {
            return Ok(());
        }
        self.scene = None;
        let max = self.caps.max_buffer;
        let buffer = |label, bytes: &[u8], count: u32| {
            (!bytes.is_empty() && (bytes.len() as u64) <= max).then(|| {
                let b = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(label),
                    contents: bytes,
                    usage: wgpu::BufferUsages::VERTEX,
                });
                (b, count)
            })
        };
        let s = scoped(device, || SceneBuffers {
            id: p.id,
            opaque: buffer("emcraft_view3d_opaque", &p.opaque, p.opaque_vertices()),
            glass: buffer("emcraft_view3d_glass", &p.glass, p.glass_vertices()),
            lines: buffer("emcraft_view3d_lines", &p.lines, p.line_count()),
        })
        .map_err(Gpu3dError::Frame)?;
        self.scene = Some(s);
        Ok(())
    }

    fn targets(&mut self, device: &wgpu::Device, size: [u32; 2]) -> Result<(), Gpu3dError> {
        if self.targets.as_ref().is_some_and(|t| t.size == size) {
            return Ok(());
        }
        self.targets = None;
        let samples = self.caps.samples;
        let t = scoped(device, || {
            let texture = |label: &str, format, samples, usage| {
                device
                    .create_texture(&wgpu::TextureDescriptor {
                        label: Some(label),
                        size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
                        mip_level_count: 1,
                        sample_count: samples,
                        dimension: wgpu::TextureDimension::D2,
                        format,
                        usage,
                        view_formats: &[],
                    })
                    .create_view(&Default::default())
            };
            let read = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC;
            let pair = |label: &str, format| {
                if samples > 1 {
                    (texture(label, format, samples, wgpu::TextureUsages::RENDER_ATTACHMENT), Some(texture(label, format, 1, read)))
                } else {
                    (texture(label, format, 1, read), None)
                }
            };
            let (color, resolved) = pair("emcraft_view3d_frame", FRAME);
            let depth = texture("emcraft_view3d_depth", DEPTH, samples, wgpu::TextureUsages::RENDER_ATTACHMENT);
            let (accum, reveal) =
                if self.caps.oit { (Some(pair("emcraft_view3d_accum", ACCUM)), Some(pair("emcraft_view3d_reveal", REVEAL))) } else { (None, None) };
            let single = |p: &(wgpu::TextureView, Option<wgpu::TextureView>)| p.1.clone().unwrap_or_else(|| p.0.clone());
            let composite = match (&self.composite, &accum, &reveal) {
                (Some((_, bgl)), Some(a), Some(r)) => Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("emcraft_view3d_composite"),
                    layout: bgl,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&single(a)) },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&single(r)) },
                    ],
                })),
                _ => None,
            };
            Targets { size, color, resolved, depth, accum, reveal, composite }
        })
        .map_err(Gpu3dError::Frame)?;
        self.targets = Some(t);
        Ok(())
    }

    /// The per-pixel targets at the window's `size`, and the show pass's bindings.
    fn g_targets(&mut self, device: &wgpu::Device, out: [u32; 2]) -> Result<(), Gpu3dError> {
        if self.g_targets.as_ref().is_none_or(|t| t.size != out) {
            self.g_targets = None;
            self.g_targets = Some(scoped(device, || gbuffer::Pipelines::targets(device, out)).map_err(Gpu3dError::Frame)?);
        }
        let (Some(t), Some(g)) = (&self.targets, &self.g_targets) else { return Err(Gpu3dError::Frame("no targets".into())) };
        let key = [t.size[0], t.size[1], out[0], out[1]];
        if self.show_bind.as_ref().is_some_and(|(k, _)| *k == key) {
            return Ok(());
        }
        let shown = t.resolved.clone().unwrap_or_else(|| t.color.clone());
        let ids = g.any.create_view(&Default::default());
        let bind = scoped(device, || {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("emcraft_view3d_show"),
                layout: &self.show_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&shown) },
                    wgpu::BindGroupEntry { binding: 1, resource: self.show_uniform.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&g.normal) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&g.depth) },
                    wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&ids) },
                ],
            })
        })
        .map_err(Gpu3dError::Frame)?;
        self.show_bind = Some((key, bind));
        Ok(())
    }

    /// The tags under pixel `px` of the frame drawn last, if that frame was `key` and drew them.
    pub fn tags_at(&self, device: &wgpu::Device, queue: &wgpu::Queue, key: u64, px: [u32; 2]) -> Option<(u32, u32)> {
        if self.last != Some(key) {
            return None;
        }
        gbuffer::read_tags(device, queue, self.g_targets.as_ref()?, px)
    }

    /// Draws `frame` offscreen, recorded into `encoder`.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        frame: &Frame,
    ) -> Result<(), Gpu3dError> {
        let side = self.caps.max_side.max(1);
        let size = [frame.size[0].clamp(1, side), frame.size[1].clamp(1, side)];
        let out = [frame.out[0].clamp(1, side), frame.out[1].clamp(1, side)];
        self.last = None;
        self.upload(device, &frame.packed)?;
        self.targets(device, size)?;
        self.g_targets(device, out)?;
        let (Some(t), Some(s), Some(g)) = (&self.targets, &self.scene, &self.g_targets) else { return Err(Gpu3dError::Frame("no targets".into())) };
        let u = Uniforms { viewport: [size[0] as f32, size[1] as f32, frame.uniforms.viewport[2], frame.uniforms.viewport[3]], ..frame.uniforms };
        queue.write_buffer(&self.uniform, 0, &u.bytes());
        queue.write_buffer(&self.show_uniform, 0, &frame.show_bytes(self.srgb));
        let glass = s.glass.as_ref();
        let oit = glass.is_some() && self.caps.oit;
        let look = frame.look;
        if look.edges || look.ao || look.outline || look.pick {
            let draw = |pass: &mut wgpu::RenderPass<'_>, opaque_only: bool| {
                for (b, n) in s.opaque.iter().chain(if opaque_only { None } else { glass }) {
                    pass.set_vertex_buffer(0, b.slice(..));
                    pass.draw(0..*n, 0..1);
                }
            };
            self.g.record(encoder, g, &self.uniform_bind, look.edges || look.ao, look.outline || look.pick, &draw);
        }
        let color = |load| {
            Some(wgpu::RenderPassColorAttachment {
                view: &t.color,
                depth_slice: None,
                resolve_target: t.resolved.as_ref(),
                ops: wgpu::Operations { load, store: wgpu::StoreOp::Store },
            })
        };
        let depth_stencil = |clear: bool| {
            Some(wgpu::RenderPassDepthStencilAttachment {
                view: &t.depth,
                depth_ops: Some(wgpu::Operations {
                    load: if clear { wgpu::LoadOp::Clear(1.0) } else { wgpu::LoadOp::Load },
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: Some(wgpu::Operations {
                    load: if clear { wgpu::LoadOp::Clear(0) } else { wgpu::LoadOp::Load },
                    store: wgpu::StoreOp::Store,
                }),
            })
        };
        let lines = |pass: &mut wgpu::RenderPass<'_>, pipeline: &wgpu::RenderPipeline| {
            if let Some((b, n)) = &s.lines {
                pass.set_pipeline(pipeline);
                pass.set_vertex_buffer(0, b.slice(..));
                pass.draw(0..6, 0..*n);
            }
        };
        let tris = |pass: &mut wgpu::RenderPass<'_>, pipeline: &wgpu::RenderPipeline, of: Option<&(wgpu::Buffer, u32)>| {
            if let Some((b, n)) = of {
                pass.set_pipeline(pipeline);
                pass.set_vertex_buffer(0, b.slice(..));
                pass.draw(0..*n, 0..1);
            }
        };
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("emcraft_view3d"),
                color_attachments: &[color(wgpu::LoadOp::Clear(wgpu::Color::BLACK))],
                depth_stencil_attachment: depth_stencil(true),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.uniform_bind, &[]);
            pass.set_pipeline(&self.ground);
            pass.draw(0..3, 0..1);
            tris(&mut pass, &self.opaque, s.opaque.as_ref());
            lines(&mut pass, &self.lines);
            if !oit {
                tris(&mut pass, &self.glass, glass);
                if glass.is_some() {
                    pass.set_stencil_reference(1);
                    tris(&mut pass, &self.glass_depth, glass);
                    lines(&mut pass, &self.front_lines);
                }
            }
        }
        let key = frame.key();
        if !oit {
            self.last = Some(key);
            return Ok(());
        }
        let (Some(accum), Some(reveal), Some((composite, _)), Some(bind)) = (&t.accum, &t.reveal, &self.composite, &t.composite) else {
            return Err(Gpu3dError::Frame("no transparency targets".into()));
        };
        {
            fn attach(p: &(wgpu::TextureView, Option<wgpu::TextureView>), clear: wgpu::Color) -> Option<wgpu::RenderPassColorAttachment<'_>> {
                Some(wgpu::RenderPassColorAttachment {
                    view: &p.0,
                    depth_slice: None,
                    resolve_target: p.1.as_ref(),
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(clear), store: wgpu::StoreOp::Store },
                })
            }
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("emcraft_view3d_oit"),
                color_attachments: &[attach(accum, wgpu::Color::TRANSPARENT), attach(reveal, wgpu::Color::WHITE)],
                depth_stencil_attachment: depth_stencil(false),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.uniform_bind, &[]);
            tris(&mut pass, &self.glass, glass);
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("emcraft_view3d_over"),
            color_attachments: &[color(wgpu::LoadOp::Load)],
            depth_stencil_attachment: depth_stencil(false),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(composite);
        pass.set_bind_group(0, bind, &[]);
        pass.draw(0..3, 0..1);
        pass.set_bind_group(0, &self.uniform_bind, &[]);
        pass.set_stencil_reference(1);
        tris(&mut pass, &self.glass_depth, glass);
        lines(&mut pass, &self.front_lines);
        drop(pass);
        self.last = Some(key);
        Ok(())
    }

    /// Shows the frame drawn last in `pass` (the window's, its viewport the 3D view).
    pub fn show(&self, pass: &mut wgpu::RenderPass<'_>) {
        if let Some((_, bind)) = &self.show_bind {
            pass.set_pipeline(&self.show);
            pass.set_bind_group(0, bind, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}
