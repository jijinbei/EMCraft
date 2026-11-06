//! The 3D view's drawing and its chrome: the drawer (the window's GPU, an offscreen device, none),
//! the frame drawn, the labels, cards and legend over it, the navigator, the tool bar and the
//! section bar.

use super::*;

emcraft_macros::named_enum! {
    /// What draws the 3D view.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Drawer {
        /// The window's GPU, in a paint callback ([`crate::gpu3d::paint`]).
        Window = "gpu",
        /// A device of the process's own, its picture read back into a texture (a window without a
        /// wgpu device, the headless harness, `ui.render`).
        Offscreen = "offscreen",
        /// No adapter at all: a message.
        None = "none",
    }
}

impl Drawer {
    /// The window's GPU where it has one, else the offscreen device, else nothing.
    pub fn choose(window: bool, offscreen: bool) -> Drawer {
        match (window, offscreen) {
            (true, _) => Drawer::Window,
            (false, true) => Drawer::Offscreen,
            (false, false) => Drawer::None,
        }
    }
}

/// What draws the view in `ui`'s context.
pub(super) fn drawer_of(ui: &egui::Ui) -> Drawer {
    Drawer::choose(crate::gpu3d::available(ui.ctx()).is_some(), crate::gpu3d::headless::shared().is_ok())
}

/// The scene drawn into `rect` by the GPU: by the window's in a paint callback, or offscreen
/// into a picture (drawn again when what it shows changed); without either, a message.
/// `selected`: what is outlined.
pub(super) fn draw(v: &mut View3d, selected: &[crate::scene3d::Pick], ui: &egui::Ui, rect: Rect, moving: bool, look: crate::prefs::GpuLook) {
    let t = &Tokens::get();
    let drawer = drawer_of(ui);
    let p = &ui.painter().with_clip_rect(rect);
    v.drawn_on_gpu = drawer != Drawer::None;
    v.drawn_by = drawer;
    let began = Instant::now();
    let frame = match drawer {
        Drawer::None => None,
        _ => Some(gpu_frame(v, selected, ui, rect, moving, look)),
    };
    match (drawer, frame) {
        (Drawer::Window, Some(frame)) => {
            v.gpu_frame = look.pick.then(|| frame.key());
            v.last_size = frame.size.map(|s| s as usize);
            crate::gpu3d::paint(p, rect, frame);
            v.gpu_error = None;
        }
        (Drawer::Offscreen, Some(frame)) => {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (frame.key(), frame.look, &frame.selected, frame.hover).hash(&mut h);
            let key = h.finish();
            if v.picture.as_ref().is_none_or(|(k, _)| *k != key) {
                match crate::gpu3d::picture(v.client, &frame) {
                    Ok(canvas) => {
                        v.picture = Some((key, Picture::new(&canvas)));
                        v.gpu_error = None;
                    }
                    Err(e) => {
                        v.picture = None;
                        v.gpu_error = Some(e.to_string());
                    }
                }
            }
            v.gpu_frame = (look.pick && v.picture.is_some()).then(|| frame.key());
            v.last_size = frame.size.map(|s| s as usize);
        }
        _ => {
            v.picture = None;
            v.gpu_error = Some(tr(t!("No GPU: the 3D view needs a graphics adapter (or a software one, such as Mesa's llvmpipe).")).to_owned());
        }
    }
    v.last_ms = began.elapsed().as_secs_f64() * 1e3;
    if drawer != Drawer::Window {
        match v.picture.as_mut() {
            Some((_, pic)) => {
                let id = pic.texture(ui.ctx());
                p.image(id, rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }
            None => {
                p.rect_filled(rect, 0.0, t.viewport_bottom);
                let text = v.gpu_error.clone().unwrap_or_default();
                let g = p.layout(text, FontId::proportional(t.font_small), t.text_dim, (rect.width() - 40.0).max(80.0));
                p.galley(rect.center() - g.size() / 2.0, g, t.text_dim);
            }
        }
    }
}

/// The frame of the scene the GPU draws into `rect`, supersampled at rest when small enough.
pub(super) fn gpu_frame(
    v: &mut View3d,
    selected: &[crate::scene3d::Pick],
    ui: &egui::Ui,
    rect: Rect,
    moving: bool,
    look: crate::prefs::GpuLook,
) -> crate::gpu3d::Frame {
    let offscreen = v.drawn_by == Drawer::Offscreen;
    let t = &Tokens::get();
    let ppp = f64::from(ui.ctx().pixels_per_point());
    let out =
        [((f64::from(rect.width()) * ppp).round().clamp(1.0, 4096.0) as u32), ((f64::from(rect.height()) * ppp).round().clamp(1.0, 4096.0) as u32)];
    let small = f64::from(out[0]) * f64::from(out[1]) <= SUPERSAMPLE_PIXELS && out[0].max(out[1]) <= 2048;
    let supersample = if !moving && small { 2 } else { 1 };
    let size = [out[0] * supersample, out[1] * supersample];
    let id = v.scene_gen();
    let mut cache = std::mem::take(&mut v.gpu);
    let field_plane = v.field_frame.as_ref().map(|f| {
        let g = &f.grid;
        let n = [g.u_end, g.v_end].map(|e| [e[0] - g.corner[0], e[1] - g.corner[1], e[2] - g.corner[2]]);
        let axis = (0..3).find(|&k| n[0][k] == 0.0 && n[1][k] == 0.0).unwrap_or(0);
        (scene3d::Pick::Plane(0).tag(), axis, g.corner[axis])
    });
    let packed = match v.shown() {
        // Offscreen (read back, often on a software adapter) a large scene moves as its coarser copy.
        Some(b) if b.gpu_scene.is_none() && moving && offscreen && !std::ptr::eq(b.moving(), &b.scene) => {
            cache.packed_arrows(b.moving(), id.wrapping_add(1 << 63), field_plane, &b.arrows)
        }
        Some(b) => cache.packed_arrows(b.gpu_scene.as_ref().unwrap_or(&b.scene), id, field_plane, &b.arrows),
        None => cache.packed(&EMPTY.scene, u64::MAX),
    };
    v.gpu = cache;
    let line_width = ppp.max(1.0) * f64::from(supersample);
    let mut uniforms = crate::gpu3d::Uniforms::new(&packed, &v.camera, size, line_width, (rgb(t.viewport_top), rgb(t.viewport_bottom)));
    if let Some(cut) = v.section {
        let sign = cut.keep.sign() as f32;
        uniforms.clip[cut.axis] = sign;
        uniforms.clip[3] = (f64::from(sign) * (cut.at - packed.origin[cut.axis])) as f32;
    }
    let gpu_look = crate::gpu3d::Look { edges: look.edges, ao: look.ao, outline: look.outline, pick: look.pick || look.outline };
    let rgba = |c: Color32, a: f32| [f32::from(c.r()) / 255.0, f32::from(c.g()) / 255.0, f32::from(c.b()) / 255.0, a];
    let hover = v.pointer_tag.map_or(0, |(_, _, tag)| tag);
    crate::gpu3d::Frame {
        look: gpu_look,
        selected: selected.iter().map(|p| p.tag()).collect(),
        hover: if selected.iter().any(|p| p.tag() == hover) { 0 } else { hover },
        outline: rgba(t.accent, 1.0),
        hover_color: rgba(t.accent, 0.5),
        ppp: ppp as f32,
        field: v.field_frame.clone(),
        ..crate::gpu3d::Frame::plain(packed, uniforms, size, out, supersample)
    }
}

/// The field view's plane for the GPU to colour: its phasors (uploaded again only when the
/// samples change) and this frame's phase and scale; `None` when there is nothing to colour.
pub(super) fn field_frame(app: &EmcraftApp, v: &mut View3d, field: &FieldState) -> Option<crate::gpu3d::FieldFrame> {
    use std::hash::{Hash, Hasher};
    let s = field.samples.as_ref()?;
    let lo = v.bounds()?.0;
    let component = field.view.component;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (app.revision, format!("{:?}", FieldView { phase: 0.0, ..field.view.clone() }), s.at.to_bits(), lo.map(f64::to_bits)).hash(&mut h);
    let id = h.finish();
    let grid = match &v.field_grid {
        Some(g) if g.id == id => g.clone(),
        _ => {
            let (nx, ny, phasors) = s.phasors(component);
            let (x, y) = s.axes();
            let frame = scene3d::field_frame_of(&app.session, s.plane, s.at, x.span(), y.span(), lo);
            let axis = match component {
                emcraft_engine::view::Component::Ex | emcraft_engine::view::Component::Hx => 0,
                emcraft_engine::view::Component::Ey | emcraft_engine::view::Component::Hy => 1,
                emcraft_engine::view::Component::Ez | emcraft_engine::view::Component::Hz => 2,
                _ => 3,
            };
            let g = std::sync::Arc::new(crate::gpu3d::FieldGrid::new(id, axis, (nx, ny, &phasors), frame));
            v.field_grid = Some(g.clone());
            g
        }
    };
    let phase = field.view.phase;
    let peak = grid.peak(phase);
    Some(crate::gpu3d::FieldFrame { grid, phase, peak })
}

/// The window pixel under `at` (points from the view's corner).
pub(super) fn pixel(ctx: &egui::Context, at: egui::Vec2) -> [u32; 2] {
    let ppp = ctx.pixels_per_point();
    [(at.x * ppp).floor().max(0.0) as u32, (at.y * ppp).floor().max(0.0) as u32]
}

/// The tag under the pointer in the GPU's last frame, read again when the pointer or the frame
/// changed (for its outline); none when `on` is off or the pointer is elsewhere.
pub(super) fn pointer_tag(v: &mut View3d, ui: &egui::Ui, rect: Rect, on: bool) {
    let hovered = ui.input(|i| i.pointer.hover_pos()).filter(|p| rect.contains(*p) && ui.rect_contains_pointer(rect));
    let (Some(key), Some(pos), true) = (v.gpu_frame, hovered, on) else {
        v.pointer_tag = None;
        return;
    };
    let px = pixel(ui.ctx(), pos - rect.min);
    if v.pointer_tag.is_some_and(|(p, k, _)| p == px && k == key) {
        return;
    }
    v.pointer_tag = crate::gpu3d::pick(ui.ctx(), v.client, key, px).map(|(any, solid)| (px, key, if solid != 0 { solid } else { any }));
}

/// The scene's labels where they project, faint on faces turned away.
pub(super) fn labels(p: &egui::Painter, rect: Rect, cam: &Camera, labels: &[scene3d::Label]) {
    let t = &Tokens::get();
    let font = FontId::proportional(t.font_small);
    let b = cam.basis();
    for l in labels {
        let Some(q) = cam.project(l.at, rect.width() as f64, rect.height() as f64) else { continue };
        let at = rect.min + vec2(q[0] as f32, q[1] as f32);
        if !rect.contains(at) {
            continue;
        }
        let away = l.facing.is_some_and(|n| n[0] * (b.eye[0] - l.at[0]) + n[1] * (b.eye[1] - l.at[1]) + n[2] * (b.eye[2] - l.at[2]) < 0.0);
        let ink = if away { t.text_dim } else { color(l.color) };
        if let Some(q) = l.anchor.and_then(|a| cam.project(a, rect.width() as f64, rect.height() as f64)) {
            let to = rect.min + vec2(q[0] as f32, q[1] as f32);
            p.line_segment([at, to], Stroke::new(1.0, ink));
            p.circle_filled(to, 2.5, ink);
        }
        let g = p.layout_no_wrap(l.text.clone(), font.clone(), ink);
        let r = Rect::from_center_size(at, g.size() + vec2(8.0, 4.0));
        if !away {
            p.rect_filled(r, 3.0, t.panel.gamma_multiply(0.85));
        }
        p.galley(r.min + vec2(4.0, 2.0), g, ink);
    }
}

/// The selection on a card at the top: its name (or how many parts; TeX, its symbols set as
/// mathematics), with Edit and Delete beside it for items of the model (a port, a body).
pub(super) struct Card {
    pub(super) tex: String,
    /// Edit: one item of the model.
    pub(super) edit: Option<Action>,
    /// Delete: its items.
    pub(super) delete: Option<Action>,
    /// The buttons work (not while busy).
    pub(super) enabled: bool,
}

impl Card {
    /// The card at the top of `rect`; its rectangle.
    pub(super) fn show(self, ui: &mut egui::Ui, rect: Rect, out: &mut Actions) -> Rect {
        let Card { tex, edit, delete, enabled } = self;
        let t = &Tokens::get();
        let p = &ui.painter().with_clip_rect(rect);
        let g = p.layout_job(crate::math::job(&tex, t.font_small, t.text));
        let (size, pitch) = (20.0, 24.0);
        let actions: Vec<(Glyph, Fixed, crate::lang::Text, Action)> = [
            edit.map(|a| (Glyph::Edit, Fixed::View3dSelectedEdit, t!("Edit"), a)),
            delete.map(|a| (Glyph::Delete, Fixed::View3dSelectedDelete, t!("Delete"), a)),
        ]
        .into_iter()
        .flatten()
        .collect();
        let buttons = if actions.is_empty() { 0.0 } else { actions.len() as f32 * pitch + 4.0 };
        let width = g.size().x + 20.0 + buttons;
        let r = Rect::from_min_size(pos2(rect.center().x - width / 2.0, rect.top() + 10.0), vec2(width, g.size().y.max(size) + 10.0));
        p.rect_filled(r, 6.0, t.panel.gamma_multiply(0.92));
        p.rect_stroke(r, 6.0, Stroke::new(1.0, t.accent), egui::StrokeKind::Inside);
        p.galley(pos2(r.left() + 10.0, r.center().y - g.size().y / 2.0), g, t.text);
        scenario::publish_target(Fixed::View3dSelected, r);
        let mut x = r.right() - buttons;
        for (glyph, handle, tip, action) in actions {
            let b = Rect::from_center_size(pos2(x + pitch / 2.0, r.center().y), vec2(size, size));
            let resp = ui.hit(b, handle.into(), if enabled { Sense::click() } else { Sense::hover() });
            if resp.hovered() && enabled {
                p.rect_filled(b, 4.0, t.hover);
            }
            glyphs::paint(p, b.shrink(2.0), glyph, if enabled { t.text } else { t.text_dim }, t.accent);
            if resp.hint(tip).clicked() && enabled {
                out.push(action);
            }
            x += pitch;
        }
        r
    }
}

/// A field plane's box in the model (m) and the standard view that faces it.
pub(super) fn facing_plane(l: emcraft_model::units::Length, lo: [f64; 3], f: Facing) -> ([f64; 3], [f64; 3], StandardView) {
    let Facing { x: (h0, h1), y: (v0, v1), plane, at } = f;
    let (h0, h1, v0, v1) = (l.to_si(h0), l.to_si(h1), l.to_si(v0), l.to_si(v1));
    match plane {
        Plane::H => ([v0, lo[1] + at, h0], [v1, lo[1] + at, h1], StandardView::Top),
        Plane::E => ([lo[0] + at, v0, h0], [lo[0] + at, v1, h1], StandardView::Right),
        Plane::Cross => ([h0, v0, lo[2] + at], [h1, v1, lo[2] + at], StandardView::Front),
    }
}

impl Built {
    /// The parts drawn over the picture (labels, legend, bounds), without the scene.
    pub(super) fn clone_shallow(&self) -> Built {
        Built {
            scene: Default::default(),
            gpu_scene: None,
            arrows: Vec::new(),
            labels: self.labels.clone(),
            legend: self.legend.clone(),
            bounds: self.bounds.or_else(|| self.scene.bounds()),
            cache: Default::default(),
        }
    }
}

pub(super) fn legend(p: &egui::Painter, rect: Rect, items: &[(String, Rgb)]) {
    let t = &Tokens::get();
    if items.is_empty() {
        return;
    }
    let font = FontId::proportional(t.font_small);
    let row = 18.0;
    // Each item's symbols set as mathematics (`εr` with a lowered r).
    let galleys: Vec<_> = items.iter().map(|(s, _)| p.layout_job(crate::math::rich(s, font.size, t.text))).collect();
    let width = galleys.iter().map(|g| g.size().x).fold(0.0, f32::max) + 34.0;
    let r = Rect::from_min_size(rect.min + vec2(10.0, 10.0), vec2(width, row * items.len() as f32 + 8.0));
    p.rect_filled(r, 6.0, t.panel.gamma_multiply(0.9));
    p.rect_stroke(r, 6.0, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
    for (i, ((_, c), g)) in items.iter().zip(galleys).enumerate() {
        let y = r.top() + 4.0 + row * (i as f32 + 0.5);
        p.rect_filled(Rect::from_center_size(pos2(r.left() + 14.0, y), vec2(12.0, 12.0)), 2.0, color(*c));
        p.galley(pos2(r.left() + 26.0, y - g.size().y / 2.0), g, t.text);
    }
}

/// The standard view looking from the side of axis `k` given by `sign` (its eye there).
pub(crate) fn view_from(k: usize, sign: f64) -> Option<StandardView> {
    StandardView::ALL.into_iter().filter(|v| *v != StandardView::Iso).find(|v| {
        let mut c = Camera::default();
        c.standard(*v);
        // The eye sits against the view direction.
        -c.basis().forward[k] * sign > 0.99
    })
}

/// The axes navigator in the bottom-left corner: the six signed axes as tips round a disc, the
/// positive ones filled with their letter, the negative ones rings; a click views the model
/// from that side.
pub(super) fn navigator(cam: &Camera, ui: &mut egui::Ui, rect: Rect, out: &mut Actions) {
    let t = &Tokens::get();
    let c = pos2(rect.left() + 52.0, rect.bottom() - 52.0);
    let p = ui.painter().with_clip_rect(rect);
    p.circle_filled(c, 44.0, t.panel.gamma_multiply(0.75));
    let axes = view3d::axes_on_screen(cam);
    let forward = cam.basis().forward;
    // Farthest first, so the tips towards the eye are drawn (and hit) on top.
    let mut tips: Vec<(usize, f64)> = (0..3).flat_map(|k| [(k, 1.0), (k, -1.0)]).collect();
    tips.sort_by(|a, b| (forward[b.0] * b.1).total_cmp(&(forward[a.0] * a.1)));
    let mut clicked = None;
    for (k, sign) in tips {
        let dir = vec2(axes[k][0] as f32, axes[k][1] as f32) * sign as f32;
        let tip = c + dir * 30.0;
        // The axes' colours: x red, y green, z blue.
        let color = [t.axis_x, t.axis_y, t.axis_z][k];
        if sign > 0.0 {
            p.line_segment([c, tip], Stroke::new(2.0, color));
        }
        let r = Rect::from_center_size(tip, vec2(18.0, 18.0));
        let Some(view) = view_from(k, sign) else { continue };
        let resp = ui.hit(r, Handle::Standard(view), Sense::click());
        let name = format!("{}{}", if sign > 0.0 { "+" } else { "−" }, ["X", "Y", "Z"][k]);
        let hover = resp.hovered();
        if sign > 0.0 {
            p.circle_filled(tip, if hover { 10.0 } else { 9.0 }, color);
            p.text(tip, Align2::CENTER_CENTER, &name[1..], FontId::proportional(t.font_small), t.axis_label);
        } else {
            p.circle_filled(tip, 7.0, t.panel);
            p.circle_stroke(tip, if hover { 8.0 } else { 7.0 }, Stroke::new(2.0, color));
        }
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, view.id()));
        if resp.on_hover_text(trf(t!("View from {side} ({view})"), args!(side = name, view = view.id()))).clicked() {
            clicked = Some(view);
        }
    }
    if let Some(v) = clicked {
        out.push(Action::StandardView(v));
    }
}

/// The view's tools as icons in a bar at the bottom centre: fit, the home (isometric) view,
/// facing the field's plane (in Results), the section view, perspective or orthographic.
pub(super) struct NavBar {
    /// The field's plane to face: its box (m) and the view that faces it.
    pub(super) facing: Option<([f64; 3], [f64; 3], StandardView)>,
    /// The view stands alone (not inside the Field view, which has its own player and no
    /// section view): it offers both.
    pub(super) standalone: bool,
    /// The field's animation is playing.
    pub(super) playing: bool,
    /// The ports are the subject: their tools too.
    pub(super) ports: bool,
    /// The tool on.
    pub(super) tool: Option<crate::tools3d::Tool>,
}

impl NavBar {
    /// The bar at the bottom of `rect`; the section view cuts the model's `bounds`.
    pub(super) fn show(self, v: &mut View3d, ui: &mut egui::Ui, rect: Rect, bounds: ([f64; 3], [f64; 3]), out: &mut Actions) {
        let NavBar { facing, standalone, playing, ports, tool: on_tool } = self;
        let t = &Tokens::get();
        let ortho = v.camera.projection == Projection::Orthographic;
        let mut items: Vec<(Glyph, Handle, crate::lang::Text)> = vec![
            (Glyph::Fit, Fixed::View3dFit.into(), t!("Fit the model to the view (double-click the view)")),
            (Glyph::Home, Handle::Standard(StandardView::Iso), t!("Home view (isometric)")),
        ];
        if facing.is_some() {
            items.push((Glyph::Lookat, Fixed::View3dFace.into(), t!("Face the field's plane")));
            // The standalone model view needs a player; the Field view already has one.
            if standalone {
                items.push(if playing {
                    (Glyph::Pause, Fixed::View3dPlay.into(), t!("Pause the field's animation"))
                } else {
                    (Glyph::Play, Fixed::View3dPlay.into(), t!("Play the field's animation"))
                });
            }
        }
        if standalone {
            items.push((Glyph::Section, Fixed::View3dSection.into(), if v.section.is_some() { t!("Section view: on") } else { t!("Section view") }));
        }
        items.push(if ortho {
            (Glyph::Orthographic, Fixed::View3dProjection.into(), t!("Orthographic — click for perspective"))
        } else {
            (Glyph::Perspective, Fixed::View3dProjection.into(), t!("Perspective — click for orthographic"))
        });
        // The tools: the ports' where ports are the subject, the ruler everywhere.
        use crate::tools3d::Tool;
        for tool in Tool::ALL.into_iter().filter(|tool| !tool.ports() || ports) {
            items.push((tool.glyph(), Handle::Tool(tool), tool.hint()));
        }
        let (size, pitch) = (28.0, 32.0);
        let width = pitch * items.len() as f32 + 6.0;
        let bar = Rect::from_center_size(pos2(rect.center().x, rect.bottom() - 26.0), vec2(width, size + 8.0));
        let p = ui.painter().with_clip_rect(rect);
        let near = ui.input(|i| i.pointer.hover_pos()).is_some_and(|q| bar.expand(24.0).contains(q));
        p.rect_filled(bar, 8.0, t.panel.gamma_multiply(if near { 0.95 } else { 0.6 }));
        p.rect_stroke(bar, 8.0, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
        for (i, (glyph, handle, tip)) in items.into_iter().enumerate() {
            let tip = tr(tip);
            let r = Rect::from_center_size(pos2(bar.left() + 3.0 + pitch * (i as f32 + 0.5), bar.center().y), vec2(size, size));
            let resp = ui.hit(r, handle, Sense::click());
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tip));
            let on = (handle == Handle::Fixed(Fixed::View3dSection) && v.section.is_some())
                || matches!(handle, Handle::Tool(tool) if on_tool == Some(tool));
            if resp.hovered() || on {
                p.rect_filled(r, 5.0, if on { t.accent_soft } else { t.hover });
            }
            let ink = if near || resp.hovered() || on { t.text } else { t.text.gamma_multiply(0.7) };
            glyphs::paint(&p, r.shrink(4.0), glyph, ink, t.accent);
            if resp.on_hover_text(tip).clicked() {
                match handle {
                    Handle::Fixed(Fixed::View3dFit) => v.goal = Some(Goal::Fit),
                    Handle::Standard(side) => out.push(Action::StandardView(side)),
                    Handle::Fixed(Fixed::View3dPlay) => out.push(Action::SetPlaying(!playing)),
                    Handle::Fixed(Fixed::View3dSection) => v.section = v.section.is_none().then(|| Cut::middle(0, bounds)),
                    Handle::Tool(tool) => out.push(Action::Tool((on_tool != Some(tool)).then_some(tool))),
                    Handle::Fixed(Fixed::View3dFace) => {
                        if let Some((lo, hi, side)) = facing {
                            v.goal = Some(Goal::Face(lo, hi, side));
                        }
                    }
                    _ => v.toggle_projection(),
                }
            }
        }
    }
}

/// The section view's controls in a bar over the tools: a guide's H-plane and E-plane cuts, the
/// axis it cuts across (x, y, z), where along it (a slider over the model's extent, the value in
/// display units `l`), which side stays (flip), the camera straight at the cut and the cut's
/// dimensions on or off.
pub(super) fn section_bar(v: &mut View3d, ui: &mut egui::Ui, rect: Rect, (lo, hi): ([f64; 3], [f64; 3]), l: emcraft_model::units::Length) {
    let Some(mut cut) = v.section else { return };
    let t = &Tokens::get();
    let k = cut.axis.min(2);
    let (size, pitch, slider_w, value_w) = (24.0, 28.0, 170.0, 84.0);
    let width = 5.0 * pitch + 8.0 + slider_w + value_w + 3.0 * pitch + 16.0;
    let bar = Rect::from_center_size(pos2(rect.center().x, rect.bottom() - 70.0), vec2(width, size + 8.0));
    let p = ui.painter().with_clip_rect(rect);
    p.rect_filled(bar, 8.0, t.panel.gamma_multiply(0.95));
    p.rect_stroke(bar, 8.0, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
    let font = FontId::proportional(t.font_label);
    let mut x = bar.left() + 6.0;
    use crate::section3d::{Mark, Preset, button};
    for (preset, handle, letter, tip) in [
        (Preset::HPlane, Fixed::View3dSectionHPlane, "H", t!("H-plane cut (y = b/2)")),
        (Preset::EPlane, Fixed::View3dSectionEPlane, "E", t!("E-plane cut (x = a/2)")),
    ] {
        let r = Rect::from_min_size(pos2(x, bar.center().y - size / 2.0), vec2(size, size));
        if button(ui, r, handle, Mark::Text(letter), tip, preset.is(cut, (lo, hi))) {
            cut = preset.cut((lo, hi));
        }
        x += pitch;
    }
    p.vline(x + 1.0, (bar.top() + 6.0)..=(bar.bottom() - 6.0), Stroke::new(1.0, t.border));
    x += 8.0;
    for (axis, name) in crate::handle::AXES.into_iter().enumerate() {
        let r = Rect::from_min_size(pos2(x, bar.center().y - size / 2.0), vec2(size, size));
        let resp = ui.hit(r, Handle::SectionAxis(axis), Sense::click());
        let tip = trf(t!("Cut across {axis}"), args!(axis = name));
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &tip));
        let chosen = axis == k;
        if chosen || resp.hovered() {
            p.rect_filled(r, 5.0, if chosen { t.accent_soft } else { t.hover });
        }
        let ink = [t.axis_x, t.axis_y, t.axis_z][axis];
        p.text(r.center(), Align2::CENTER_CENTER, name, font.clone(), if chosen { ink } else { t.text_dim });
        if resp.on_hover_text(tip).clicked() && !chosen {
            cut = Cut { axis, ..Cut::middle(axis, (lo, hi)) };
        }
        x += pitch;
    }
    // The position, in display units over the model's extent on the axis.
    let (a, b) = (l.from_si(lo[cut.axis]), l.from_si(hi[cut.axis]));
    let mut at = l.from_si(cut.at).clamp(a.min(b), a.max(b));
    let sr = Rect::from_min_size(pos2(x + 4.0, bar.center().y - 10.0), vec2(slider_w - 8.0, 20.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(sr).layout(egui::Layout::left_to_right(egui::Align::Center)));
    child.spacing_mut().slider_width = sr.width();
    let s = child.add(egui::Slider::new(&mut at, a.min(b)..=a.max(b)).show_value(false));
    scenario::publish_target(Fixed::View3dSectionAt, s.rect);
    if s.hint(t!("Where the cut lies")).changed() && at.is_finite() {
        cut.at = l.to_si(at);
    }
    x += slider_w;
    let value = format!("{} {}", format!("{at:.3}").trim_end_matches('0').trim_end_matches('.'), l.symbol());
    p.text(pos2(x, bar.center().y), Align2::LEFT_CENTER, value, font, t.text);
    x += value_w;
    let r = Rect::from_min_size(pos2(x, bar.center().y - size / 2.0), vec2(size, size));
    let resp = ui.hit(r, Fixed::View3dSectionFlip.into(), Sense::click());
    let tip = tr(t!("Keep the other side"));
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tip));
    if resp.hovered() {
        p.rect_filled(r, 5.0, t.hover);
    }
    glyphs::paint(&p, r.shrink(3.0), Glyph::Flip, t.text, t.accent);
    if resp.on_hover_text(tip).clicked() {
        cut.keep = cut.keep.flipped();
    }
    x += pitch;
    let r = Rect::from_min_size(pos2(x, bar.center().y - size / 2.0), vec2(size, size));
    if button(ui, r, Fixed::View3dSectionLook, Mark::Glyph(Glyph::Lookat), t!("Look straight at the cut"), false) {
        let outline = v.shown().map_or_else(|| crate::section3d::flat(cut, (lo, hi)), |b| crate::section3d::outline(&b.scene, cut, (lo, hi)));
        v.camera.projection = Projection::Orthographic;
        v.goal = crate::section3d::look(cut, outline);
    }
    x += pitch;
    let r = Rect::from_min_size(pos2(x, bar.center().y - size / 2.0), vec2(size, size));
    let dims = v.section_dims;
    if button(ui, r, Fixed::View3dSectionDims, Mark::Glyph(Glyph::Ruler), if dims { t!("Dimensions: on") } else { t!("Dimensions: off") }, dims) {
        v.section_dims = !dims;
    }
    v.section = Some(cut);
}
