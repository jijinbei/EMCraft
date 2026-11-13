//! The S-parameters page as every window shows it: what its chart shows ([`SView`]: in dB, as a
//! magnitude, a phase, a group delay or on a Smith chart; mixed mode; the kept solves drawn
//! over it; the band picked to solve more finely), the table of S's first column, the last
//! coupling matrix, and the kept solves' lines.

use emcraft_engine::Session;
use emcraft_engine::view::SDisplay;
use serde_json::{Value, json};

use super::Table;
use crate::lang::{args, tr, trf};

/// Most points a band picked on the chart adds, and the points it adds at first.
pub const MAX_REFINE: usize = 201;
pub const REFINE_POINTS: usize = 21;
/// Most entries of S's first column the table shows.
const TABLE_ENTRIES: usize = 8;

/// What the S-parameter chart shows: how S is drawn, mixed mode, the kept solves drawn over it,
/// the table unfolded, and the band picked (Hz) with the points to add in it.
#[derive(Clone, Debug, PartialEq)]
pub struct SView {
    pub display: SDisplay,
    pub mixed: bool,
    pub compare: Vec<u32>,
    pub table: bool,
    pub band: Option<(f64, f64)>,
    pub points: usize,
}

impl Default for SView {
    fn default() -> Self {
        SView { display: SDisplay::Db, mixed: false, compare: Vec::new(), table: false, band: None, points: REFINE_POINTS }
    }
}

impl SView {
    /// The view with kept solve `id` drawn over the chart, or taken off it when it is.
    pub fn toggled(self, id: u32) -> SView {
        let mut compare = self.compare;
        match compare.iter().position(|c| *c == id) {
            Some(k) => {
                compare.remove(k);
            }
            None => compare.push(id),
        }
        SView { compare, ..self }
    }

    /// The view as `session` holds it: the kept solves let go drawn no more.
    pub fn kept(self, session: &Session) -> SView {
        let kept: Vec<u32> = session.archive().iter().map(|a| a.id).collect();
        SView { compare: self.compare.into_iter().filter(|id| kept.contains(id)).collect(), ..self }
    }

    /// `solve.refine`'s parameters for the band picked, in the session's display units.
    pub fn refine(&self, session: &Session) -> Option<Value> {
        let (a, b) = self.band?;
        let fu = session.display().frequency;
        Some(json!({"start": fu.from_si(a), "stop": fu.from_si(b), "points": self.points.clamp(2, MAX_REFINE)}))
    }
}

/// Whether the sweep's or a measurement's ports pair in order for mixed mode: four or more, an
/// even number.
pub fn pairs(session: &Session) -> bool {
    let even = |n: usize| n >= 4 && n.is_multiple_of(2);
    session.results().sweep.as_ref().is_some_and(|sw| even(sw.results.first().map_or(0, |x| x.s.ports())))
        || session.views().measured.iter().any(|m| even(m.data.outputs()))
}

/// The chart `view` asks of `session`.
pub fn chart(session: &Session, view: &SView) -> emcraft_engine::Result<emcraft_render::chart::Chart> {
    emcraft_engine::view::sparameter_chart_with(session, view.display, &view.compare, view.mixed && pairs(session))
}

/// The decimals that tell frequencies (display units) apart: enough for their smallest step,
/// from 1 to 6.
///
/// ```
/// use emcraft_ui_core::results::sparameters::frequency_decimals;
/// assert_eq!((frequency_decimals(&[6.0, 7.0]), frequency_decimals(&[6.0, 6.025]), frequency_decimals(&[9.0])), (1, 2, 3));
/// ```
pub fn frequency_decimals(freqs: &[f64]) -> usize {
    let step = freqs.windows(2).map(|w| (w[1] - w[0]).abs()).filter(|d| *d > 0.0).fold(f64::INFINITY, f64::min);
    if !step.is_finite() {
        return 3;
    }
    (-step.log10()).ceil().clamp(1.0, 6.0) as usize
}

/// The value of `freqs` nearest `f`.
///
/// ```
/// assert_eq!(emcraft_ui_core::results::sparameters::nearest(&[5.0, 6.0, 7.0], 6.4), Some(6.0));
/// ```
pub fn nearest(freqs: &[f64], f: f64) -> Option<f64> {
    freqs.iter().copied().filter(|x| x.is_finite()).min_by(|a, b| (a - f).abs().total_cmp(&(b - f).abs()))
}

/// The sweep's first column of S (S11, S21, and on with more ports or terminals) at each
/// frequency: |S| in dB and its phase in degrees, a terminal port's entries named in the caption,
/// the rows solved in full marked when a fast sweep solved some in its basis.
pub fn table(session: &Session) -> Option<Table> {
    let sw = session.results().sweep.as_ref()?;
    let fu = session.display().frequency;
    let entries = sw.entries();
    let n = entries.len().clamp(2, TABLE_ENTRIES);
    let caption = entries.iter().any(|e| e.terminal.is_some()).then(|| {
        let names: Vec<String> = entries.iter().enumerate().map(|(i, e)| format!("{} = {}", i + 1, e.name)).collect();
        trf(t!("Entries of S: {entries}"), args!(entries = names.join(", ")))
    });
    let freqs: Vec<f64> = sw.results.iter().map(|r| fu.from_si(r.frequency)).collect();
    let fd = frequency_decimals(&freqs);
    let headers =
        std::iter::once(format!("f ({})", fu.symbol())).chain((1..=n).flat_map(|p| [format!("|S{p}1| (dB)"), format!("arg S{p}1 (°)")])).collect();
    let rows = sw
        .results
        .iter()
        .zip(&freqs)
        .map(|(r, f)| {
            let cells = (0..n).flat_map(|p| match r.s.get(p, 0) {
                Some(c) => [format!("{:.2}", emcraft_model::units::decibels(c.norm())), format!("{:.1}", c.arg().to_degrees())],
                None => ["–".to_owned(), "–".to_owned()],
            });
            std::iter::once(format!("{f:.fd$}")).chain(cells).collect()
        })
        .collect();
    let marks = sw.results.iter().any(|r| !r.full).then(|| sw.results.iter().map(|r| r.full).collect());
    Some(Table { caption, headers, rows, marks })
}

/// The last coupling matrix (`filter.synthesize`, `results.coupling`) as a table — rows and
/// columns S, 1, …, N, L, its zeros left blank — with its reflection and transmission zeros in
/// the caption.
pub fn coupling(reply: &Value) -> Table {
    let names: Vec<String> =
        reply.get("names").and_then(Value::as_array).map_or_default(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect());
    let at = |i: usize, j: usize| reply["matrix"][i][j].as_f64().unwrap_or(0.0);
    let rows = names
        .iter()
        .enumerate()
        .map(|(i, name)| {
            std::iter::once(name.clone())
                .chain((0..names.len()).map(|j| if at(i, j).abs() < 1e-9 { String::new() } else { format!("{:.4}", at(i, j)) }))
                .collect()
        })
        .collect();
    let list = |key: &str| {
        let v: Vec<String> = reply[key].as_array().map_or_default(|a| a.iter().filter_map(|x| x.as_f64().map(|x| format!("{x:.3}"))).collect());
        if v.is_empty() { "—".to_owned() } else { v.join(", ") }
    };
    let caption = trf(t!("Reflection zeros ω: {r} · transmission zeros ω: {z}"), args!(r = list("reflection_zeros"), z = list("transmission_zeros")));
    Table { caption: Some(caption), headers: std::iter::once(String::new()).chain(names.iter().cloned()).collect(), rows, marks: None }
}

/// Whether the page lists the kept solves: two to compare, or one while nothing is solved now.
pub fn lists_history(session: &Session) -> bool {
    let kept = session.archive().len();
    kept >= 2 || (kept == 1 && session.results().sweep.is_none())
}

/// A kept solve's line: `#id` (★ pinned), what changed since the one before, and its deepest
/// |S11|.
pub fn history_line(r: &emcraft_engine::archive::HistoryRow) -> (String, String, Option<String>) {
    let star = if r.pinned { " ★" } else { "" };
    let what = if r.changes.is_empty() { tr(t!("as it was")).to_owned() } else { r.changes.join(", ") };
    (format!("#{}{star}", r.id), what, r.best_s11_db.map(|db| format!("{db:.1} dB")))
}

/// A kept solve's tip: its label, its frequencies, how it was made (how long, on what, how
/// large), its deepest |S11| and where, and for one read back from the project file (`stored`)
/// that restoring it solves it again.
pub fn history_tip(session: &Session, r: &emcraft_engine::archive::HistoryRow, stored: bool) -> String {
    let made = &r.provenance;
    let label = r.label.clone();
    let points = Some(trf(t!("{n} frequencies"), args!(n = r.points.unwrap_or(0))));
    let solved = Some(trf(t!("solved in {seconds} s on {device}"), args!(seconds = format!("{:.1}", made.seconds), device = made.device.id())));
    let size = made.unknowns.zip(made.tetrahedra).map(|(u, t)| trf(t!("{unknowns} unknowns, {tets} tetrahedra"), args!(unknowns = u, tets = t)));
    let saved = stored.then(|| tr(t!("Saved: restore solves it again")).to_owned());
    let unit = session.display().frequency.symbol();
    let deepest = r.best_s11_db.zip(r.best_at).map(|(db, at)| {
        trf(t!("deepest |S11| {db} dB at {f} {unit}"), args!(db = crate::number::decibels(db), f = crate::number::frequency(at), unit = unit))
    });
    [label, points, solved, size, saved, deepest].into_iter().flatten().collect::<Vec<_>>().join("\n")
}

/// A kept solve's actions: restore, pin or unpin, delete — each its label, its command and its
/// parameters.
pub fn history_actions(id: u32, pinned: bool) -> [(crate::lang::Text, &'static str, Value); 3] {
    [
        (t!("Restore"), "results.history.restore", json!({"id": id})),
        (if pinned { t!("Unpin") } else { t!("Pin") }, "results.history.pin", json!({"id": id, "pinned": !pinned})),
        (t!("Delete"), "results.history.delete", json!({"id": id})),
    ]
}

/// Whether the ports' reference planes move S at once (a phase shift of each port's mode) or
/// call for a new solve (terminal ports, a periodic cell).
pub fn planes_instant(session: &Session) -> bool {
    session.results().sweep.as_ref().is_some_and(|sw| !sw.driven.has_terminals() && sw.driven.periodic.is_none())
}
