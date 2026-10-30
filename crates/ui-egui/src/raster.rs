//! A frame drawn without a window or a GPU: egui's tessellated triangles filled on the CPU, with
//! the frame's textures (the font atlas, the field and chart pictures). `ui.render` and the
//! scenarios' shots use it. Adapted from SolidCraft's test rasteriser
//! (`crates/ui-egui/src/ui_kit/checks_tests.rs`, `raster` and `textures_update`): barycentric
//! coverage with half-open edges, premultiplied blending; here with bilinear texture sampling and
//! no panics on partial or missing textures.

use std::collections::HashMap;

use egui::{Color32, Rect, TextureId, vec2};

/// The textures a frame's meshes sample, kept up to date from each frame's `textures_delta`.
#[derive(Default)]
pub struct Textures {
    map: HashMap<TextureId, egui::ColorImage>,
}

impl Textures {
    /// Applies a frame's texture changes (new textures, partial updates, frees).
    pub fn update(&mut self, delta: &egui::TexturesDelta) {
        for (id, d) in delta.set.iter().flat_map(|(id, list)| list.iter().map(move |d| (id, d))) {
            let egui::ImageData::Color(image) = &d.image;
            match d.pos {
                Some([x, y]) => {
                    let Some(existing) = self.map.get_mut(id) else { continue };
                    let (ew, eh) = (existing.size[0], existing.size[1]);
                    for row in 0..image.size[1] {
                        if y + row >= eh {
                            break;
                        }
                        for col in 0..image.size[0] {
                            if x + col >= ew {
                                break;
                            }
                            if let (Some(dst), Some(src)) =
                                (existing.pixels.get_mut((y + row) * ew + x + col), image.pixels.get(row * image.size[0] + col))
                            {
                                *dst = *src;
                            }
                        }
                    }
                }
                None => {
                    self.map.insert(*id, image.as_ref().clone());
                }
            }
        }
        for id in &delta.free {
            self.map.remove(id);
        }
    }

    fn sample(&self, id: TextureId, u: f32, v: f32) -> Option<[f32; 4]> {
        let t = self.map.get(&id)?;
        let (w, h) = (t.size[0], t.size[1]);
        if w == 0 || h == 0 {
            return None;
        }
        let x = (u * w as f32 - 0.5).clamp(0.0, (w - 1) as f32);
        let y = (v * h as f32 - 0.5).clamp(0.0, (h - 1) as f32);
        let (x0, y0) = (x.floor() as usize, y.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let px = |x: usize, y: usize| t.pixels.get(y * w + x).copied().unwrap_or(Color32::TRANSPARENT).to_array();
        let (a, b, c, d) = (px(x0, y0), px(x1, y0), px(x0, y1), px(x1, y1));
        let mut out = [0.0; 4];
        for k in 0..4 {
            let top = f32::from(a[k]) * (1.0 - fx) + f32::from(b[k]) * fx;
            let bottom = f32::from(c[k]) * (1.0 - fx) + f32::from(d[k]) * fx;
            out[k] = top * (1.0 - fy) + bottom * fy;
        }
        Some(out)
    }
}

/// The largest picture side (pixels).
pub const MAX_SIDE: usize = 8192;

/// Fills a frame's shapes into an RGBA picture of `size` points at `ppp` pixels per point.
pub fn raster(
    ctx: &egui::Context,
    shapes: Vec<egui::epaint::ClippedShape>,
    ppp: f32,
    size: egui::Vec2,
    textures: &Textures,
) -> emcraft_render::Canvas {
    let bounds = Rect::from_min_size(egui::Pos2::ZERO, size);
    let width = ((size.x * ppp).round().max(1.0) as usize).min(MAX_SIDE);
    let height = ((size.y * ppp).round().max(1.0) as usize).min(MAX_SIDE);
    let mut px = vec![0.0f32; width * height * 4];
    for p in px.as_chunks_mut::<4>().0 {
        p[3] = 255.0;
    }
    let cross = |a: egui::Vec2, b: egui::Vec2| a.x * b.y - a.y * b.x;
    for clipped in ctx.tessellate(shapes, ppp) {
        let egui::epaint::Primitive::Mesh(mesh) = clipped.primitive else { continue };
        let clip = clipped.clip_rect.intersect(bounds);
        for tri in mesh.indices.as_chunks::<3>().0 {
            let (Some(a), Some(b0), Some(c0)) =
                (mesh.vertices.get(tri[0] as usize), mesh.vertices.get(tri[1] as usize), mesh.vertices.get(tri[2] as usize))
            else {
                continue;
            };
            let (a, mut b, mut c) = (*a, *b0, *c0);
            let signed = cross(b.pos - a.pos, c.pos - a.pos);
            if signed < 0.0 {
                std::mem::swap(&mut b, &mut c);
            }
            let area = signed.abs();
            if area < 1e-6 {
                continue;
            }
            let r = Rect::from_min_max(a.pos.min(b.pos).min(c.pos), a.pos.max(b.pos).max(c.pos)).intersect(clip);
            if !r.is_positive() {
                continue;
            }
            let x0 = (r.min.x * ppp).floor().max(0.0) as usize;
            let y0 = (r.min.y * ppp).floor().max(0.0) as usize;
            let x1 = ((r.max.x * ppp).ceil().max(0.0) as usize).min(width);
            let y1 = ((r.max.y * ppp).ceil().max(0.0) as usize).min(height);
            for y in y0..y1 {
                for x in x0..x1 {
                    let at = egui::pos2(0.0, 0.0) + vec2(x as f32 + 0.5, y as f32 + 0.5) / ppp;
                    if !clip.contains(at) {
                        continue;
                    }
                    let w = [cross(b.pos - at, c.pos - at) / area, cross(c.pos - at, a.pos - at) / area, cross(a.pos - at, b.pos - at) / area];
                    // Half-open triangles: a shared edge is filled once.
                    let outside = w.iter().zip([(b.pos, c.pos), (c.pos, a.pos), (a.pos, b.pos)]).any(|(wt, (p, q))| {
                        let e = q - p;
                        *wt < 0.0 || (*wt == 0.0 && !(e.y > 0.0 || (e.y == 0.0 && e.x < 0.0)))
                    });
                    if outside {
                        continue;
                    }
                    let mut color = [0.0f32; 4];
                    let mut uv = egui::Vec2::ZERO;
                    for (v, wt) in [a, b, c].iter().zip(w) {
                        uv += v.uv.to_vec2() * wt;
                        for (d, ch) in color.iter_mut().zip(v.color.to_array()) {
                            *d += f32::from(ch) * wt;
                        }
                    }
                    if let Some(texel) = textures.sample(mesh.texture_id, uv.x, uv.y) {
                        for (d, ch) in color.iter_mut().zip(texel) {
                            *d *= ch / 255.0;
                        }
                    }
                    let i = (y * width + x) * 4;
                    let alpha = (color[3] / 255.0).clamp(0.0, 1.0);
                    if let Some(dst) = px.get_mut(i..i + 4) {
                        for (d, ch) in dst.iter_mut().zip(color) {
                            *d = ch + *d * (1.0 - alpha);
                        }
                    }
                }
            }
        }
    }
    let rgba = px.iter().map(|v| v.round().clamp(0.0, 255.0) as u8).collect();
    emcraft_render::Canvas { w: width, h: height, rgba }
}

/// The app drawn into a fresh offscreen context of `size` points: a few frames (so layout
/// settles and textures load), the last one filled on the CPU.
pub fn render_app(app: &mut crate::EmcraftApp, size: egui::Vec2, ppp: f32) -> emcraft_render::Canvas {
    let ctx = egui::Context::default();
    let mut textures = Textures::default();
    let mut last = None;
    for frame in 0..3 {
        let raw = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, size)),
            time: Some(frame as f64 / 60.0),
            focused: true,
            ..Default::default()
        };
        ctx.set_pixels_per_point(ppp);
        app.use_theme();
        crate::theme::apply(&ctx);
        let mut out = ctx.run_ui(raw, |ui| app.ui(ui));
        textures.update(&out.textures_delta);
        out.textures_delta.clear();
        last = Some((out.shapes, out.pixels_per_point));
    }
    match last {
        Some((shapes, p)) => raster(&ctx, shapes, p, size, &textures),
        None => emcraft_render::Canvas::new(1, 1, emcraft_render::Rgb::WHITE),
    }
}
