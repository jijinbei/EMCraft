//! The S-parameter chart: |S11| and |S21| (and the rest of the first column with more ports or
//! terminals) in dB against frequency, the solved points over the
//! reference's curves, with the sections' cut-offs marked.

use emcraft_model::units::decibels;
use emcraft_render::chart::{Chart, Series, Style};
use emcraft_render::{Axis, Pen};
use emcraft_solver::Complex64 as C;

use super::chart::{DB_FLOOR, db_axis};
use super::reference::{reference, section_cutoffs};
use crate::{EngineError, Result, Session};

/// The most entries of S's first column the chart draws.
const MAX_CHART_ENTRIES: usize = 8;

/// The series colours of the first column's entries past S₂₁ (green, orange, purple).
const MORE_PENS: [u8; 3] = [2, 4, 3];

/// |S| in dB, no lower than the chart reaches.
fn db(c: C) -> f64 {
    decibels(c.norm()).max(DB_FLOOR)
}

/// |S11| and |S21| in dB against frequency: the solved points and the reference curves.
pub fn sparameter_chart(session: &Session) -> Result<Chart> {
    let sweep = session.results.sweep.as_ref();
    let td = session.results.time_domain.as_ref();
    let fu = session.display.frequency;
    let (f0, f1) = match (sweep, td) {
        (Some(sw), _) if !sw.results.is_empty() => (sw.results[0].frequency, sw.results[sw.results.len() - 1].frequency),
        (_, Some(t)) if !t.frequencies.is_empty() => (t.frequencies[0], t.frequencies[t.frequencies.len() - 1]),
        _ => return Err(EngineError::NotSolved { what: "results", run: "solve.frequency or solve.time_domain" }),
    };
    let pad = if f1 > f0 { 0.0 } else { 0.05 * f0 };
    let (f0, f1) = (f0 - pad, f1 + pad);
    let mut series = reference_curves(session, f0, f1);
    if let Some(sweep) = sweep {
        let pts = |p: usize, q: usize| sweep.results.iter().filter_map(|r| r.s.get(p, q).map(|s| [fu.from_si(r.frequency), db(s)])).collect();
        series.push(Series {
            label: format!("$|S_{{21}}|$ FEM (order {})", sweep.order.as_u8()),
            pen: Pen::Strong(0),
            style: Style::Markers,
            points: pts(1, 0),
        });
        series.push(Series { label: "$|S_{11}|$ FEM".into(), pen: Pen::Strong(1), style: Style::Markers, points: pts(0, 0) });
        // More entries (a terminal port's conductors, more ports): the rest of the first column.
        let n = sweep.results.first().map_or(0, |r| r.s.ports()).min(MAX_CHART_ENTRIES);
        for (p, k) in (2..n).zip(MORE_PENS.iter().cycle()) {
            series.push(Series { label: format!("$|S_{{{}1}}|$ FEM", p + 1), pen: Pen::Strong(*k), style: Style::Markers, points: pts(p, 0) });
        }
    }
    if let Some(td) = td {
        let pts = |v: &[C]| td.frequencies.iter().zip(v).map(|(f, s)| [fu.from_si(*f), db(*s)]).collect();
        series.push(Series { label: "$|S_{21}|$ time domain".into(), pen: Pen::Series(2), style: Style::Line, points: pts(&td.s21) });
        series.push(Series { label: "$|S_{11}|$ time domain".into(), pen: Pen::Series(4), style: Style::Line, points: pts(&td.s11) });
    }
    let marks = section_cutoffs(session).into_iter().map(|(fc, name)| (fu.from_si(fc), format!("$f_c$ {name}"))).collect();
    Ok(Chart {
        title: "S-parameters".into(),
        x: Axis::new(&format!("frequency ({})", fu.symbol()), fu.from_si(f0), fu.from_si(f1)),
        y: db_axis("$|S|$ (dB)", &series),
        series,
        marks,
    })
}

/// The reference's |S21| and |S11| from `f0` to `f1` (Hz), finely; none without a reference.
fn reference_curves(session: &Session, f0: f64, f1: f64) -> Vec<Series> {
    let Ok(reference) = reference(session) else { return Vec::new() };
    let fu = session.display.frequency;
    // The curve: fewer modes than the comparison takes (mode matching is dense linear algebra
    // per frequency); 40 keep the curve within 10⁻² dB.
    let (n, modes) = if reference.is_mode_matching() { (160, 40) } else { (400, 1) };
    let (mut s11, mut s21) = (Vec::new(), Vec::new());
    for f in crate::numbers::evenly_spaced(f0, f1, n + 1) {
        if let Ok((r, t)) = reference.s(f, modes) {
            s11.push([fu.from_si(f), db(r)]);
            s21.push([fu.from_si(f), db(t)]);
        }
    }
    let label = reference.label();
    vec![
        Series { label: format!("$|S_{{21}}|$ {label}"), pen: Pen::Series(0), style: Style::Line, points: s21 },
        Series { label: format!("$|S_{{11}}|$ {label}"), pen: Pen::Series(1), style: Style::Line, points: s11 },
    ]
}
