//! The browser's rows: the session's model and results as a tree, as the model is made — its
//! bodies and what each is made of, the materials (and what uses each), the region solved and
//! what bounds it, what drives it, its mesh and its results; a system's source, stages and
//! receiver. Each row names what it is for good (its `id`), says it in both languages, carries
//! its glyph and what a click or double-click asks for, and the part of the 3D view it is (its
//! [`Pick`]). Each window draws the rows its own way; the 3D view names what is picked by them
//! ([`rows_of`]).

use std::fmt::Display;

use emcraft_engine::Session;
use emcraft_engine::workflow::Analysis;
use emcraft_model::Kind;
use serde_json::json;

use super::Item;
use crate::glyph::Glyph;
use crate::lang::{self, Both, args};
use crate::pick::Pick;
use crate::replies::Replies;
use crate::view::View;

/// The stage `name` as an item of the model, which its row edits and deletes.
pub fn stage_item(name: &str) -> Item {
    Item::new("system.stage.edit", "system.stage.delete", "stage", json!(name))
}

/// The row of what `pick` is: its key (its id) and itself.
pub fn row_of(session: &Session, replies: &Replies, pick: Pick) -> Option<(String, Row)> {
    rows_of(session, replies, &[pick]).pop()
}

/// The rows of what `picks` are, in their order (those without a row left out).
pub fn rows_of(session: &Session, replies: &Replies, picks: &[Pick]) -> Vec<(String, Row)> {
    let all: Vec<Row> = tree(session, replies).into_iter().flat_map(|(_, rows)| rows).collect();
    picks.iter().filter_map(|p| all.iter().find(|r| r.pick == Some(*p)).map(|r| (r.id.clone(), r.clone()))).collect()
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
    /// A SolidCraft design of the user's, by its file: the window opens it in SolidCraft and
    /// reads it again when it is saved there (docs/adr/0010-geometry-from-solidcraft.md).
    Design(String),
}

impl Act {
    /// Whether a double-click edits.
    pub fn edits(&self) -> bool {
        matches!(self, Act::Edit(_) | Act::EditWith(..))
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

    /// Its detail as TeX: its own, else its text's symbols set as mathematics.
    pub fn detail_tex(&self) -> String {
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

/// The tree's sections and rows for a session: what the model is made of (its bodies and their
/// materials), the region it is solved in and what bounds it, what drives it, its mesh and its
/// results.
pub fn tree(session: &Session, replies: &Replies) -> Vec<(&'static str, Vec<Row>)> {
    let f = Fmt::new(session);
    let m = session.model();
    let mut sections = Vec::new();
    if let Some(board) = m.pcb().filter(|_| m.analysis() == Analysis::Pcb) {
        let mut rows = vec![node("pcb", words(t!("PCB"), &[]), Some(own(&board.name)), Glyph::Box, Act::Show(View::Pcb))];
        let nets: std::collections::BTreeSet<_> = board.segments.iter().map(|s| s.net.as_str()).collect();
        for net in nets.into_iter().take(128) {
            let act = Act::Run("view.pcb", serde_json::json!({"net":net,"stage":"net"}));
            rows.push(node(format!("net:{net}"), words(t!("Net"), &[]), Some(own(net)), Glyph::Chart, act).under());
        }
        return vec![(t!("PCB").en(), rows)];
    }
    if m.analysis() == Analysis::System {
        return system_tree(session);
    }
    // The variables the job defined (a study's), not a template's own dimensions: double-click
    // to change one.
    let own_variables = variables(session);
    if !own_variables.is_empty() {
        sections.push((t!("Parameters").en(), own_variables));
    }
    sections.extend([
        (t!("Bodies").en(), design_rows(session).into_iter().chain(bodies(m, &f)).collect()),
        (t!("Materials").en(), materials(m)),
        (t!("Domain").en(), domain(m, &f)),
        (t!("Excitation").en(), excitation(session, &f)),
        (t!("Mesh").en(), mesh(m)),
        (t!("Results").en(), results(session, replies)),
    ]);
    // Each row's eye, once: what its pick shows in the view, unless the row named its own.
    for (_, rows) in &mut sections {
        for r in rows.iter_mut().filter(|r| r.eye.is_none()) {
            r.eye = r.pick.and_then(|p| p.display_object(m));
        }
    }
    sections
}

/// A system's tree: its variables, the source, the stages in order (each edited and deleted from
/// its row), the receiver, the measurements its S-parameter stages read and its budget.
fn system_tree(s: &Session) -> Vec<(&'static str, Vec<Row>)> {
    let m = s.model();
    let fu = s.display().frequency;
    let mut sections = Vec::new();
    let own_variables = variables(s);
    if !own_variables.is_empty() {
        sections.push((t!("Parameters").en(), own_variables));
    }
    let src = &m.system.source;
    let antenna = src.antenna.clone().unwrap_or_else(|| format!("{} Ω", m.system.receiver.impedance));
    let power = src.power.map_or_else(String::new, |p| format!(" · {p} dBm"));
    let source =
        node("system:source", own(&antenna), Some(own(&format!("{} K{power}", src.temperature))), Glyph::Radiation, Act::Edit("system.source"));
    sections.push((t!("Source").en(), vec![source]));
    let stages = m
        .system
        .stages
        .iter()
        .map(|st| {
            let kind = crate::system::kind_name(&st.part);
            let summary = crate::system::summary(st);
            let detail = Both { en: format!("{} · {summary}", kind.en()), shown: format!("{} · {summary}", lang::tr(kind)) };
            let glyph = match st.part.kind() {
                "amplifier" => Glyph::Sweep,
                "data" => Glyph::Touchstone,
                "line" => Glyph::Waveguide,
                _ => Glyph::Lumped,
            };
            node(format!("stage:{}", st.name), own(&st.name), Some(detail), glyph, Act::None).item(stage_item(&st.name))
        })
        .collect();
    sections.push((t!("Stages").en(), stages));
    let r = &m.system.receiver;
    let bw = r.bandwidth.map_or_else(String::new, |b| format!(" · {} {}", fu.from_si(b), fu.symbol()));
    let receiver = node(
        "system:receiver",
        own(&format!("{} Ω", r.impedance)),
        Some(own(&format!("SNR {} dB{bw}", r.snr))),
        Glyph::Table,
        Act::Edit("system.receiver"),
    );
    sections.push((t!("Receiver").en(), vec![receiver]));
    let measured = s
        .views()
        .measured
        .iter()
        .map(|x| {
            let detail = words(t!("{ports} ports · {n} frequencies"), args!(ports = x.data.outputs(), n = x.data.frequencies().len()));
            node(format!("measured:{}", x.name), own(&x.name), Some(detail), Glyph::Touchstone, Act::Show(View::SParameters))
        })
        .collect();
    sections.push((t!("Measurements").en(), measured));
    let results = match &s.results().system {
        Some(r) => {
            let detail = words(t!("{n} frequencies"), args!(n = r.points.len()));
            vec![node("result:system", words(t!("System budget"), &[]), Some(detail), Glyph::Table, Act::Show(View::System))]
        }
        None => Vec::new(),
    };
    sections.push((t!("Results").en(), results));
    sections
}

/// The project's parameters: a template's (what sets its shape, ADR 0010: each with what it is)
/// and the variables the job defined; a click edits one (`project.variable`), the model built
/// again with it.
fn variables(s: &emcraft_engine::Session) -> Vec<Row> {
    let template = s.sample().and_then(emcraft_engine::template_parameters).unwrap_or_default();
    s.variables()
        .iter()
        .map(|(name, value)| {
            let given = s.history().iter().rev().find(|st| st.defines(name)).and_then(|st| st.param("value").cloned());
            // (A template's parameter says what it is.)
            let expression = given.as_ref().and_then(serde_json::Value::as_str).map(|e| format!(" ({e})")).unwrap_or_default();
            let edit = serde_json::json!({"name": name, "value": given.unwrap_or_else(|| serde_json::json!(value))});
            let label = template.iter().find(|p| p.name == *name).and_then(|p| p.label.clone());
            let said = |shown: &str| match &label {
                Some(_) => format!("= {value}{expression} · {shown}"),
                None => format!("= {value}{expression}"),
            };
            let detail = Both { en: said(label.as_deref().unwrap_or("")), shown: said(label.as_deref().map_or("", crate::lang::parameter_label)) };
            node(format!("variable:{name}"), own(name), Some(detail), Glyph::Variable, Act::EditWith("project.variable", edit))
        })
        .chain(design_parameters(s))
        .collect()
}

/// The parameters of the SolidCraft designs imported that no variable sets (the ones set by an
/// expression `=a` are the variables'): each its value and unit and its design's file; a click
/// sets it where the design is imported (`geometry.design`).
fn design_parameters(s: &emcraft_engine::Session) -> Vec<Row> {
    emcraft_engine::designs(s)
        .into_iter()
        .flat_map(|d| {
            let parameters = d.parameters.clone().unwrap_or_default();
            parameters.into_iter().filter_map(move |q| design_parameter(&d, &q))
        })
        .collect()
}

/// Parameter `q` of design `d` as a row, none when a variable sets it (`=a`): its value (as
/// set where the design is imported, else as saved) and unit, and the design's file.
fn design_parameter(d: &emcraft_engine::ImportedDesign, q: &emcraft_engine::DesignParameter) -> Option<Row> {
    let set = d.set.get(&q.name);
    if set.and_then(serde_json::Value::as_str).is_some_and(|e| e.trim_start().starts_with('=')) {
        return None;
    }
    let value = match set {
        Some(serde_json::Value::String(e)) => e.clone(),
        Some(v) => v.to_string(),
        None => q.value.map_or_else(String::new, |x| format!("{x}")),
    };
    let said = format!("= {value} {} · {}", q.unit, file_name(&d.path));
    let edit = serde_json::json!({"design": d.path, "name": q.name, "value": value});
    let id = format!("design:{}:{}", d.path, q.name);
    Some(node(id, own(&q.name), Some(Both { en: said.clone(), shown: said }), Glyph::Variable, Act::EditWith("geometry.design", edit)))
}

/// A file's name without its folders (a template's design: after `template:`).
fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\', ':']).next().unwrap_or(path)
}

/// The SolidCraft designs of the user's the project imports, each a row that opens it in
/// SolidCraft (a template's design is set by the template's parameters).
fn design_rows(s: &emcraft_engine::Session) -> Vec<Row> {
    emcraft_engine::designs(s)
        .into_iter()
        .filter(|d| !d.template)
        .map(|d| {
            let detail = words(t!("SolidCraft design"), &[]);
            node(format!("design:{}", d.path), own(file_name(&d.path)), Some(detail), Glyph::Import, Act::Design(d.path.clone()))
        })
        .collect()
}

/// A page's name, as its row says it.
fn view_name(v: View) -> Both {
    Both { en: v.label().to_owned(), shown: lang::lookup(v.label()).to_owned() }
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
    let region = m.domain.solid.as_ref().map_or("vacuum", |s| s.material.as_str());
    let mut rows = waveguide_bodies(m, f);
    // A board solved by the finite elements (a board's PCB analysis has its own tree, above):
    // a row like the other bodies, the 3D view kept (its route view is the PCB analysis').
    if let Some(board) = m.pcb() {
        rows.push(node("board", words(t!("PCB"), &[]), Some(own(&board.name)), Glyph::Part, Act::None));
    }
    rows.extend(part_bodies(m));
    rows.extend(placed_bodies(m, region));
    rows
}

/// A waveguide's bodies: its box, then under it each section, layer, strip and via, in the
/// order placed.
fn waveguide_bodies(m: &emcraft_model::Model, f: &Fmt) -> Vec<Row> {
    let (mm, lu) = (|x| f.len(x), f.unit());
    let mut rows = Vec::new();
    // Its shape is the template's parameters' (ADR 0010): a row selects, it edits nothing.
    let edit = || Act::None;
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
    rows
}

/// An imported part's walls and region body: a double-click gives them another material.
fn part_bodies(m: &emcraft_model::Model) -> Vec<Row> {
    let mut rows = Vec::new();
    for (i, b) in emcraft_engine::bodies::parts(m).into_iter().enumerate() {
        let (detail, glyph) = if b.material == emcraft_engine::bodies::METAL {
            (words(t!("metal · imported"), &[]), Glyph::Part)
        } else {
            (words(t!("the region · imported"), &[]), Glyph::Region)
        };
        let item = Item::new("geometry.body.material", "", "body", serde_json::json!(b.name));
        rows.push(node(format!("body:{}", b.name), own(&b.name), Some(detail), glyph, Act::None).picks(Pick::Part(i)).item(item));
    }
    rows
}

/// The bodies placed, by what they are made of (hatched when the region's own); a
/// double-click changes it.
fn placed_bodies(m: &emcraft_model::Model, region: &str) -> Vec<Row> {
    let mut rows = Vec::new();
    for b in emcraft_engine::bodies::bodies(m) {
        let pick = match b.at {
            emcraft_engine::bodies::BodyRef::Feature(i) => Pick::Feature(i),
            emcraft_engine::bodies::BodyRef::Inclusion(i) => Pick::Inclusion(i),
        };
        // What it is made of is EMCraft's; its shape is the template's or the design's (ADR 0010).
        let item = Item::new("geometry.body.material", "", "body", serde_json::json!(b.name));
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
                // The shape's kind by its name in the window's language (a box: 箱).
                let kind = other.kind().to_lowercase();
                let named = crate::param_labels::value_label(&kind).map_or_else(|| kind.clone(), |l| lang::lookup(l).to_owned());
                let template = t!("{shape} · {material}");
                let detail = Both {
                    en: lang::fill(template.en(), args!(shape = kind, material = material)),
                    shown: lang::fill(lang::tr(template), args!(shape = named, material = material)),
                };
                (detail, Act::Edit("model.background"))
            }
        };
        rows.push(node("region", words(t!("Region"), &[]), Some(detail), Glyph::Region, act).picks(Pick::Solid));
    } else if m.waveguide().is_some() {
        rows.push(node("region", words(t!("Region"), &[]), Some(words(t!("inside the waveguide"), &[])), Glyph::Region, Act::None));
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
            let detail = match d.wall_roughness {
                Some(h) => {
                    let rough = words(t!("{roughness} {unit} rough"), args!(roughness = mm(h.get()), unit = lu));
                    Both { en: format!("{} · {}", detail.en, rough.en), shown: format!("{} · {}", detail.shown, rough.shown) }
                }
                None => detail,
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
            // Selected, it stays on the model (where the port is drawn), its form beside it; its
            // modes open from the Excitation step's table.
            Row { role: roles.get(i).cloned(), ..node(format!("port:{}", p.name), name, Some(detail), Glyph::Port, Act::None) }
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

use super::face_place;

/// The periodic boundaries in brief: the axes tied and, across x or y, the incident wave.
pub fn periodic_text(p: &emcraft_model::Periodic) -> String {
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

fn results(session: &Session, replies: &Replies) -> Vec<Row> {
    let r = session.results();
    let fu = session.display().frequency;
    let mut rows = Vec::new();
    let name = |t: crate::lang::Text| words(t, &[]);
    if let Some(sw) = &r.sweep {
        let detail = words(t!("{n} frequencies · {unknowns} unknowns"), args!(n = sw.results.len(), unknowns = sw.driven.dofs.unknowns));
        rows.push(node("result:sweep", name(t!("Sweep")), Some(detail), Glyph::Chart, Act::Show(View::SParameters)));
        rows.push(node("result:fields", name(t!("Fields")), None, Glyph::FieldLines, Act::Show(View::Field)).under());
    }
    // What else a sweep is read as, each its page: the time response and the analyses.
    if r.sweep.is_some() || r.time_domain.is_some() {
        rows.extend(
            [("result:tdr", View::Tdr, Glyph::Time), ("result:response", View::Pulse, Glyph::KindPulse)]
                .map(|(id, v, glyph)| node(id, view_name(v), None, glyph, Act::Show(v)).under()),
        );
    }
    if r.sweep.is_some() || replies.analysis.is_some() {
        rows.push(node("result:analyses", view_name(View::Analyses), None, Glyph::Table, Act::Show(View::Analyses)).under());
    }
    if let Some(e) = &r.eigen {
        let detail = words(t!("{n} modes"), args!(n = e.modes.len()));
        rows.push(node("result:eigenmodes", name(t!("Eigenmodes")), Some(detail), Glyph::Eigen, Act::Show(View::Eigenmodes)));
    }
    if let Some(rev) = &r.revolution {
        let detail = words(t!("m = {m} · {n} frequencies"), args!(m = rev.m, n = rev.data.frequencies().len()));
        rows.push(node("result:revolution", name(t!("Body of revolution")), Some(detail), Glyph::Chart, Act::Show(View::SParameters)));
    }
    if let Some(b) = &r.bloch {
        let detail = words(t!("{n} waves · {frequencies} frequencies"), args!(n = b.waves.len(), frequencies = b.frequencies.len()));
        rows.push(node("result:bloch", name(t!("Bloch waves")), Some(detail), Glyph::Periodic, Act::Show(View::Pulse)));
        rows.push(node("result:pulse", name(t!("Pulse")), None, Glyph::KindPulse, Act::Show(View::Pulse)).under());
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
    if let Some(study) = &r.study {
        let (title, detail) = match study.kind {
            emcraft_engine::study::Kind::Sweep if study.is_grid() => (t!("Parameter sweep"), t!("{names} · {n} points")),
            emcraft_engine::study::Kind::Sweep => (t!("Parameter sweep"), t!("{names} · {n} values")),
            emcraft_engine::study::Kind::Optimize => (t!("Optimization"), t!("{names} · {n} solves")),
            emcraft_engine::study::Kind::Tolerance => (t!("Tolerance analysis"), t!("{names} · {n} draws")),
        };
        let glyph = match study.kind {
            emcraft_engine::study::Kind::Sweep | emcraft_engine::study::Kind::Tolerance => Glyph::ParamSweep,
            emcraft_engine::study::Kind::Optimize => Glyph::Optimize,
        };
        let detail = words(detail, args!(names = study.variables.join(", "), n = study.rows.len()));
        rows.push(node("result:study", name(title), Some(detail), glyph, Act::Show(View::Study)));
    }
    rows
}
