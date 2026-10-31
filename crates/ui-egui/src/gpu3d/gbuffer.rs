//! What lies under each pixel of the 3D view, at the window's resolution: the opaque surface's
//! normal and depth (for the screen-space edges and ambient occlusion that `show.wgsl` draws),
//! and the tag of the nearest tagged triangle and of the nearest solid one (opacity ≥ 200, as
//! `emcraft_render::view3d::pick` counts them), for the outline of what is selected or under the
//! pointer and for picking by reading one pixel back.
//!
//! Three small passes over the scene's triangles, each with its own depth: the opaque surfaces'
//! normals and depths; every tagged triangle's tag; the solid tagged triangles' tags. As in the
//! CPU's picking, untagged triangles hide nothing.

use eframe::wgpu;

/// The normal (xyz) of the opaque surface.
pub const NORMAL: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// Its window depth (0 near, 1 far; 1 where there is none).
pub const DEPTH_OUT: wgpu::TextureFormat = wgpu::TextureFormat::R32Float;
pub const ID: wgpu::TextureFormat = wgpu::TextureFormat::R32Uint;
const ZBUF: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// The passes' pipelines.
pub struct Pipelines {
    normals: wgpu::RenderPipeline,
    any: wgpu::RenderPipeline,
    solid: wgpu::RenderPipeline,
}

/// The targets, kept while the view's size stays.
pub struct Targets {
    pub size: [u32; 2],
    pub normal: wgpu::TextureView,
    pub depth: wgpu::TextureView,
    pub any: wgpu::Texture,
    pub solid: wgpu::Texture,
    zbuf: wgpu::TextureView,
}

impl Pipelines {
    pub fn new(
        device: &wgpu::Device,
        module: &wgpu::ShaderModule,
        layout: &wgpu::PipelineLayout,
        buffers: &[Option<wgpu::VertexBufferLayout<'_>>],
    ) -> Pipelines {
        let pipeline = |label: &str, fs: &str, targets: &[Option<wgpu::ColorTargetState>]| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                vertex: wgpu::VertexState { module, entry_point: Some("vs_g"), buffers, compilation_options: Default::default() },
                primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: None, ..Default::default() },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: ZBUF,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::Less),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState { module, entry_point: Some(fs), targets, compilation_options: Default::default() }),
                multiview_mask: None,
                cache: None,
            })
        };
        let t = |format| Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL });
        Pipelines {
            normals: pipeline("emcraft_view3d_g_normals", "fs_g_normals", &[t(NORMAL), t(DEPTH_OUT)]),
            any: pipeline("emcraft_view3d_g_any", "fs_g_any", &[t(ID)]),
            solid: pipeline("emcraft_view3d_g_solid", "fs_g_solid", &[t(ID)]),
        }
    }

    pub fn targets(device: &wgpu::Device, size: [u32; 2]) -> Targets {
        let texture = |label: &str, format, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let read = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC;
        Targets {
            size,
            normal: texture("emcraft_view3d_g_normal", NORMAL, read).create_view(&Default::default()),
            depth: texture("emcraft_view3d_g_depth", DEPTH_OUT, read).create_view(&Default::default()),
            any: texture("emcraft_view3d_g_any", ID, read),
            solid: texture("emcraft_view3d_g_solid", ID, read),
            zbuf: texture("emcraft_view3d_g_zbuf", ZBUF, wgpu::TextureUsages::RENDER_ATTACHMENT).create_view(&Default::default()),
        }
    }

    /// Records the passes: `draw` draws the triangles (`opaque_only` for the normals).
    pub fn record(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        t: &Targets,
        uniforms: &wgpu::BindGroup,
        field: &wgpu::BindGroup,
        normals: bool,
        ids: bool,
        draw: &dyn Fn(&mut wgpu::RenderPass<'_>, bool),
    ) {
        fn attach(view: &wgpu::TextureView, clear: wgpu::Color) -> Option<wgpu::RenderPassColorAttachment<'_>> {
            Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(clear), store: wgpu::StoreOp::Store },
            })
        }
        let zbuf = Some(wgpu::RenderPassDepthStencilAttachment {
            view: &t.zbuf,
            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
            stencil_ops: None,
        });
        let any = t.any.create_view(&Default::default());
        let solid = t.solid.create_view(&Default::default());
        let mut passes: Vec<(&str, Vec<Option<wgpu::RenderPassColorAttachment<'_>>>, &wgpu::RenderPipeline, bool)> = Vec::new();
        if normals {
            let none = wgpu::Color { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };
            let far = wgpu::Color { r: 1.0, g: 0.0, b: 0.0, a: 0.0 };
            passes.push(("emcraft_view3d_g_normals", vec![attach(&t.normal, none), attach(&t.depth, far)], &self.normals, true));
        }
        if ids {
            passes.push(("emcraft_view3d_g_any", vec![attach(&any, wgpu::Color::TRANSPARENT)], &self.any, false));
            passes.push(("emcraft_view3d_g_solid", vec![attach(&solid, wgpu::Color::TRANSPARENT)], &self.solid, false));
        }
        for (label, colors, pipeline, opaque_only) in passes {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(label),
                color_attachments: &colors,
                depth_stencil_attachment: zbuf.clone(),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, uniforms, &[]);
            pass.set_bind_group(1, field, &[]);
            pass.set_pipeline(pipeline);
            draw(&mut pass, opaque_only);
        }
    }
}

/// The tags at pixel `px` of `t` (the nearest tagged triangle's and the nearest solid one's; 0
/// for none), read back at once. `None` if the device cannot.
pub fn read_tags(device: &wgpu::Device, queue: &wgpu::Queue, t: &Targets, px: [u32; 2]) -> Option<(u32, u32)> {
    if px[0] >= t.size[0] || px[1] >= t.size[1] {
        return Some((0, 0));
    }
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("emcraft_view3d_pick"),
        size: 512,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("emcraft_view3d_pick") });
    for (k, tex) in [&t.any, &t.solid].into_iter().enumerate() {
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: tex,
                mip_level: 0,
                origin: wgpu::Origin3d { x: px[0], y: px[1], z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout { offset: 256 * k as u64, bytes_per_row: None, rows_per_image: None },
            },
            wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
        );
    }
    let submission = queue.submit([encoder.finish()]);
    let slice = buffer.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r.is_ok());
    });
    device.poll(wgpu::PollType::Wait { submission_index: Some(submission), timeout: Some(std::time::Duration::from_millis(250)) }).ok()?;
    if !rx.try_recv().ok()? {
        return None;
    }
    let data = slice.get_mapped_range().ok()?;
    let word = |at: usize| Some(u32::from_le_bytes([*data.get(at)?, *data.get(at + 1)?, *data.get(at + 2)?, *data.get(at + 3)?]));
    let out = (word(0)?, word(256)?);
    drop(data);
    buffer.unmap();
    Some(out)
}
