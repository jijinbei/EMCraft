//! The browser: the session's model and results as a tree on a floating card at the left, as
//! the model is made: its bodies and what each is made of, the materials (and what uses each),
//! the region solved and what bounds it (its outer boundary, its symmetry planes), what drives
//! it, its mesh and its results. Rows
//! are read from the session every frame; clicking a result shows it, double-clicking a
//! geometry, mesh or boundary row opens its command's dialog on the model's values. A row that
//! names an item of the model (a material, a port, a body, a source) edits and deletes it: its
//! right-click menu (Edit, Delete, Show in 3D), a double-click (Edit) and the Delete key on the
//! selected row (undoable; a refusal shows as an error).
//!
//! Rows show their symbols set as mathematics ([`crate::math`]: `εr` with a lowered r, `S₂₁`)
//! while their plain text names them (handles, keys). A waveguide port's row says its part in
//! the S-parameters ([`PortRole`]): the driven input, whose reflection is S11, or an output,
//! the transmission to it S21; its menu drives it (`view.driven`).

use egui::{Align2, FontId, Rect, Sense, Stroke, pos2, vec2};

use crate::glyphs::{self, Glyph};
use emcraft_model::Kind;
use std::fmt::Display;

use crate::handle::{Fixed, Handle};
use crate::lang::{self, Both, args};
use crate::scene3d::Pick;
use crate::theme::Tokens;
use crate::{Action, Actions, EmcraftApp, Style, UiExt, View, scenario};

/// Folded sections (what is selected is the window's one selection: `View3d::selected`).
#[derive(Clone, Debug, Default)]
pub struct Browser {
    pub folded: Vec<String>,
}

/// What a row does.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    None,
    Show(View),
    Run(&'static str, serde_json::Value),
    Edit(&'static str),
    /// Opens a command's dialog with these values over its usual ones (a body's material).
    EditWith(&'static str, serde_json::Value),
}

impl Act {
    /// Whether a double-click edits.
    pub fn edits(&self) -> bool {
        matches!(self, Act::Edit(_) | Act::EditWith(..))
    }

    /// What a double-click asks for: the dialog the row edits with.
    pub fn edit(self) -> Option<Action> {
        match self {
            Act::Edit(id) => Some(Action::dialog(id)),
            Act::EditWith(id, values) => Some(Action::dialog_with(id, values)),
            _ => None,
        }
    }
}

/// An item of the model a row names, with the commands that edit and delete it.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Item {
    /// Its edit command (its dialog opens on `project.values`).
    pub edit: &'static str,
    pub delete: &'static str,
    /// The parameter naming it in both (`name`, `body`, `index`) and its value.
    pub key: &'static str,
    pub value: serde_json::Value,
}

impl Item {
    fn new(edit: &'static str, delete: &'static str, key: &'static str, value: serde_json::Value) -> Item {
        Item { edit, delete, key, value }
    }

    /// The parameters naming it: `{"name": "ptfe"}`.
    pub fn params(&self) -> serde_json::Value {
        let mut o = serde_json::Map::new();
        o.insert(self.key.to_owned(), self.value.clone());
        serde_json::Value::Object(o)
    }
}

/// A row: what it names (its name, then the detail in a dimmer ink), its glyph, depth and
/// action. Its `id` names it for good (its handle `row:<id>`, its key in the selection): it does
/// not change with the words or the language.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    /// What it is, for good: `port:1`, `body:Block`, `section:2`, `region`.
    pub id: String,
    /// Its name and detail in English (its tooltip in English, tests).
    pub text: String,
    /// Its name in the current language.
    pub shown: String,
    /// Its detail in the current language (empty: none), drawn dimmer after the name.
    pub detail: String,
    pub glyph: Glyph,
    pub act: Act,
    /// How deep it sits under the section (0) or under the row above (1).
    pub depth: u8,
    /// What the row is in the 3D view (selecting one selects the other).
    pub pick: Option<Pick>,
    /// The item of the model it names, which it edits and deletes.
    pub item: Option<Item>,
    /// Its detail as TeX ([`crate::math`]), when not its text's symbols set as mathematics.
    pub rich: Option<String>,
    /// A waveguide port's part in the S-parameters.
    pub role: Option<PortRole>,
    /// What its eye shows or hides (none: no eye): its pick's display object, or its own (a
    /// symmetry plane's sheet), set once as the tree is built.
    pub eye: Option<emcraft_engine::visibility::DisplayObject>,
}

/// A waveguide port's part in the S-parameters as they are read: the driven port is the input
/// (what the field views show excited; its reflection is S11), the others outputs (the
/// transmission to port 2 is S21). A sweep still drives every port in turn.
#[derive(Clone, Debug, PartialEq)]
pub struct PortRole {
    /// The port's name.
    pub name: String,
    pub input: bool,
    /// The entry of S it reads, (row, column) from 1: (1, 1) at the input, (2, 1) at port 2.
    pub s: (usize, usize),
}

impl PortRole {
    /// `input` or `output`.
    pub fn id(&self) -> &'static str {
        if self.input { "input" } else { "output" }
    }

    /// The S-parameter in plain text: `S21` (`S12,1` past nine entries).
    pub fn sparameter(&self) -> String {
        let (i, j) = self.s;
        if i < 10 && j < 10 { format!("S{i}{j}") } else { format!("S{i},{j}") }
    }

    /// Its role in short, in the current language, for its row's pill: `in · S11`, `out · S21`.
    pub fn text(&self) -> String {
        let s = self.sparameter();
        if self.input { lang::trf(t!("in · {s}"), args!(s = s)) } else { lang::trf(t!("out · {s}"), args!(s = s)) }
    }

    /// Its role in full, for tooltips and the Excitation step: `input · reflection S11`,
    /// `output · transmission S21`.
    pub fn tip(&self) -> String {
        let s = self.sparameter();
        if self.input { lang::trf(t!("input · reflection {s}"), args!(s = s)) } else { lang::trf(t!("output · transmission {s}"), args!(s = s)) }
    }

    /// Its colour (the theme's `port_input` or `port_output`).
    pub fn colour(&self) -> egui::Color32 {
        let sc = crate::theme::scene();
        crate::theme::color32(if self.input { sc.port_input } else { sc.port_output })
    }
}

/// Each waveguide port's role, in the model's order: the driven one (`view.driven`) the input.
pub fn port_roles(s: &emcraft_engine::Session) -> Vec<PortRole> {
    let entries = s.sparameter_entries();
    let k = s.driven_entry();
    let driven = s.driven_port();
    s.model()
        .ports
        .iter()
        .enumerate()
        .map(|(j, p)| {
            let input = driven == Some(j);
            let row = if input { k } else { entries.iter().position(|e| e.port == j).unwrap_or(j) };
            PortRole { name: p.name.clone(), input, s: (row + 1, k + 1) }
        })
        .collect()
}

/// A port role's mark in `r`: for the input a wave going in with its reflection coming back,
/// for an output a wave going out of a face.
pub fn role_mark(p: &egui::Painter, r: Rect, input: bool, ink: egui::Color32) {
    let s = Stroke::new(1.6, ink);
    let (l, rt, c) = (r.left() + 1.0, r.right() - 1.0, r.center());
    let head = |p: &egui::Painter, tip: egui::Pos2, dir: f32| {
        p.add(egui::Shape::convex_polygon(vec![tip, tip + vec2(-4.0 * dir, -3.0), tip + vec2(-4.0 * dir, 3.0)], ink, Stroke::NONE));
    };
    if input {
        // The face at the right; the wave in above, the reflection back below.
        p.line_segment([pos2(rt, r.top() + 1.0), pos2(rt, r.bottom() - 1.0)], Stroke::new(2.0, ink));
        let (y0, y1) = (c.y - 3.0, c.y + 3.5);
        p.line_segment([pos2(l, y0), pos2(rt - 5.0, y0)], s);
        head(p, pos2(rt - 2.0, y0), 1.0);
        p.line_segment([pos2(rt - 3.0, y1), pos2(l + 4.0, y1)], Stroke::new(1.2, ink.gamma_multiply(0.8)));
        head(p, pos2(l + 1.0, y1), -1.0);
    } else {
        // The face at the left, the wave leaving it.
        p.line_segment([pos2(l, r.top() + 1.0), pos2(l, r.bottom() - 1.0)], Stroke::new(2.0, ink));
        p.line_segment([pos2(l + 3.0, c.y), pos2(rt - 3.0, c.y)], s);
        head(p, pos2(rt, c.y), 1.0);
    }
}

/// A row of `template` (looked up in the current language) with its values.
/// A row `id` named `name`, its detail `detail` (none: `None`), with `glyph` and `act`.
fn node(id: impl Into<String>, name: Both, detail: Option<Both>, glyph: Glyph, act: Act) -> Row {
    let (en, shown) = match &detail {
        Some(d) => (format!("{}: {}", name.en, d.en), d.shown.clone()),
        None => (name.en.clone(), String::new()),
    };
    Row { id: id.into(), text: en, shown: name.shown, detail: shown, glyph, act, depth: 0, pick: None, item: None, rich: None, role: None, eye: None }
}

/// Text in both languages from a template and its arguments.
fn words(template: crate::lang::Text, args: &[(&str, &dyn Display)]) -> Both {
    lang::both(template, args)
}

/// A name that is the model's own (a body's, a material's): the same in every language.
fn own(name: &str) -> Both {
    Both { en: name.to_owned(), shown: name.to_owned() }
}

impl Row {
    fn picks(self, pick: Pick) -> Row {
        Row { pick: Some(pick), ..self }
    }

    fn item(self, item: Item) -> Row {
        Row { item: Some(item), ..self }
    }

    /// The row one step under the one above it.
    fn under(self) -> Row {
        Row { depth: 1, ..self }
    }

    /// Whether a double-click edits it.
    pub fn editable(&self) -> bool {
        self.item.is_some() || self.act.edits()
    }

    /// What a double-click (or Edit) asks for: its item's edit dialog, else its command's.
    pub fn edit_action(&self) -> Option<Action> {
        match &self.item {
            Some(item) => Some(Action::EditItem(item.clone())),
            None => self.act.clone().edit(),
        }
    }

    /// Its detail as TeX: its own, else its text's symbols set as mathematics.
    fn detail_tex(&self) -> String {
        self.rich.clone().unwrap_or_else(|| crate::math::texify(&self.detail).into_owned())
    }

    /// Its name and detail as TeX (a card in the 3D view).
    pub fn tex(&self) -> String {
        match self.detail.is_empty() {
            true => crate::math::texify(&self.shown).into_owned(),
            false => format!("{}  ·  {}", crate::math::texify(&self.shown), self.detail_tex()),
        }
    }

    /// Its name and detail and, for a port, its role: what its tooltip says.
    pub fn tip(&self) -> String {
        let whole = if self.detail.is_empty() { self.shown.clone() } else { format!("{}  ·  {}", self.shown, self.detail) };
        match &self.role {
            Some(role) => format!("{whole}\n{}", role.tip()),
            None => whole,
        }
    }
}

/// The row of what `pick` is: its key (its id) and itself.
pub fn row_of(app: &EmcraftApp, pick: Pick) -> Option<(String, Row)> {
    rows_of(app, &[pick]).pop()
}

/// The rows of what `picks` are, in their order (those without a row left out).
pub fn rows_of(app: &EmcraftApp, picks: &[Pick]) -> Vec<(String, Row)> {
    let all: Vec<Row> = tree(app).into_iter().flat_map(|(_, rows)| rows).collect();
    picks.iter().filter_map(|p| all.iter().find(|r| r.pick == Some(*p)).map(|r| (r.id.clone(), r.clone()))).collect()
}

/// The items of the model the selected parts in 3D name, in their order.
pub fn selected_items(app: &EmcraftApp) -> Vec<Item> {
    rows_of(app, app.ui_state.selection()).into_iter().filter_map(|(_, r)| r.item).collect()
}

/// Whether a row shows selected: one of the parts selected in 3D, or (a row with no part) the
/// row clicked.
fn row_selected(app: &EmcraftApp, r: &Row) -> bool {
    match r.pick {
        Some(p) => app.ui_state.selection().contains(&p),
        None => app.ui_state.selection().row() == Some(r.id.as_str()),
    }
}

/// The tree's sections and rows for a session: what the model is made of (its bodies and their
/// materials), the region it is solved in and what bounds it, what drives it, its mesh and its
/// results.
pub fn tree(app: &EmcraftApp) -> Vec<(&'static str, Vec<Row>)> {
    let f = Fmt::new(&app.session);
    let m = app.session.model();
    let mut sections = Vec::new();
    if let Some(board) = m.pcb().filter(|_| m.analysis() == crate::steps::Solver::Pcb) {
        let mut rows = vec![node("pcb", words(t!("PCB"), &[]), Some(own(&board.name)), Glyph::Box, Act::Show(View::Pcb))];
        let nets: std::collections::BTreeSet<_> = board.segments.iter().map(|s| s.net.as_str()).collect();
        for net in nets.into_iter().take(128) {
            let act = Act::Run("view.pcb", serde_json::json!({"net":net,"stage":"net"}));
            rows.push(node(format!("net:{net}"), words(t!("Net"), &[]), Some(own(net)), Glyph::Chart, act).under());
        }
        return vec![(t!("PCB").en(), rows)];
    }
    // The variables the job defined (a study's), not a template's own dimensions: double-click
    // to change one.
    let own_variables = variables(&app.session);
    if !own_variables.is_empty() {
        sections.push((t!("Variables").en(), own_variables));
    }
    sections.extend([
        (t!("Bodies").en(), bodies(m, &f)),
        (t!("Materials").en(), materials(m)),
        (t!("Domain").en(), domain(m, &f)),
        (t!("Excitation").en(), excitation(&app.session, &f)),
        (t!("Mesh").en(), mesh(m)),
        (t!("Results").en(), results(app)),
    ]);
    // Each row's eye, once: what its pick shows in the view, unless the row named its own.
    for (_, rows) in &mut sections {
        for r in rows.iter_mut().filter(|r| r.eye.is_none()) {
            r.eye = r.pick.and_then(|p| p.display_object(m));
        }
    }
    sections
}

fn variables(s: &emcraft_engine::Session) -> Vec<Row> {
    let template: Vec<String> = s.sample().and_then(emcraft_engine::template_parameters).unwrap_or_default().into_iter().map(|p| p.name).collect();
    s.variables()
        .iter()
        .filter(|(name, _)| !template.contains(name))
        .map(|(name, value)| {
            let given = s.history().iter().rev().find(|st| st.defines(name)).and_then(|st| st.param("value").cloned());
            let expression = given.as_ref().and_then(serde_json::Value::as_str).map(|e| format!(" ({e})")).unwrap_or_default();
            let edit = serde_json::json!({"name": name, "value": given.unwrap_or_else(|| serde_json::json!(value))});
            let detail = Both { en: format!("= {value}{expression}"), shown: format!("= {value}{expression}") };
            node(format!("variable:{name}"), own(name), Some(detail), Glyph::Variable, Act::EditWith("project.variable", edit))
        })
        .collect()
}

/// Lengths in the session's display unit, trailing zeros trimmed.
struct Fmt {
    length: emcraft_model::units::Length,
}

impl Fmt {
    fn new(s: &emcraft_engine::Session) -> Fmt {
        Fmt { length: s.display().length }
    }

    fn len(&self, x: f64) -> String {
        format!("{:.4}", self.length.from_si(x)).trim_end_matches('0').trim_end_matches('.').to_owned()
    }

    fn unit(&self) -> &'static str {
        self.length.symbol()
    }
}

/// A material's name as the window says it: the region's own material (`metal` is the perfect
/// conductor) by its name.
fn material_name(name: &str) -> Both {
    match name {
        emcraft_engine::bodies::METAL => words(t!("metal"), &[]),
        // The built-in one, said in the window's language.
        emcraft_model::region::BACKGROUND => words(t!("vacuum"), &[]),
        _ => own(name),
    }
}

/// The glyph of a body made of `material`: metal filled, the region's air dashed, another
/// material hatched.
fn body_glyph(material: &str, region: &str) -> Glyph {
    match material {
        emcraft_engine::bodies::METAL => Glyph::Metal,
        m if m == region => Glyph::Region,
        "clip" => Glyph::Feature,
        _ => Glyph::Dielectric,
    }
}

/// What the model is made of: the waveguide (its sections, layers, strips and vias under it),
/// an imported part's walls and region, and the bodies placed, each with its material.
fn bodies(m: &emcraft_model::Model, f: &Fmt) -> Vec<Row> {
    let (mm, lu) = (|x| f.len(x), f.unit());
    let region = m.domain.solid.as_ref().map_or("vacuum", |s| s.material.as_str());
    let mut rows = Vec::new();
    // A waveguide's bodies: its box, then under it each section, layer, strip and via, in
    // the order placed.
    let edit = || Act::Edit("geometry.waveguide");
    let mut count = [0usize; 4];
    for body in m.bodies().iter().filter(|b| b.is_extruded()) {
        let material = body.material().unwrap_or("vacuum");
        let box_size = match body.shape {
            emcraft_model::Shape::Box { size } => Some(size),
            _ => None,
        };
        let row = match (body.kind, box_size, &body.shape) {
            (Kind::Extruded(_), Some(size), _) => {
                let detail = words(t!("{w} × {h} × {l} {unit}"), args!(w = mm(size.x), h = mm(size.y), l = mm(size.z), unit = lu));
                node("waveguide", words(t!("Waveguide"), &[]), Some(detail), Glyph::Waveguide, edit()).picks(Pick::Guide)
            }
            (Kind::Slab(emcraft_model::Axis::Z), Some(size), _) => {
                let i = count[0];
                count[0] += 1;
                let shown = material_name(material).shown;
                let narrowed = m.waveguide().is_some_and(|w| size.x != w.width.get());
                let detail = if narrowed {
                    words(
                        t!("{length} {unit} · {material} · aperture {aperture}"),
                        args!(length = mm(size.z), unit = lu, material = shown, aperture = mm(size.x)),
                    )
                } else {
                    words(t!("{length} {unit} · {material}"), args!(length = mm(size.z), unit = lu, material = shown))
                };
                let name = words(t!("Section {n}"), args!(n = i + 1));
                node(format!("section:{}", i + 1), name, Some(detail), body_glyph(material, "vacuum"), edit()).under().picks(Pick::Section(i))
            }
            (Kind::Slab(_), Some(size), _) => {
                let i = count[1];
                count[1] += 1;
                let detail =
                    words(t!("{length} {unit} · {material}"), args!(length = mm(size.y), unit = lu, material = material_name(material).shown));
                node(format!("layer:{}", i + 1), words(t!("Layer {n}"), args!(n = i + 1)), Some(detail), Glyph::Dielectric, edit())
                    .under()
                    .picks(Pick::Layer(i))
            }
            (Kind::Cells, _, emcraft_model::Shape::Sheet { size: [_, width] }) => {
                let i = count[2];
                count[2] += 1;
                let detail = words(t!("metal · y {y} · width {width}"), args!(y = mm(body.at.y), width = mm(width.get())));
                node(format!("strip:{}", i + 1), words(t!("Strip {n}"), args!(n = i + 1)), Some(detail), Glyph::Metal, edit())
                    .under()
                    .picks(Pick::Strip(i))
            }
            (Kind::Cells, Some(size), _) => {
                let i = count[3];
                count[3] += 1;
                let detail = words(
                    t!("metal · {wx} × {wz} {unit} · y {y0} to {y1}"),
                    args!(wx = mm(size.x), wz = mm(size.z), unit = lu, y0 = mm(body.at.y), y1 = mm(body.at.y + size.y)),
                );
                node(format!("via:{}", i + 1), words(t!("Via {n}"), args!(n = i + 1)), Some(detail), Glyph::Metal, edit()).under().picks(Pick::Via(i))
            }
            _ => continue,
        };
        rows.push(row);
    }
    // A board solved by the finite elements (a board's PCB analysis has its own tree, above):
    // a row like the other bodies, the 3D view kept (its route view is the PCB analysis').
    if let Some(board) = m.pcb() {
        rows.push(node("board", words(t!("PCB"), &[]), Some(own(&board.name)), Glyph::Part, Act::None));
    }
    // An imported part's walls and region body: a double-click gives them another material.
    for (i, b) in emcraft_engine::bodies::parts(m).into_iter().enumerate() {
        let (detail, glyph) = if b.material == emcraft_engine::bodies::METAL {
            (words(t!("metal · imported"), &[]), Glyph::Part)
        } else {
            (words(t!("the region · imported"), &[]), Glyph::Region)
        };
        let item = Item::new("geometry.body.material", "", "body", serde_json::json!(b.name));
        rows.push(node(format!("body:{}", b.name), own(&b.name), Some(detail), glyph, Act::None).picks(Pick::Part(i)).item(item));
    }
    // The bodies placed, by what they are made of; a double-click changes it.
    for b in emcraft_engine::bodies::bodies(m) {
        let pick = match b.at {
            emcraft_engine::bodies::BodyRef::Feature(i) => Pick::Feature(i),
            emcraft_engine::bodies::BodyRef::Inclusion(i) => Pick::Inclusion(i),
        };
        let item = Item::new("geometry.body.edit", "geometry.body.delete", "body", serde_json::json!(b.name));
        let detail = words(t!("{material} · {shape}"), args!(material = material_name(&b.material).shown, shape = b.shape.to_lowercase()));
        let glyph = body_glyph(&b.material, region);
        rows.push(node(format!("body:{}", b.name), own(&b.name), Some(detail), glyph, Act::None).picks(pick).item(item));
    }
    rows
}

/// The materials, each with its properties and what is made of it; the perfect conductor too
/// while something is.
fn materials(m: &emcraft_model::Model) -> Vec<Row> {
    let names = emcraft_engine::bodies::bodies(m);
    let users = |name: &str| -> Vec<String> {
        let mut out: Vec<String> = m
            .material_uses(name)
            .into_iter()
            .map(|u| match u {
                emcraft_model::MaterialUse::Section(k) => lang::trf(t!("Section {n}"), args!(n = k + 1)),
                emcraft_model::MaterialUse::Layer(k) => lang::trf(t!("Layer {n}"), args!(n = k + 1)),
                emcraft_model::MaterialUse::Region => lang::tr(t!("region")).to_owned(),
                emcraft_model::MaterialUse::Inclusion(i) => {
                    names.iter().find(|b| b.at == emcraft_engine::bodies::BodyRef::Inclusion(i)).map_or_else(String::new, |b| b.name.clone())
                }
            })
            .collect();
        out.retain(|s| !s.is_empty());
        out
    };
    let mut rows: Vec<Row> = m
        .materials
        .iter()
        .enumerate()
        .map(|(i, mat)| {
            let used = users(&mat.name);
            let tail = if used.is_empty() { String::new() } else { format!("  ·  {}", used.join(", ")) };
            let props = crate::math::material(mat);
            let detail = Both { en: format!("{}{tail}", mat.summary()), shown: format!("{}{tail}", mat.summary()) };
            Row {
                rich: Some(format!("{props}{}", tail.replace('$', "\\$"))),
                ..node(format!("material:{}", mat.name), material_name(&mat.name), Some(detail), Glyph::Material, Act::None)
                    .picks(Pick::Material(i))
                    .item(Item::new("model.material.edit", "model.material.delete", "name", serde_json::json!(mat.name)))
            }
        })
        .collect();
    // The perfect conductor, while a body or the walls are made of it.
    let mut metal: Vec<String> =
        emcraft_engine::bodies::parts(m).into_iter().filter(|b| b.material == emcraft_engine::bodies::METAL).map(|b| b.name).collect();
    metal.extend(names.iter().filter(|b| b.material == emcraft_engine::bodies::METAL).map(|b| b.name.clone()));
    if let Some(w) = m.waveguide() {
        metal.extend((0..w.strips.len()).map(|k| lang::trf(t!("Strip {n}"), args!(n = k + 1))));
        metal.extend((0..w.vias.len()).map(|k| lang::trf(t!("Via {n}"), args!(n = k + 1))));
    }
    if !metal.is_empty() {
        let detail = Both {
            en: format!("perfect conductor  ·  {}", metal.join(", ")),
            shown: format!("{}  ·  {}", lang::tr(t!("perfect conductor")), metal.join(", ")),
        };
        rows.push(node("material:metal", words(t!("metal"), &[]), Some(detail), Glyph::Metal, Act::None));
    }
    rows
}

/// The region solved and what bounds it: its shape and filling, then its outer boundary and
/// the planes that cut it.
fn domain(m: &emcraft_model::Model, f: &Fmt) -> Vec<Row> {
    let (mm, lu) = (|x| f.len(x), f.unit());
    let mut rows = Vec::new();
    let d = &m.domain;
    if let Some(solid) = &d.solid {
        let material = material_name(&solid.material).shown;
        let block = solid.shape.box_bounds().filter(|_| !matches!(solid.shape, emcraft_model::Shape::Box { .. }));
        let (detail, act) = match &solid.shape {
            // An automatic region, or a box off the origin: its sides (ADR 0005).
            _ if let Some(b) = block => {
                let size = b.max - b.min;
                let args = args!(x = mm(size.x), y = mm(size.y), z = mm(size.z), unit = lu, material = material);
                let detail = if matches!(d.region, emcraft_model::Region::Auto { .. }) {
                    words(t!("automatic · {x} × {y} × {z} {unit} · {material}"), args)
                } else {
                    words(t!("box {x} × {y} × {z} {unit} · {material}"), args)
                };
                (detail, Act::Edit("model.region"))
            }
            // An imported region: a double-click changes what fills it (its shape is its part's).
            emcraft_model::Shape::Surface { name, .. } if name == emcraft_model::CAVITY => {
                (words(t!("the air the part encloses · {material}"), args!(material = material)), Act::Edit("model.background"))
            }
            emcraft_model::Shape::Surface { name, .. } => {
                (words(t!("inside {name} · {material}"), args!(name = name, material = material)), Act::Edit("model.background"))
            }
            other => {
                (words(t!("{shape} · {material}"), args!(shape = other.kind().to_lowercase(), material = material)), Act::Edit("geometry.solid"))
            }
        };
        rows.push(node("region", words(t!("Region"), &[]), Some(detail), Glyph::Region, act).picks(Pick::Solid));
    } else if m.waveguide().is_some() {
        rows.push(node(
            "region",
            words(t!("Region"), &[]),
            Some(words(t!("inside the waveguide"), &[])),
            Glyph::Region,
            Act::Edit("geometry.waveguide"),
        ));
    }
    if rows.is_empty() {
        return rows;
    }
    // The outer boundary: open through a layer, else conducting walls.
    let outer = match &d.open {
        Some(open) => {
            let detail = if open.shape == emcraft_model::OpenShape::Sphere {
                words(t!("open · spherical PML {thickness} {unit}"), args!(thickness = mm(open.thickness.get()), unit = lu))
            } else {
                words(t!("open · PML {thickness} {unit}"), args!(thickness = mm(open.thickness.get()), unit = lu))
            };
            node("outer", words(t!("Outer boundary"), &[]), Some(detail), Glyph::Open, Act::Edit("model.open"))
        }
        None => {
            let wall = match d.wall_conductivity {
                Some(sigma) => words(t!("σ = {sigma} S/m"), args!(sigma = format!("{sigma:.3e}"))),
                None => words(t!("perfect conductor"), &[]),
            };
            let detail = match d.wall_thickness {
                Some(t) => {
                    let thick = words(t!("{thickness} {unit} thick"), args!(thickness = mm(t.get()), unit = lu));
                    Both { en: format!("{} · {}", wall.en, thick.en), shown: format!("{} · {}", wall.shown, thick.shown) }
                }
                None => wall,
            };
            node("outer", words(t!("Outer boundary"), &[]), Some(detail), Glyph::Walls, Act::Edit("model.walls"))
        }
    };
    rows.push(outer.under());
    for plane in &d.symmetry {
        let axis = plane.axis.id();
        let (detail, glyph) = if plane.kind == emcraft_model::SymmetryKind::Electric {
            (words(t!("{at} {unit} · electric wall"), args!(at = mm(plane.at), unit = lu)), Glyph::SymmetryElectric)
        } else {
            (words(t!("{at} {unit} · magnetic wall"), args!(at = mm(plane.at), unit = lu)), Glyph::SymmetryMagnetic)
        };
        // Its own plane's values, not the first one's.
        let edit = Act::EditWith("model.symmetry", serde_json::json!({"axis": axis}));
        let name = words(t!("Symmetry plane {axis}"), args!(axis = axis));
        let eye = Some(emcraft_engine::visibility::DisplayObject::Geometry(emcraft_engine::visibility::GeometryObject::Symmetry(plane.axis)));
        rows.push(Row { eye, ..node(format!("symmetry:{axis}"), name, Some(detail), glyph, edit).under() });
    }
    if let Some(r) = &d.rotation {
        let detail = words(t!("{n} sectors about z · harmonic {m}"), args!(n = r.n, m = r.m));
        rows.push(node("rotation", words(t!("Rotational symmetry"), &[]), Some(detail), Glyph::Symmetry, Act::Edit("model.rotation")).under());
    }
    if let Some(p) = &d.periodic {
        let detail = Both { en: periodic_text(p), shown: periodic_text(p) };
        rows.push(node("periodic", words(t!("Periodic boundaries"), &[]), Some(detail), Glyph::Periodic, Act::Edit("model.periodic")).under());
    }
    rows
}

/// What drives the model: its waveguide ports, lumped ports and sources.
fn excitation(s: &emcraft_engine::Session, f: &Fmt) -> Vec<Row> {
    let m = s.model();
    let roles = port_roles(s);
    let l = s.display().length;
    let mut rows: Vec<Row> = m
        .ports
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let item = Item::new("model.port.edit", "model.port.delete", "name", serde_json::json!(p.name));
            let name = words(t!("Port {name}"), args!(name = p.name));
            let detail = port_detail(p, l);
            Row { role: roles.get(i).cloned(), ..node(format!("port:{}", p.name), name, Some(detail), Glyph::Port, Act::Show(View::PortModes)) }
                .picks(Pick::Port(i))
                .item(item)
        })
        .collect();
    rows.extend(m.lumped.iter().enumerate().map(|(i, p)| {
        let item = Item::new("model.port.lumped.edit", "model.port.lumped.delete", "name", serde_json::json!(p.name));
        let detail = words(t!("{impedance} Ω"), args!(impedance = p.impedance));
        node(format!("lumped:{}", p.name), words(t!("Lumped port {name}"), args!(name = p.name)), Some(detail), Glyph::Lumped, Act::None)
            .picks(Pick::Lumped(i))
            .item(item)
    }));
    rows.extend(m.sources.iter().enumerate().map(|(i, src)| {
        let item = Item::new("source.edit", "source.delete", "index", serde_json::json!(i + 1));
        let id = format!("source:{}", i + 1);
        match src {
            emcraft_model::Source::Dipole { length, current, .. } => {
                let detail = words(t!("{length} {unit} · {current} A"), args!(length = f.len(length.get()), unit = f.unit(), current = current));
                node(id, words(t!("Dipole"), &[]), Some(detail), Glyph::Dipole, Act::None).item(item)
            }
            emcraft_model::Source::PlaneWave { direction: emcraft_model::Vec3 { x, y, z }, .. } => {
                let detail = words(t!("along {direction}"), args!(direction = format!("[{x:.2}, {y:.2}, {z:.2}]")));
                node(id, words(t!("Plane wave"), &[]), Some(detail), Glyph::PlaneWave, Act::None).item(item)
            }
        }
    }));
    rows
}

/// A waveguide port's detail: where it lies and its modes (`y = 0 mm · 4 modes · 50 Ω`).
fn port_detail(p: &emcraft_model::Port, l: emcraft_model::units::Length) -> Both {
    let z = p.reference.map(|z| format!(" · {z} Ω")).unwrap_or_default();
    let template = match (p.face.is_some(), p.end) {
        (true, _) => {
            lang::pick(p.modes as usize, t!("{axis} = {at} {unit} · {n} mode{impedance}"), t!("{axis} = {at} {unit} · {n} modes{impedance}"))
        }
        (false, emcraft_model::End::Start) => lang::pick(p.modes as usize, t!("start · {n} mode{impedance}"), t!("start · {n} modes{impedance}")),
        (false, emcraft_model::End::End) => lang::pick(p.modes as usize, t!("end · {n} mode{impedance}"), t!("end · {n} modes{impedance}")),
    };
    let (axis, at) = face_place(p, l);
    lang::both(template, args!(n = p.modes, impedance = z, axis = axis, at = at, unit = l.symbol()))
}

/// A waveguide port as the window names it everywhere (the Excitation step, the 3D view's
/// legend says `Port 1 (start)`): `Port 1 (start): 4 modes, 50 Ω`; a port on a face by its
/// plane, in display units `l`: `Port 1 (y = 100 mm): 4 modes`.
pub fn port_text(p: &emcraft_model::Port, l: emcraft_model::units::Length) -> Both {
    let z = p.reference.map(|z| format!(", {z} Ω")).unwrap_or_default();
    let template = match (p.face.is_some(), p.end) {
        (true, _) => lang::pick(
            p.modes as usize,
            t!("Port {name} ({axis} = {at} {unit}): {n} mode{impedance}"),
            t!("Port {name} ({axis} = {at} {unit}): {n} modes{impedance}"),
        ),
        (false, emcraft_model::End::Start) => {
            lang::pick(p.modes as usize, t!("Port {name} (start): {n} mode{impedance}"), t!("Port {name} (start): {n} modes{impedance}"))
        }
        (false, emcraft_model::End::End) => {
            lang::pick(p.modes as usize, t!("Port {name} (end): {n} mode{impedance}"), t!("Port {name} (end): {n} modes{impedance}"))
        }
    };
    let (axis, at) = face_place(p, l);
    lang::both(template, args!(name = p.name, n = p.modes, impedance = z, axis = axis, at = at, unit = l.symbol()))
}

/// A face port's plane: its axis and where it lies across it, in display units `l`, to a
/// thousandth (none for an end port).
pub fn face_place(p: &emcraft_model::Port, l: emcraft_model::units::Length) -> (&'static str, String) {
    match p.face {
        Some(f) => (f.axis.id(), format!("{}", (l.from_si(f.at) * 1e3).round() / 1e3 + 0.0)),
        None => ("", String::new()),
    }
}

/// The periodic boundaries in brief: the axes tied and, across x or y, the incident wave.
pub(crate) fn periodic_text(p: &emcraft_model::Periodic) -> String {
    let axes: Vec<&str> = p.axes().into_iter().map(|d| ["x", "y", "z"][d.min(2)]).collect();
    if !(p.x || p.y) {
        return axes.join(", ");
    }
    let pol = if p.polarization == emcraft_model::Polarization::Te { "TE" } else { "TM" };
    format!("{}, θ = {:.1}°, φ = {:.1}°, {pol}", axes.join(", "), p.theta.to_degrees(), p.phi.to_degrees())
}

fn mesh(m: &emcraft_model::Model) -> Vec<Row> {
    let detail = match &m.mesh {
        Some(mesh) => words(t!("{nodes} nodes · {tets} tetrahedra"), args!(nodes = mesh.nodes.len(), tets = mesh.tets.len())),
        None => words(t!("not meshed"), &[]),
    };
    vec![node("mesh", words(t!("Mesh"), &[]), Some(detail), Glyph::Mesh, Act::Edit("mesh.generate"))]
}

fn results(app: &EmcraftApp) -> Vec<Row> {
    let r = app.session.results();
    let fu = app.session.display().frequency;
    let mut rows = Vec::new();
    let name = |t: crate::lang::Text| words(t, &[]);
    if let Some(sw) = &r.sweep {
        let detail = words(t!("{n} frequencies · {unknowns} unknowns"), args!(n = sw.results.len(), unknowns = sw.driven.dofs.unknowns));
        rows.push(node("result:sweep", name(t!("Sweep")), Some(detail), Glyph::Chart, Act::Show(View::SParameters)));
        rows.push(node("result:fields", name(t!("Fields")), None, Glyph::FieldLines, Act::Show(View::Field)).under());
    }
    if let Some(e) = &r.eigen {
        let detail = words(t!("{n} modes"), args!(n = e.modes.len()));
        rows.push(node("result:eigenmodes", name(t!("Eigenmodes")), Some(detail), Glyph::Eigen, Act::Show(View::Eigenmodes)));
    }
    if let Some(rev) = &r.revolution {
        let detail = words(t!("m = {m} · {n} frequencies"), args!(m = rev.m, n = rev.data.frequencies().len()));
        rows.push(node("result:revolution", name(t!("Body of revolution")), Some(detail), Glyph::Chart, Act::Show(View::SParameters)));
        if rev.bloch.is_some() {
            rows.push(node("result:pulse", name(t!("Pulse")), None, Glyph::KindPulse, Act::Show(View::Pulse)).under());
        }
    }
    if let Some(d) = &r.dispersion {
        let detail = words(t!("{n} phases"), args!(n = d.points.len()));
        rows.push(node("result:dispersion", name(t!("Dispersion")), Some(detail), Glyph::Dispersion, Act::Show(View::Dispersion)));
    }
    if let Some(td) = &r.time_domain {
        let detail = words(t!("{n} frequencies"), args!(n = td.frequencies.len()));
        rows.push(node("result:time_domain", name(t!("Time domain")), Some(detail), Glyph::Time, Act::Show(View::SParameters)));
    }
    if let Some(rad) = &r.radiation {
        let at = format!("{:.4} {}", fu.from_si(rad.result.frequency), fu.symbol());
        rows.push(node(
            "result:radiation",
            name(t!("Radiation")),
            Some(words(t!("at {frequency}"), args!(frequency = at))),
            Glyph::Radiation,
            Act::Show(View::FarField),
        ));
    }
    if r.td_radiation.is_some() {
        rows.push(node("result:td_radiation", name(t!("Time-domain radiation")), None, Glyph::Radiation, Act::Show(View::FarField)));
    }
    // Port modes open from the Excitation step's table; no row of their own.
    if app.replies.compare.is_some() {
        rows.push(node("result:compare", name(t!("Comparison with theory")), None, Glyph::Compare, Act::Show(View::Compare)));
    }
    if let Some(study) = &r.study {
        let (title, detail) = match study.kind {
            emcraft_engine::study::Kind::Sweep if study.is_grid() => (t!("Parameter sweep"), t!("{names} · {n} points")),
            emcraft_engine::study::Kind::Sweep => (t!("Parameter sweep"), t!("{names} · {n} values")),
            emcraft_engine::study::Kind::Optimize => (t!("Optimization"), t!("{names} · {n} solves")),
        };
        let glyph = if study.kind == emcraft_engine::study::Kind::Sweep { Glyph::ParamSweep } else { Glyph::Optimize };
        let detail = words(detail, args!(names = study.variables.join(", "), n = study.rows.len()));
        rows.push(node("result:study", name(title), Some(detail), glyph, Act::Show(View::Study)));
    }
    rows
}

/// The browser card.
pub fn show(app: &EmcraftApp, ui: &mut egui::Ui, out: &mut Actions) {
    let t = &Tokens::get();
    egui::Panel::left("emcraft_browser")
        .default_size(300.0)
        .size_range(200.0..=480.0)
        .resizable(true)
        .show_separator_line(false)
        .frame(egui::Frame::NONE.fill(t.viewport_top).inner_margin(egui::Margin { left: 6, right: 0, top: 0, bottom: 6 }))
        .show(ui, |ui| {
            t.card(t.panel).inner_margin(egui::Margin::symmetric(6, 6)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.set_height(ui.available_height());
                scenario::publish_rect(Fixed::Browser, ui.max_rect());
                ui.text(Style::SmallDim, t!("BROWSER"));
                let tree = tree(app);
                delete_key(app, ui, &tree, out);
                let stale = crate::steps::results_stale(app);
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    for (section, rows) in &tree {
                        // An empty section stays folded, its heading dim.
                        let folded = rows.is_empty() || app.browser.folded.iter().any(|f| f == section);
                        section_header(ui, section, rows.len(), folded, out);
                        if folded {
                            continue;
                        }
                        for r in rows {
                            browser_row(app, ui, r, stale && *section == "Results", out);
                        }
                    }
                });
            });
        });
}

/// A section's glyph, before its heading.
fn section_glyph(section: &str) -> Glyph {
    match section {
        "Variables" => Glyph::Variable,
        "Bodies" => Glyph::Box,
        "Materials" => Glyph::Material,
        "Domain" => Glyph::Region,
        "Excitation" => Glyph::Port,
        "Mesh" => Glyph::Mesh,
        "Results" => Glyph::Chart,
        "PCB" => Glyph::Part,
        _ => Glyph::Blank,
    }
}

/// A section's heading row: its chevron, glyph, name and count, the heading standing out
/// ([`crate::theme::Emphasis::Primary`]); a click folds or unfolds it.
fn section_header(ui: &mut egui::Ui, section: &'static str, count: usize, folded: bool, out: &mut Actions) {
    let t = &Tokens::get();
    let e = crate::theme::Emphasis::Primary;
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), t.size(e) + 14.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, section));
    if resp.hovered() {
        ui.painter().rect_filled(r, t.radius_sm, t.hover);
    }
    chevron(ui, pos2(r.left() + 9.0, r.center().y), !folded, t.text_dim);
    // An empty section's heading is dim.
    let ink = if count == 0 { t.text_dim } else { t.ink(e) };
    let icon = t.size(e) + 2.0;
    let glyph = Rect::from_min_size(pos2(r.left() + 18.0, r.center().y - icon / 2.0), vec2(icon, icon));
    glyphs::paint(ui.painter(), glyph, section_glyph(section), ink, if count == 0 { t.text_dim } else { t.accent });
    let text = pos2(glyph.right() + 6.0, r.center().y);
    ui.painter().text(text, Align2::LEFT_CENTER, lang::lookup(section), FontId::proportional(t.size(e)), ink);
    ui.painter().text(pos2(r.right() - 6.0, r.center().y), Align2::RIGHT_CENTER, count.to_string(), FontId::proportional(t.font_small), t.text_dim);
    scenario::publish_target(Handle::Fold(section), r);
    if resp.clicked() {
        out.push(Action::Fold((*section).to_owned()));
    }
}

/// A row: its glyph, its name and, dimmer, its detail in one line (the detail cut short first),
/// the eye of a displayed object, a port's role, the out-of-date mark; a click selects it (or
/// toggles it in the selection), a double click edits it.
fn browser_row(app: &EmcraftApp, ui: &mut egui::Ui, r: &Row, out_of_date: bool, out: &mut Actions) {
    let t = &Tokens::get();
    let key = r.id.clone();
    let selected = row_selected(app, r);
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
    let indent = 20.0 + 16.0 * f32::from(r.depth);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, r.tip()));
    if selected {
        ui.painter().rect_filled(rect, t.radius_sm, t.selected_bg);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, t.radius_sm, t.hover);
    }
    let mark = Rect::from_center_size(pos2(rect.left() + indent + 7.0, rect.center().y), vec2(15.0, 15.0));
    match &r.role {
        Some(role) => role_mark(ui.painter(), mark, role.input, role.colour()),
        None => glyphs::paint(ui.painter(), mark, r.glyph, t.icon, t.accent),
    }
    // A result of a model changed after its run: dimmed, marked at the right.
    let ink = if selected {
        t.selected_ink
    } else if out_of_date {
        t.text_dim
    } else {
        t.text
    };
    let mut clip = rect;
    if let Some(object) = &r.eye {
        clip.max.x = visibility_eye(app, ui, rect, object, out).left() - 3.0;
    }
    if out_of_date {
        let c = pos2(rect.right() - 12.0, rect.center().y);
        crate::steps::stale_mark(ui.painter(), c);
        clip.max.x -= 24.0;
    }
    if let Some(role) = &r.role {
        clip.max.x = role_pill(ui, rect, role).left() - 4.0;
    }
    // The name whole, then the detail dimmer, cut short with an ellipsis where it meets the
    // pill, the eye or the edge.
    let left = rect.left() + indent + 19.0;
    let painter = ui.painter().with_clip_rect(clip);
    let name = ui.painter().layout_job(crate::math::job(&crate::math::texify(&r.shown), t.font_body, ink));
    let name_width = name.size().x.min((clip.right() - left).max(8.0));
    painter.galley(pos2(left, rect.center().y - name.size().y / 2.0), name, ink);
    if !r.detail.is_empty() {
        let from = left + name_width + 8.0;
        let dim = if selected { t.selected_ink.gamma_multiply(0.75) } else { t.text_dim };
        let mut job = crate::math::job(&r.detail_tex(), t.font_small, dim);
        job.wrap =
            egui::text::TextWrapping {
                max_width: (clip.right() - from).max(1.0), max_rows: 1, break_anywhere: true, overflow_character: Some('…')
            };
        let detail = ui.painter().layout_job(job);
        if from < clip.right() - 12.0 {
            painter.galley(pos2(from, rect.center().y - detail.size().y / 2.0), detail, dim);
        }
    }
    scenario::publish_target(Handle::Row(&r.id), rect);
    let resp = if out_of_date {
        let tip = lang::trf(t!("{row} — out of date"), args!(row = r.tip()));
        crate::math::tip(resp, &tip)
    } else {
        crate::math::tip(resp, &r.tip())
    };
    // Shift or Ctrl toggles a part in what is selected, as a click in 3D.
    let toggles = ui.input(|i| i.modifiers.shift || i.modifiers.ctrl || i.modifiers.command);
    if resp.clicked()
        && toggles
        && let Some(p) = r.pick
    {
        let mut picks = app.ui_state.selection().clone();
        picks.toggle(p);
        let row = rows_of(app, &picks).pop().map(|(k, _)| k);
        out.push(Action::Select { row, picks: picks.to_vec() });
    } else if resp.clicked() {
        out.push(Action::Select { row: Some(key.clone()), picks: r.pick.into_iter().collect() });
        match &r.act {
            Act::Show(v) => out.push(Action::ShowView(*v)),
            Act::Run(command, params) => out.push(Action::run(command, params.clone())),
            _ => {}
        }
    }
    if resp.double_clicked() && !app.busy() {
        out.extend(r.edit_action());
    }
    if r.editable() || r.pick.is_some() {
        resp.context_menu(|ui| row_menu(app, r, &key, ui, out));
    }
}

/// The eye at the right of `rect` that shows or hides `object`; its rectangle.
fn visibility_eye(app: &EmcraftApp, ui: &egui::Ui, rect: Rect, object: &emcraft_engine::visibility::DisplayObject, out: &mut Actions) -> Rect {
    let t = &Tokens::get();
    let visible = app.session.views().visible(object);
    let name = object.name();
    let kind = object.kind();
    let eye = Rect::from_center_size(pos2(rect.right() - 13.0, rect.center().y), vec2(22.0, 20.0));
    let response = ui.interact(eye, ui.id().with(("object_eye", kind, &name)), Sense::click());
    scenario::publish_target(
        Handle::Choice(
            match kind {
                "material" => "material_visibility",
                _ => "geometry_visibility",
            },
            &name,
        ),
        eye,
    );
    let ink = if visible { t.text } else { t.text_dim };
    let c = eye.center();
    let points = vec![
        c + vec2(-8.0, 0.0),
        c + vec2(-4.0, -4.0),
        c + vec2(0.0, -5.0),
        c + vec2(4.0, -4.0),
        c + vec2(8.0, 0.0),
        c + vec2(4.0, 4.0),
        c + vec2(0.0, 5.0),
        c + vec2(-4.0, 4.0),
    ];
    ui.painter().add(egui::Shape::closed_line(points, Stroke::new(1.2, ink)));
    ui.painter().circle_filled(eye.center(), 2.0, ink);
    if !visible {
        ui.painter().line_segment([eye.left_bottom() + vec2(3.0, -3.0), eye.right_top() + vec2(-3.0, 3.0)], Stroke::new(1.4, ink));
    }
    if response.on_hover_text(lang::tr(if visible { t!("Hide object") } else { t!("Show object") })).clicked() {
        out.push(Action::run("view.visibility", serde_json::json!({"kind":kind,"name":name,"visible":!visible})));
    }
    eye
}

/// A port's role at the right of its row: its mark's colour as a soft pill, the role in short
/// with the S-parameter set as mathematics. The rectangle it took.
fn role_pill(ui: &egui::Ui, rect: Rect, role: &PortRole) -> Rect {
    let t = &Tokens::get();
    let galley = ui.painter().layout_job(crate::math::rich(&role.text(), t.font_small, t.text));
    let size = galley.size() + vec2(12.0, 2.0);
    let pill = Rect::from_min_size(pos2(rect.right() - 4.0 - size.x, rect.center().y - size.y / 2.0), size);
    let ink = role.colour();
    ui.painter().rect_filled(pill, size.y / 2.0, ink.gamma_multiply(0.2));
    ui.painter().rect_stroke(pill, size.y / 2.0, Stroke::new(1.0, ink), egui::StrokeKind::Inside);
    ui.painter().galley(pill.min + vec2(6.0, 1.0), galley, t.text);
    pill
}

/// A row's right-click menu: Edit, Delete (its item's), Show in 3D (what it is there); a port
/// that is an output can be made the input (`view.driven`).
fn row_menu(app: &EmcraftApp, r: &Row, key: &str, ui: &mut egui::Ui, out: &mut Actions) {
    if let Some(role) = r.role.as_ref().filter(|role| !role.input) {
        let drive = egui::Button::new(lang::tr(t!("Make the input")));
        let params = serde_json::json!({"port": role.name});
        ui.act(true, drive, Fixed::RowMenuDrive.into(), Action::run("view.driven", params), out);
    }
    if let Some(edit) = r.edit_action() {
        ui.act(true, egui::Button::new(lang::tr(t!("Edit…"))), Fixed::RowMenuEdit.into(), edit, out);
    }
    if let Some(item) = &r.item {
        // A row among several parts selected deletes them all.
        let several = r.pick.is_some_and(|p| app.ui_state.selection().len() > 1 && app.ui_state.selection().contains(&p));
        let items = if several { selected_items(app) } else { vec![item.clone()] };
        let delete = egui::Button::new(lang::tr(t!("Delete"))).shortcut_text("Del");
        ui.act(true, delete, Fixed::RowMenuDelete.into(), Action::DeleteItems(items), out);
    }
    if let Some(pick) = r.pick {
        let show = egui::Button::new(lang::tr(t!("Show in 3D")));
        if ui.act(true, show, Fixed::RowMenuShow.into(), Action::Select { row: Some(key.to_owned()), picks: vec![pick] }, out).clicked() {
            out.push(Action::SetView(View::Model));
        }
    }
}

/// Delete deletes the items of the parts selected in 3D, else the selected row's (when no field
/// takes the key and nothing asks).
fn delete_key(app: &EmcraftApp, ui: &egui::Ui, tree: &[(&'static str, Vec<Row>)], out: &mut Actions) {
    if app.ui_state.selection().is_clear() {
        return;
    }
    if app.busy() || app.ui_state.dialog().is_some() || app.ui_state.ask().is_some() || ui.ctx().text_edit_focused() {
        return;
    }
    if !ui.input(|i| i.key_pressed(egui::Key::Delete)) {
        return;
    }
    let picked = selected_items(app);
    if !picked.is_empty() {
        out.push(Action::DeleteItems(picked));
        return;
    }
    let Some(selected) = app.ui_state.selection().row() else { return };
    let item = tree.iter().flat_map(|(_, rows)| rows.iter()).find(|r| r.id == selected);
    if let Some(item) = item.and_then(|r| r.item.clone()) {
        out.push(Action::DeleteItems(vec![item]));
    }
}

/// A disclosure chevron: down when open, right when closed.
fn chevron(ui: &egui::Ui, c: egui::Pos2, open: bool, ink: egui::Color32) {
    let d = 3.5;
    let pts = if open {
        [c + vec2(-d, -d / 2.0), c + vec2(0.0, d / 2.0), c + vec2(d, -d / 2.0)]
    } else {
        [c + vec2(-d / 2.0, -d), c + vec2(d / 2.0, 0.0), c + vec2(-d / 2.0, d)]
    };
    ui.painter().add(egui::Shape::line(pts.to_vec(), Stroke::new(1.4, ink)));
}
