//! The model's steps: its bodies and extent, its materials, its boundaries, its mesh.

use super::*;
use crate::lang::args;

pub(super) fn geometry(app: &EmcraftApp, ui: &mut egui::Ui, has_dialog: bool, out: &mut Actions) {
    let lu = app.session.display().length.symbol();
    let t = &Tokens::get();
    // No geometry yet: the two ways to begin, a guide or a design from SolidCraft.
    if app.session.model().bounds().is_none() && !has_dialog {
        let enabled = !app.busy();
        use crate::glyphs::Glyph;
        for (glyph, label, handle, command) in [
            (Glyph::Import, t!("Import a SolidCraft design"), Fixed::StepImport, "geometry.import"),
            (Glyph::Waveguide, t!("Waveguide"), Fixed::StepWaveguide, "geometry.waveguide"),
        ] {
            if ui.named_enabled(enabled, ui_kit::glyph_button(glyph, label, handle == Fixed::StepImport), handle.into()).clicked() {
                out.push(Action::Fix(command.to_owned()));
            }
            ui.add_space(4.0);
        }
        ui.add_space(6.0);
    }
    ui_kit::section(ui, t!("Bodies"));
    let tree = crate::browser::tree(app);
    for (_, rows) in tree.iter().filter(|(s, _)| *s == "Bodies") {
        for r in rows {
            ui_kit::small(ui, crate::lang::Raw(&r.tip()), t.text);
        }
    }
    if let Some(emcraft_model::Aabb { min: lo, max: hi }) = app.session.model().bounds() {
        let l = app.session.display().length;
        kv(ui, t!("extent"), format!("{} × {} × {} {lu}", fmt(l.from_si(hi.x - lo.x)), fmt(l.from_si(hi.y - lo.y)), fmt(l.from_si(hi.z - lo.z))));
    }
}

pub(super) fn materials(app: &EmcraftApp, ui: &mut egui::Ui, out: &mut Actions) {
    let t = &Tokens::get();
    ui_kit::section(ui, t!("Materials"));
    let m = app.session.model();
    for (i, mat) in m.materials.iter().enumerate() {
        ui.horizontal(|ui| {
            let c = crate::scene3d::material_rgb(&crate::theme::scene_colours(), m, i);
            let (r, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
            ui.painter().rect_filled(r, 2.0, Color32::from_rgb(c.0, c.1, c.2));
            let tex = format!("{}:  {}", mat.name.replace('$', "\\$"), crate::math::material(mat));
            let r = ui.label(crate::math::job(&tex, t.font_small, t.text));
            r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, format!("{}: {}", mat.name, mat.summary())));
        });
    }
    ui.add_space(4.0);
    let glyph = ui_kit::glyph_button(crate::glyphs::Glyph::Material, t!("New material"), false);
    if ui.named_enabled(!app.busy(), glyph, Fixed::StepMaterial.into()).clicked() {
        out.push(Action::Fix("model.material.create".to_owned()));
    }
}

pub(super) fn boundaries(app: &EmcraftApp, ui: &mut egui::Ui) {
    let lu = app.session.display().length.symbol();
    ui_kit::section(ui, t!("Boundaries"));
    let m = app.session.model();
    kv(ui, t!("walls"), m.domain.wall_conductivity.map_or_else(|| tr(t!("perfect conductor")).into(), |s| format!("σ = {s:.3e} S/m")));
    let l = app.session.display().length;
    if let Some(w) = m.domain.wall_thickness {
        kv(ui, t!("wall thickness"), format!("{} {lu}", fmt(l.from_si(w.get()))));
    }
    let kind = |o: &emcraft_model::Open| tr(if o.shape == emcraft_model::OpenShape::Sphere { t!("spherical PML") } else { t!("PML") });
    let thick = |o: &emcraft_model::Open| {
        trf(t!("{kind} {thickness} {unit} thick"), args!(kind = kind(o), thickness = fmt(l.from_si(o.thickness.get())), unit = lu))
    };
    kv(ui, t!("open"), m.domain.open.map_or_else(|| tr(t!("closed (no PML)")).into(), |o| thick(&o)));
    let planes: Vec<String> = m
        .domain
        .symmetry
        .iter()
        .map(|s| {
            let wall = tr(if s.kind == emcraft_model::SymmetryKind::Electric { t!("electric") } else { t!("magnetic") });
            format!("{} = {} {lu}, {wall}", s.axis.id(), fmt(l.from_si(s.at)))
        })
        .collect();
    kv(ui, t!("symmetry"), if planes.is_empty() { tr(t!("none (the whole model)")).into() } else { planes.join("; ") });
    // The group the model has (its geometry and ports) and the one the solved planes make.
    if let Some((_, found)) = &app.workflow.symmetry {
        let axes = |list: Vec<&str>| {
            if list.is_empty() { tr(t!("no mirror plane")).to_owned() } else { trf(t!("mirrors across {axes}"), args!(axes = list.join(", "))) }
        };
        let has = found.iter().map(|f| f.axis.id()).collect::<Vec<_>>();
        kv(ui, t!("model's symmetry"), format!("{} · {}", emcraft_model::symmetry::mirror_group(has.len()), axes(has.clone())));
        let used = m.domain.symmetry.iter().map(|s| s.axis.id()).collect::<Vec<_>>();
        let share = 1u32 << used.len().min(3);
        let solved = if used.is_empty() {
            trf(t!("the whole model ({group})"), args!(group = emcraft_model::symmetry::mirror_group(0)))
        } else {
            trf(t!("1/{share} of the model ({group})"), args!(share = share, group = emcraft_model::symmetry::mirror_group(used.len())))
        };
        kv(ui, t!("solved"), solved);
        let unused: Vec<&str> = has.into_iter().filter(|a| !used.contains(a)).collect();
        if !unused.is_empty() && used.len() < 2 && m.domain.rotation.is_none() {
            ui_kit::small(ui, trf(t!("Find symmetry can cut it across {axes} too"), args!(axes = unused.join(", "))), Tokens::get().text_dim);
        }
    }
    // A rotation set, or the one the model allows.
    if let Some(r) = &m.domain.rotation {
        kv(ui, t!("rotation"), trf(t!("C{n} · 1/{n} of the model, harmonic {m}"), args!(n = r.n, m = r.m)));
    } else if let Some((_, Some(found))) = &app.workflow.rotation {
        kv(ui, t!("model's rotation"), found.group());
        ui_kit::small(ui, t!("Find rotation can solve it on a sector"), Tokens::get().text_dim);
    }
    if let Some(p) = &m.domain.periodic {
        kv(ui, t!("periodic"), crate::browser::periodic_text(p));
    }
}

pub(super) fn mesh(app: &EmcraftApp, ui: &mut egui::Ui) {
    let lu = app.session.display().length.symbol();
    let t = &Tokens::get();
    ui_kit::section(ui, t!("Mesh"));
    let l = app.session.display().length;
    let Some(m) = app.session.model().mesh.as_ref() else {
        ui_kit::small(ui, t!("not meshed yet"), t.text_dim);
        return;
    };
    kv(ui, t!("nodes"), m.nodes.len().to_string());
    kv(ui, t!("tetrahedra"), m.tets.len().to_string());
    kv(ui, t!("boundary faces"), m.boundary.len().to_string());
    let volume: f64 = (0..m.tets.len()).filter_map(|i| m.volume(i)).sum();
    let h = (6.0 * std::f64::consts::SQRT_2 * volume / m.tets.len().max(1) as f64).cbrt();
    kv(ui, t!("mean element"), format!("{:.3} {lu}", l.from_si(h)));
    if let Some(b) = app.session.model().job.range() {
        let index = app.session.model().materials.iter().map(|x| (x.eps_r.get() * x.mu_r.get()).sqrt()).fold(1.0, f64::max);
        let lambda = emcraft_model::units::C0 / (b.stop.get() * index);
        kv(ui, t!("per wavelength"), trf(t!("{n} at the band's top"), args!(n = format!("{:.1}", lambda / h.max(1e-300)))));
    }
    ui_kit::section(ui, t!("Estimate"));
    match estimate(app) {
        Ok(e) => {
            let n = |k: &str| e.get(k).and_then(Value::as_f64).unwrap_or(f64::NAN);
            let order = e.get("order").and_then(Value::as_u64).unwrap_or(2);
            kv(ui, t!("unknowns"), trf(t!("{n} (order {order})"), args!(n = format!("{:.0}", n("unknowns")), order = order)));
            kv(ui, t!("memory"), format!("{:.0} MB", n("memory_mb")));
            let measured = e.get("from_this_session").and_then(Value::as_bool) == Some(true);
            let template = if measured { t!("{secs} s per frequency") } else { t!("{secs} s per frequency (reference machine)") };
            kv(ui, t!("time"), trf(template, args!(secs = format!("{:.2}", n("seconds_per_frequency")))));
        }
        Err(e) => {
            ui_kit::small(ui, Raw(e), t.text_dim);
        }
    }
}
