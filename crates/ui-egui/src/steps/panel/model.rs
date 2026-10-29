//! The model's steps: its bodies and extent, its materials, its boundaries, its mesh.

use super::*;

pub(super) fn geometry(app: &mut EmcraftApp, ui: &mut egui::Ui, lu: &str, t: &Tokens) {
    ui_kit::section(ui, "Bodies");
    let tree = crate::browser::tree(app);
    for (_, rows) in tree.iter().filter(|(s, _)| *s == "Geometry") {
        for r in rows {
            ui_kit::small(ui, &r.text, t.text);
        }
    }
    if let Some(emcraft_model::Aabb { min: lo, max: hi }) = app.session.model().bounds() {
        let l = app.session.display().length;
        kv(ui, "extent", format!("{} × {} × {} {lu}", fmt(l.from_si(hi.x - lo.x)), fmt(l.from_si(hi.y - lo.y)), fmt(l.from_si(hi.z - lo.z))), t);
    }
}

pub(super) fn materials(app: &mut EmcraftApp, ui: &mut egui::Ui, t: &Tokens) {
    ui_kit::section(ui, "Materials");
    let m = app.session.model();
    for (i, mat) in m.materials.iter().enumerate() {
        ui.horizontal(|ui| {
            let c = crate::scene3d::material_rgb(m, i);
            let (r, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
            ui.painter().rect_filled(r, 2.0, Color32::from_rgb(c.0, c.1, c.2));
            ui_kit::small(ui, format!("{}: {}", mat.name, mat.summary()), t.text);
        });
    }
}

pub(super) fn boundaries(app: &mut EmcraftApp, ui: &mut egui::Ui, lu: &str, t: &Tokens) {
    ui_kit::section(ui, "Boundaries");
    let m = app.session.model();
    kv(ui, "walls", m.wall_conductivity.map_or("perfect conductor".into(), |s| format!("σ = {s:.3e} S/m")), t);
    let l = app.session.display().length;
    let kind = |o: &emcraft_model::Open| if o.shape == emcraft_model::OpenShape::Sphere { "spherical PML" } else { "PML" };
    kv(ui, "open", m.open.map_or("closed (no PML)".into(), |o| format!("{} {} {lu} thick", kind(&o), fmt(l.from_si(o.thickness)))), t);
    let planes: Vec<String> = m
        .symmetry
        .iter()
        .map(|s| {
            let wall = if s.kind == emcraft_model::SymmetryKind::Electric { "electric" } else { "magnetic" };
            format!("{} = {} {lu}, {wall}", if s.axis == emcraft_model::Axis::X { "x" } else { "y" }, fmt(l.from_si(s.at)))
        })
        .collect();
    kv(ui, "symmetry", if planes.is_empty() { "none (the whole model)".into() } else { planes.join("; ") }, t);
    if let Some(p) = &m.periodic {
        kv(ui, "periodic", crate::browser::periodic_text(p), t);
    }
}

pub(super) fn mesh(app: &mut EmcraftApp, ui: &mut egui::Ui, lu: &str, t: &Tokens) {
    ui_kit::section(ui, "Mesh");
    let l = app.session.display().length;
    let Some(m) = app.session.model().mesh.as_ref() else {
        ui_kit::small(ui, "not meshed yet", t.text_dim);
        return;
    };
    kv(ui, "nodes", m.nodes.len().to_string(), t);
    kv(ui, "tetrahedra", m.tets.len().to_string(), t);
    kv(ui, "boundary faces", m.boundary.len().to_string(), t);
    let volume: f64 = (0..m.tets.len()).filter_map(|i| m.volume(i)).sum();
    let h = (6.0 * std::f64::consts::SQRT_2 * volume / m.tets.len().max(1) as f64).cbrt();
    kv(ui, "mean element", format!("{:.3} {lu}", l.from_si(h)), t);
    if let Some(b) = app.session.model().band {
        let index = app.session.model().materials.iter().map(|x| (x.eps_r * x.mu_r).sqrt()).fold(1.0, f64::max);
        let lambda = emcraft_model::units::C0 / (b.stop * index);
        kv(ui, "per wavelength", format!("{:.1} at the band's top", lambda / h.max(1e-300)), t);
    }
    ui_kit::section(ui, "Estimate");
    match estimate(app) {
        Ok(e) => {
            let n = |k: &str| e.get(k).and_then(Value::as_f64).unwrap_or(f64::NAN);
            kv(ui, "unknowns", format!("{:.0} (order {})", n("unknowns"), e.get("order").and_then(Value::as_u64).unwrap_or(2)), t);
            kv(ui, "memory", format!("{:.0} MB", n("memory_mb")), t);
            let measured = e.get("from_this_session").and_then(Value::as_bool) == Some(true);
            kv(ui, "time", format!("{:.2} s per frequency{}", n("seconds_per_frequency"), if measured { "" } else { " (reference machine)" }), t);
        }
        Err(e) => {
            ui_kit::small(ui, e, t.text_dim);
        }
    }
}
