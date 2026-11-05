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
            (Glyph::Import, "Import a SolidCraft design", Fixed::StepImport, "geometry.import"),
            (Glyph::Waveguide, "Waveguide", Fixed::StepWaveguide, "geometry.waveguide"),
        ] {
            if ui.named_enabled(enabled, ui_kit::glyph_button(glyph, label, handle == Fixed::StepImport), handle.into()).clicked() {
                out.push(Action::Fix(command.to_owned()));
            }
            ui.add_space(4.0);
        }
        ui.add_space(6.0);
    }
    ui_kit::section(ui, "Bodies");
    let tree = crate::browser::tree(app);
    for (_, rows) in tree.iter().filter(|(s, _)| *s == "Geometry") {
        for r in rows {
            ui_kit::small(ui, &r.shown, t.text);
        }
    }
    if let Some(emcraft_model::Aabb { min: lo, max: hi }) = app.session.model().bounds() {
        let l = app.session.display().length;
        kv(ui, "extent", format!("{} × {} × {} {lu}", fmt(l.from_si(hi.x - lo.x)), fmt(l.from_si(hi.y - lo.y)), fmt(l.from_si(hi.z - lo.z))));
    }
}

pub(super) fn materials(app: &EmcraftApp, ui: &mut egui::Ui) {
    let t = &Tokens::get();
    ui_kit::section(ui, "Materials");
    let m = app.session.model();
    for (i, mat) in m.materials.iter().enumerate() {
        ui.horizontal(|ui| {
            let c = crate::scene3d::material_rgb(m, i);
            let (r, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
            ui.painter().rect_filled(r, 2.0, Color32::from_rgb(c.0, c.1, c.2));
            let tex = format!("{}:  {}", mat.name.replace('$', "\\$"), crate::math::material(mat));
            let r = ui.label(crate::math::job(&tex, t.font_small, t.text));
            r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, format!("{}: {}", mat.name, mat.summary())));
        });
    }
}

pub(super) fn boundaries(app: &EmcraftApp, ui: &mut egui::Ui) {
    let lu = app.session.display().length.symbol();
    ui_kit::section(ui, "Boundaries");
    let m = app.session.model();
    kv(ui, "walls", m.wall_conductivity.map_or_else(|| tr("perfect conductor").into(), |s| format!("σ = {s:.3e} S/m")));
    let l = app.session.display().length;
    if let Some(w) = m.wall_thickness {
        kv(ui, "wall thickness", format!("{} {lu}", fmt(l.from_si(w.get()))));
    }
    let kind = |o: &emcraft_model::Open| tr(if o.shape == emcraft_model::OpenShape::Sphere { "spherical PML" } else { "PML" });
    let thick = |o: &emcraft_model::Open| {
        trf("{kind} {thickness} {unit} thick", args!(kind = kind(o), thickness = fmt(l.from_si(o.thickness.get())), unit = lu))
    };
    kv(ui, "open", m.open.map_or_else(|| tr("closed (no PML)").into(), |o| thick(&o)));
    let planes: Vec<String> = m
        .symmetry
        .iter()
        .map(|s| {
            let wall = tr(if s.kind == emcraft_model::SymmetryKind::Electric { "electric" } else { "magnetic" });
            format!("{} = {} {lu}, {wall}", s.axis.id(), fmt(l.from_si(s.at)))
        })
        .collect();
    kv(ui, "symmetry", if planes.is_empty() { tr("none (the whole model)").into() } else { planes.join("; ") });
    // The group the model has (its geometry and ports) and the one the solved planes make.
    if let Some((_, found)) = &app.workflow.symmetry {
        let axes = |list: Vec<&str>| {
            if list.is_empty() { tr("no mirror plane").to_owned() } else { trf("mirrors across {axes}", args!(axes = list.join(", "))) }
        };
        let has = found.iter().map(|f| f.axis.id()).collect::<Vec<_>>();
        kv(ui, "model's symmetry", format!("{} · {}", emcraft_model::symmetry::mirror_group(has.len()), axes(has.clone())));
        let used = m.symmetry.iter().map(|s| s.axis.id()).collect::<Vec<_>>();
        let share = 1u32 << used.len().min(3);
        let solved = if used.is_empty() {
            trf("the whole model ({group})", args!(group = emcraft_model::symmetry::mirror_group(0)))
        } else {
            trf("1/{share} of the model ({group})", args!(share = share, group = emcraft_model::symmetry::mirror_group(used.len())))
        };
        kv(ui, "solved", solved);
        let unused: Vec<&str> = has.into_iter().filter(|a| !used.contains(a)).collect();
        if !unused.is_empty() && used.len() < 2 && m.rotation.is_none() {
            ui_kit::small(ui, trf("Find symmetry can cut it across {axes} too", args!(axes = unused.join(", "))), Tokens::get().text_dim);
        }
    }
    // A rotation set, or the one the model allows.
    if let Some(r) = &m.rotation {
        kv(ui, "rotation", trf("C{n} · 1/{n} of the model, harmonic {m}", args!(n = r.n, m = r.m)));
    } else if let Some((_, Some(found))) = &app.workflow.rotation {
        kv(ui, "model's rotation", found.group());
        ui_kit::small(ui, "Find rotation can solve it on a sector", Tokens::get().text_dim);
    }
    if let Some(p) = &m.periodic {
        kv(ui, "periodic", crate::browser::periodic_text(p));
    }
}

pub(super) fn mesh(app: &EmcraftApp, ui: &mut egui::Ui) {
    let lu = app.session.display().length.symbol();
    let t = &Tokens::get();
    ui_kit::section(ui, "Mesh");
    let l = app.session.display().length;
    let Some(m) = app.session.model().mesh.as_ref() else {
        ui_kit::small(ui, "not meshed yet", t.text_dim);
        return;
    };
    kv(ui, "nodes", m.nodes.len().to_string());
    kv(ui, "tetrahedra", m.tets.len().to_string());
    kv(ui, "boundary faces", m.boundary.len().to_string());
    let volume: f64 = (0..m.tets.len()).filter_map(|i| m.volume(i)).sum();
    let h = (6.0 * std::f64::consts::SQRT_2 * volume / m.tets.len().max(1) as f64).cbrt();
    kv(ui, "mean element", format!("{:.3} {lu}", l.from_si(h)));
    if let Some(b) = app.session.model().job.range() {
        let index = app.session.model().materials.iter().map(|x| (x.eps_r.get() * x.mu_r.get()).sqrt()).fold(1.0, f64::max);
        let lambda = emcraft_model::units::C0 / (b.stop.get() * index);
        kv(ui, "per wavelength", trf("{n} at the band's top", args!(n = format!("{:.1}", lambda / h.max(1e-300)))));
    }
    ui_kit::section(ui, "Estimate");
    match estimate(app) {
        Ok(e) => {
            let n = |k: &str| e.get(k).and_then(Value::as_f64).unwrap_or(f64::NAN);
            let order = e.get("order").and_then(Value::as_u64).unwrap_or(2);
            kv(ui, "unknowns", trf("{n} (order {order})", args!(n = format!("{:.0}", n("unknowns")), order = order)));
            kv(ui, "memory", format!("{:.0} MB", n("memory_mb")));
            let measured = e.get("from_this_session").and_then(Value::as_bool) == Some(true);
            let template = if measured { "{secs} s per frequency" } else { "{secs} s per frequency (reference machine)" };
            kv(ui, "time", trf(template, args!(secs = format!("{:.2}", n("seconds_per_frequency")))));
        }
        Err(e) => {
            ui_kit::small(ui, Raw(e), t.text_dim);
        }
    }
}
