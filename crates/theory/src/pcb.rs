//! PCB route screening plus an explicitly limited, lossless quasi-TEM line model.
//! Impedance: Hammerstad–Jensen (1980), existing `microstrip` implementation. Cascading:
//! Pozar, Microwave Engineering, 4th ed., §4.4 (`coupled::line`/`s_params`). Geometry findings
//! are not a field solution or an EMC/SI compliance verdict. No guessed ground across gaps.
use crate::{coupled, microstrip};
use emcraft_model::pcb::{Board, Segment, distance};
use emcraft_model::units::C0;
use num_complex::Complex64 as C;
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Debug)]
pub struct Finding {
    pub code: &'static str,
    pub detail: String,
    pub segment: Option<String>,
}
#[derive(Clone, Debug)]
pub struct Section {
    pub id: String,
    pub reference: String,
    pub length: f64,
    pub h: f64,
    pub impedance: f64,
    pub delay: f64,
    pub uncovered: Vec<[f64; 2]>,
}
#[derive(Clone, Debug)]
pub struct Report {
    pub findings: Vec<Finding>,
    pub sections: Vec<Section>,
    pub path: Vec<usize>,
    pub model_available: bool,
    pub length: f64,
}
fn cross(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}
fn sub(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] - b[0], a[1] - b[1]]
}
/// Point-to-segment distance (m) and projection, for drawing/picking and geometry screening.
pub fn near(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> (f64, f64) {
    let d = sub(b, a);
    let q = sub(p, a);
    let t = ((q[0] * d[0] + q[1] * d[1]) / (d[0] * d[0] + d[1] * d[1]).max(1e-30)).clamp(0.0, 1.0);
    (distance(p, [a[0] + t * d[0], a[1] + t * d[1]]), t)
}
fn inside(p: [f64; 2], poly: &[[f64; 2]]) -> bool {
    let mut on = false;
    for (a, b) in poly.iter().zip(poly.iter().cycle().skip(1)).take(poly.len()) {
        if near(p, *a, *b).0 < 1e-12 {
            return true;
        }
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0] {
            on = !on;
        }
    }
    on
}
// Split at every polygon intersection; midpoint membership is exact between intersections,
// unlike fixed sampling which can miss narrow slots. Collinear boundary endpoints count too.
fn gaps(a: [f64; 2], b: [f64; 2], polys: &[&[[f64; 2]]], budget: &mut usize) -> Result<Vec<[f64; 2]>, String> {
    let mut cuts = vec![0.0, 1.0];
    let d = sub(b, a);
    for poly in polys {
        for (c, e) in poly.iter().zip(poly.iter().cycle().skip(1)).take(poly.len()) {
            if *budget == 0 {
                return Err("return geometry work limit exceeded; select a smaller net".into());
            }
            *budget -= 1;
            let edge = sub(*e, *c);
            let den = cross(d, edge);
            if den.abs() > 1e-24 {
                let t = cross(sub(*c, a), edge) / den;
                let u = cross(sub(*c, a), d) / den;
                if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u) {
                    cuts.push(t);
                }
            } else if cross(sub(*c, a), d).abs() < 1e-20 {
                for p in [*c, *e] {
                    let (r, t) = near(p, a, b);
                    if r < 1e-12 {
                        cuts.push(t);
                    }
                }
            }
        }
    }
    cuts.sort_by(f64::total_cmp);
    cuts.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
    let mut out = Vec::new();
    for v in cuts.windows(2) {
        let t = (v[0] + v[1]) / 2.0;
        let p = [a[0] + t * d[0], a[1] + t * d[1]];
        let mut covered = false;
        for poly in polys {
            if *budget < poly.len() {
                return Err("return geometry work limit exceeded".into());
            }
            *budget -= poly.len();
            if inside(p, poly) {
                covered = true;
                break;
            }
        }
        if !covered {
            out.push([v[0], v[1]]);
        }
    }
    Ok(out)
}
fn key(layer: &str, p: [f64; 2]) -> (String, i64, i64) {
    (layer.into(), (p[0] / 1e-9).round() as i64, (p[1] / 1e-9).round() as i64)
}
fn path(segments: &[&Segment]) -> Option<Vec<usize>> {
    let mut graph: BTreeMap<_, Vec<usize>> = BTreeMap::new();
    for (i, s) in segments.iter().enumerate() {
        for p in [s.start, s.end] {
            graph.entry(key(&s.layer, p)).or_default().push(i);
        }
    }
    let ends: Vec<_> = graph.iter().filter(|(_, v)| v.len() == 1).map(|(k, _)| k.clone()).collect();
    if ends.len() != 2 || graph.values().any(|v| v.len() > 2) {
        return None;
    }
    // Non-adjacent same-layer copper crossings/overlaps create electrical shortcuts even
    // when their centreline endpoints alone form a simple chain.
    for (i, a) in segments.iter().enumerate() {
        for b in segments.iter().skip(i + 1) {
            if a.layer != b.layer || [a.start, a.end].iter().any(|p| [b.start, b.end].iter().any(|q| key(&a.layer, *p) == key(&b.layer, *q))) {
                continue;
            }
            let d = sub(a.end, a.start);
            let e = sub(b.end, b.start);
            let den = cross(d, e);
            let intersects = den.abs() > 1e-24
                && (0.0..=1.0).contains(&(cross(sub(b.start, a.start), e) / den))
                && (0.0..=1.0).contains(&(cross(sub(b.start, a.start), d) / den));
            let gap =
                [near(a.start, b.start, b.end).0, near(a.end, b.start, b.end).0, near(b.start, a.start, a.end).0, near(b.end, a.start, a.end).0]
                    .into_iter()
                    .fold(f64::INFINITY, f64::min);
            if intersects || gap < (a.width + b.width) / 2.0 {
                return None;
            }
        }
    }
    let mut at = ends[0].clone();
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for _ in 0..segments.len() {
        let i = *graph.get(&at)?.iter().find(|i| !seen.contains(*i))?;
        seen.insert(i);
        out.push(i);
        let s = segments[i];
        at = if key(&s.layer, s.start) == at { key(&s.layer, s.end) } else { key(&s.layer, s.start) };
    }
    (seen.len() == segments.len() && at == ends[1]).then_some(out)
}
/// Inspect a complete routed net. Coordinates/lengths are metres. Numerical S-parameters are
/// enabled only for an unbranched, single-layer outer microstrip with continuous ground.
/// ```
/// let b=emcraft_model::pcb::Board{name:"empty".into(),layers:vec![],segments:vec![],vias:vec![],copper:vec![],notes:vec![]};
/// assert!(emcraft_theory::pcb::inspect(&b,"CLK","GND").is_err());
/// ```
pub fn inspect(board: &Board, net: &str, ground: &str) -> Result<Report, String> {
    board.check().map_err(|e| e.to_string())?;
    if net == ground {
        return Err("select a signal net different from the reference net".into());
    }
    let traces: Vec<_> = board.segments.iter().filter(|s| s.net == net).collect();
    if traces.is_empty() || traces.len() > 512 {
        return Err("net must contain 1..512 routed segments".into());
    }
    let mut report = Report {
        findings: vec![],
        sections: vec![],
        path: vec![],
        model_available: true,
        length: traces.iter().map(|s| distance(s.start, s.end)).sum(),
    };
    let mut finding = |code, detail: String, segment: Option<String>| report.findings.push(Finding { code, detail, segment });
    match path(&traces) {
        Some(p) => report.path = p,
        None => {
            report.model_available = false;
            finding("route_topology", "Branched, disconnected or closed route: a two-terminal line model cannot represent it.".into(), None);
        }
    }
    let signal_vias: Vec<_> = board.vias.iter().filter(|v| v.net == net).collect();
    if !signal_vias.is_empty() {
        report.model_available = false;
        finding("via_transition", "Signal vias require a via/transition model; no two-port prediction is made.".into(), None);
    }
    let mut budget = 1_000_000;
    for s in &traces {
        let Some(index) = board.layers.iter().position(|l| l.name == s.layer) else {
            return Err("unknown layer".into());
        };
        let reference = board
            .layers
            .iter()
            .enumerate()
            .filter(|(i, l)| *i != index && board.copper.iter().any(|c| c.net == ground && c.layer == l.name))
            .min_by(|(_, a), (_, b)| (a.z - board.layers[index].z).abs().total_cmp(&(b.z - board.layers[index].z).abs()));
        let Some((ri, plane)) = reference else {
            report.model_available = false;
            finding("no_reference", format!("No filled {ground} reference copper on another layer."), Some(s.id.clone()));
            continue;
        };
        let h = (plane.z - board.layers[index].z).abs();
        let polys: Vec<&[[f64; 2]]> =
            board.copper.iter().filter(|c| c.net == ground && c.layer == plane.name).map(|c| c.polygon.as_slice()).collect();
        let uncovered = gaps(s.start, s.end, &polys, &mut budget)?;
        if !uncovered.is_empty() {
            report.model_available = false;
            finding(
                "return_gap",
                format!("The route crosses missing {ground} copper on {}. Restore a continuous reference or reroute.", plane.name),
                Some(s.id.clone()),
            );
        }
        let d = sub(s.end, s.start);
        let len = distance(s.start, s.end);
        let offset = [-d[1] / len * (h + s.width / 2.0), d[0] / len * (h + s.width / 2.0)];
        for sign in [-1.0, 1.0] {
            let a = [s.start[0] + sign * offset[0], s.start[1] + sign * offset[1]];
            let b = [s.end[0] + sign * offset[0], s.end[1] + sign * offset[1]];
            if !gaps(a, b, &polys, &mut budget)?.is_empty() {
                report.model_available = false;
                finding(
                    "reference_edge",
                    "Reference copper is missing within h + W/2 of this route; infinite-plane impedance is not applicable.".into(),
                    Some(s.id.clone()),
                );
                break;
            }
        }
        let dielectric = index.min(ri);
        let er = board.layers[dielectric].eps_below;
        let outer = index == 0 || index + 1 == board.layers.len();
        if !outer || index.abs_diff(ri) != 1 || !(0.01..=100.0).contains(&(s.width / h)) {
            report.model_available = false;
            finding(
                "unsupported_cross_section",
                "Only adjacent-plane outer microstrip within 0.01 ≤ W/h ≤ 100 is supported by this model.".into(),
                Some(s.id.clone()),
            );
        }
        let ee = microstrip::eps_eff(s.width / h, er);
        let impedance = microstrip::z0(s.width / h, er);
        if !impedance.is_finite() || !ee.is_finite() {
            return Err("non-finite line estimate".into());
        }
        report.sections.push(Section {
            id: s.id.clone(),
            reference: plane.name.clone(),
            length: len,
            h,
            impedance,
            delay: len * ee.sqrt() / C0,
            uncovered,
        });
        for other in board.segments.iter().filter(|o| o.net != net && o.net != ground && o.layer == s.layer) {
            if budget == 0 {
                return Err("coupling screening work limit exceeded".into());
            }
            budget -= 1;
            let gap = [
                near(other.start, s.start, s.end).0,
                near(other.end, s.start, s.end).0,
                near(s.start, other.start, other.end).0,
                near(s.end, other.start, other.end).0,
            ]
            .into_iter()
            .fold(f64::INFINITY, f64::min)
                - (s.width + other.width) / 2.0;
            let od = sub(other.end, other.start);
            let parallel = cross(d, od).abs() / (len * distance(other.start, other.end));
            if gap < 3.0 * h && parallel < 0.1 {
                finding(
                    "nearby_signal",
                    format!(
                        "{} runs close and parallel (gap {:0.3e} m). This is geometric screening, not calculated crosstalk.",
                        other.net,
                        gap.max(0.0)
                    ),
                    Some(s.id.clone()),
                );
                break;
            }
        }
    }
    if report.sections.len() != traces.len() {
        report.model_available = false;
    }
    for via in signal_vias {
        let refs: BTreeSet<_> = report.sections.iter().map(|s| s.reference.as_str()).collect();
        if refs.len() < 2 {
            continue;
        }
        let stitched = board.vias.iter().any(|v| {
            v.net == ground
                && distance(v.at, via.at) <= 0.002
                && refs.iter().all(|r| {
                    let i = board.layers.iter().position(|l| l.name == *r);
                    let a = board.layers.iter().position(|l| l.name == v.layers[0]);
                    let b = board.layers.iter().position(|l| l.name == v.layers[1]);
                    match (i, a, b) {
                        (Some(i), Some(a), Some(b)) => i >= a.min(b) && i <= a.max(b),
                        _ => false,
                    }
                })
        });
        if !stitched {
            finding(
                "missing_return_via",
                "No reference-net via within 2 mm spanning the selected reference layers. Verify the return transition.".into(),
                None,
            );
        }
    }
    Ok(report)
}
/// Lossless line cascade for a valid report. Returns S11 and S21, both referred to `reference`
/// ohms. Bends, via inductance, pads, losses and coupling are deliberately not synthesized.
pub fn s_parameters(report: &Report, frequency: f64, reference: f64) -> Result<(C, C), String> {
    if !report.model_available
        || !frequency.is_finite()
        || frequency < 0.0
        || frequency > 1e12
        || !reference.is_finite()
        || !(0.1..=10000.0).contains(&reference)
    {
        return Err("transmission-line model unavailable or invalid frequency/impedance".into());
    }
    let mut matrix = coupled::line(reference, 0.0);
    for i in &report.path {
        let s = report.sections.get(*i).ok_or("incomplete route")?;
        let q = coupled::line(s.impedance, std::f64::consts::TAU * frequency * s.delay);
        matrix = coupled::product(&matrix, &q);
    }
    let out = coupled::s_params(&matrix, reference);
    if ![out.0.re, out.0.im, out.1.re, out.1.im].iter().all(|v| v.is_finite()) {
        return Err("non-finite cascade".into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use emcraft_model::pcb::{Copper, Layer};
    fn board() -> Board {
        Board {
            name: "test".into(),
            layers: vec![Layer { name: "top".into(), z: 0.0, eps_below: 4.2 }, Layer { name: "ground".into(), z: 0.00018, eps_below: 1.0 }],
            segments: vec![Segment {
                id: "route".into(),
                net: "DATA".into(),
                layer: "top".into(),
                start: [0.005, 0.01],
                end: [0.045, 0.01],
                width: 0.0003,
            }],
            vias: vec![],
            copper: vec![Copper { net: "GND".into(), layer: "ground".into(), polygon: vec![[0.0, 0.0], [0.05, 0.0], [0.05, 0.02], [0.0, 0.02]] }],
            notes: vec![],
        }
    }
    #[test]
    fn narrow_ground_slot_is_found_and_disables_si() {
        let mut b = board();
        let valid = inspect(&b, "DATA", "GND").unwrap();
        assert!(valid.model_available);
        let mut left = b.copper[0].clone();
        left.polygon[1][0] = 0.020001;
        left.polygon[2][0] = 0.020001;
        let mut right = b.copper[0].clone();
        right.polygon[0][0] = 0.020002;
        right.polygon[3][0] = 0.020002;
        b.copper = vec![left, right];
        let r = inspect(&b, "DATA", "GND").unwrap();
        assert!(!r.model_available);
        assert!(r.findings.iter().any(|f| f.code == "return_gap"));
        assert!(s_parameters(&r, 1e9, 50.0).is_err());
    }
    #[test]
    fn a_reference_edge_invalidates_the_infinite_plane_model_even_under_covered_centreline() {
        let mut b = board();
        b.copper[0].polygon[2][1] = 0.0102;
        b.copper[0].polygon[3][1] = 0.0102;
        let r = inspect(&b, "DATA", "GND").unwrap();
        assert!(!r.model_available);
        assert!(r.findings.iter().any(|f| f.code == "reference_edge"));
        assert!(!r.findings.iter().any(|f| f.code == "return_gap"));
    }
    #[test]
    fn a_return_via_screen_checks_distance_and_layer_span_without_claiming_a_via_model() {
        let mut b = board();
        b.layers.extend([Layer { name: "g2".into(), z: 0.0014, eps_below: 4.2 }, Layer { name: "bottom".into(), z: 0.00158, eps_below: 1.0 }]);
        let mut plane = b.copper[0].clone();
        plane.layer = "g2".into();
        b.copper.push(plane);
        b.segments[0].end = [0.025, 0.01];
        let mut end = b.segments[0].clone();
        end.id = "last".into();
        end.layer = "bottom".into();
        end.start = [0.025, 0.01];
        end.end = [0.045, 0.01];
        b.segments.push(end);
        b.vias.push(emcraft_model::pcb::Via { net: "DATA".into(), at: [0.025, 0.01], layers: ["top".into(), "bottom".into()], diameter: 0.0006 });
        let r = inspect(&b, "DATA", "GND").unwrap();
        assert!(!r.model_available);
        assert!(r.findings.iter().any(|f| f.code == "missing_return_via"));
        let mut via = b.vias[0].clone();
        via.net = "GND".into();
        via.at[0] += 0.0005;
        b.vias.push(via);
        let r = inspect(&b, "DATA", "GND").unwrap();
        assert!(!r.model_available);
        assert!(r.findings.iter().any(|f| f.code == "via_transition"));
        assert!(!r.findings.iter().any(|f| f.code == "missing_return_via"));
        b.vias[1].layers[1] = "ground".into();
        assert!(inspect(&b, "DATA", "GND").unwrap().findings.iter().any(|f| f.code == "missing_return_via"));
        b.vias[1].layers[1] = "bottom".into();
        b.vias[1].at[0] += 0.01;
        assert!(inspect(&b, "DATA", "GND").unwrap().findings.iter().any(|f| f.code == "missing_return_via"));
    }
    #[test]
    fn matched_quarter_wave_has_independent_closed_form_and_no_reflection() {
        let r = Report {
            findings: vec![],
            sections: vec![Section {
                id: "line".into(),
                reference: "gnd".into(),
                length: 0.1,
                h: 0.001,
                impedance: 50.0,
                delay: 1e-9,
                uncovered: vec![],
            }],
            path: vec![0],
            model_available: true,
            length: 0.1,
        };
        let (s11, s21) = s_parameters(&r, 0.25e9, 50.0).unwrap();
        assert!(s11.norm() < 1e-14);
        assert!((s21 - C::new(0.0, -1.0)).norm() < 1e-14);
        assert!((s11.norm_sqr() + s21.norm_sqr() - 1.0).abs() < 1e-14);
        assert!(s_parameters(&r, f64::NAN, 50.0).is_err());
    }
    #[test]
    fn topology_layer_and_reference_failures_do_not_get_plausible_si() {
        let mut b = board();
        let mut branch = b.segments[0].clone();
        branch.id = "branch".into();
        branch.end = [0.005, 0.015];
        b.segments.push(branch.clone());
        branch.id = "third".into();
        branch.end = [0.01, 0.015];
        b.segments.push(branch);
        assert!(!inspect(&b, "DATA", "GND").unwrap().model_available);
        b.segments.pop();
        b.segments.pop();
        b.copper.clear();
        let r = inspect(&b, "DATA", "GND").unwrap();
        assert!(!r.model_available);
        assert!(r.findings.iter().any(|f| f.code == "no_reference"));
        assert!(inspect(&b, "missing", "GND").is_err());
        assert!(inspect(&b, "DATA", "DATA").is_err());
        b.segments[0].width = f64::NAN;
        assert!(inspect(&b, "DATA", "GND").is_err());
    }
    #[test]
    fn a_route_crossing_itself_is_not_a_serial_line() {
        let mut b = board();
        let points = [[0.005, 0.005], [0.04, 0.015], [0.04, 0.005], [0.005, 0.015]];
        b.segments = points
            .windows(2)
            .enumerate()
            .map(|(i, p)| Segment { id: format!("track-{i}"), net: "DATA".into(), layer: "top".into(), start: p[0], end: p[1], width: 0.0003 })
            .collect();
        let r = inspect(&b, "DATA", "GND").unwrap();
        assert!(r.findings.iter().any(|f| f.code == "route_topology"));
        assert!(s_parameters(&r, 1e9, 50.0).is_err());
    }
    #[test]
    fn serial_width_steps_do_not_depend_on_input_order() {
        let mut b = board();
        b.segments[0].end = [0.025, 0.01];
        let mut last = b.segments[0].clone();
        last.id = "last".into();
        last.start = [0.025, 0.01];
        last.end = [0.045, 0.01];
        last.width = 0.0006;
        b.segments.push(last);
        let a = inspect(&b, "DATA", "GND").unwrap();
        let waves = s_parameters(&a, 1e9, 50.0).unwrap();
        b.segments.reverse();
        assert_eq!(s_parameters(&inspect(&b, "DATA", "GND").unwrap(), 1e9, 50.0).unwrap(), waves);
    }
}

/// A geometrically parallel overlapping pair. Points are SI metres; this is not a coupling coefficient.
#[derive(Clone, Debug)]
pub struct Pair {
    pub source: String,
    pub target: String,
    pub source_segment: String,
    pub target_segment: String,
    pub layer: String,
    pub a: [[f64; 2]; 2],
    pub b: [[f64; 2]; 2],
    pub gap: f64,
    pub length: f64,
}
/// Find same-layer parallel overlaps, including wide spacing for before/after illustration.
/// ```
/// let b=emcraft_model::pcb::Board{name:"empty".into(),layers:vec![],segments:vec![],vias:vec![],copper:vec![],notes:vec![]};
/// assert!(emcraft_theory::pcb::pairs(&b,"DATA","GND").is_err());
/// ```
pub fn pairs(board: &Board, source: &str, ground: &str) -> Result<Vec<Pair>, String> {
    board.check().map_err(|e| e.to_string())?;
    let selected: Vec<_> = board.segments.iter().filter(|s| s.net == source).collect();
    if selected.is_empty() || selected.len() > 512 {
        return Err("net must contain 1..512 routed segments".into());
    }
    let mut out = Vec::new();
    let mut work = 0usize;
    for s in selected {
        let d = sub(s.end, s.start);
        let len = distance(s.start, s.end);
        let u = [d[0] / len, d[1] / len];
        for t in board.segments.iter().filter(|t| t.net != source && t.net != ground && t.layer == s.layer) {
            work += 1;
            if work > 1_000_000 {
                return Err("pair screening work limit exceeded".into());
            }
            let e = sub(t.end, t.start);
            let el = distance(t.start, t.end);
            // Only genuinely parallel straight sections get a schematic; angled/transition cases are withheld.
            if cross(d, e).abs() / (len * el) > 1e-6 {
                continue;
            }
            let dot = |p: [f64; 2]| {
                let q = sub(p, s.start);
                q[0] * u[0] + q[1] * u[1]
            };
            let x = dot(t.start);
            let y = dot(t.end);
            let lo = x.min(y).max(0.);
            let hi = x.max(y).min(len);
            if hi - lo < 1e-9 {
                continue;
            }
            let a = |z: f64| [s.start[0] + z * u[0], s.start[1] + z * u[1]];
            let b = |z: f64| {
                let f = (z - x) / (y - x);
                [t.start[0] + f * e[0], t.start[1] + f * e[1]]
            };
            let gap = distance(a(lo), b(lo)) - (s.width + t.width) / 2.;
            if gap <= 0. {
                continue;
            }
            if out.len() >= 256 {
                return Err("too many parallel pairs; select a smaller net".into());
            }
            out.push(Pair {
                source: s.net.clone(),
                target: t.net.clone(),
                source_segment: s.id.clone(),
                target_segment: t.id.clone(),
                layer: s.layer.clone(),
                a: [a(lo), a(hi)],
                b: [b(lo), b(hi)],
                gap,
                length: hi - lo,
            });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod pair_tests {
    use super::*;
    use emcraft_model::pcb::{Layer, Segment};
    #[test]
    fn overlaps_are_oriented_and_nonparallel_or_different_layers_are_withheld() {
        let mut b = Board {
            name: "pair".into(),
            layers: vec![Layer { name: "top".into(), z: 0., eps_below: 4. }, Layer { name: "bottom".into(), z: 0.001, eps_below: 1. }],
            segments: vec![
                Segment { id: "a".into(), net: "A".into(), layer: "top".into(), start: [0., 0.], end: [0.04, 0.], width: 0.0003 },
                Segment { id: "b".into(), net: "B".into(), layer: "top".into(), start: [0.03, 0.001], end: [0.01, 0.001], width: 0.0003 },
            ],
            vias: vec![],
            copper: vec![],
            notes: vec![],
        };
        let p = pairs(&b, "A", "GND").unwrap();
        assert_eq!(p.len(), 1);
        assert!((p[0].length - 0.02).abs() < 1e-12);
        assert!((p[0].gap - 0.0007).abs() < 1e-12);
        assert_eq!(p[0].a[0], [0.01, 0.]);
        b.segments[1].layer = "bottom".into();
        assert!(pairs(&b, "A", "GND").unwrap().is_empty());
        b.segments[1].layer = "top".into();
        b.segments[1].end[1] = 0.003;
        assert!(pairs(&b, "A", "GND").unwrap().is_empty());
    }
}
