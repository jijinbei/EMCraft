//! Clean-room KiCad 6+ board reader, from the published S-expression format documentation:
//! https://dev-docs.kicad.org/en/file-formats/sexpr-pcb/ . No KiCad source code is used.
//! Only saved filled copper is treated as copper, never an unfilled zone outline.
use crate::{Error, Result};
use emcraft_model::pcb::{Board, Copper, Layer, Segment, Via};
use emcraft_model::units::Length;
use std::collections::BTreeMap;
const MAX_BYTES: usize = 64 * 1024 * 1024;
#[derive(Debug)]
enum Node {
    Atom(String),
    List(Vec<Node>),
}

impl Node {
    fn atom(&self) -> Option<&str> {
        match self {
            Self::Atom(a) => Some(a),
            _ => None,
        }
    }

    fn list(&self) -> &[Node] {
        match self {
            Self::List(a) => a,
            _ => &[],
        }
    }

    fn head(&self) -> Option<&str> {
        self.list().first()?.atom()
    }

    fn child(&self, k: &str) -> Option<&Node> {
        self.list().iter().find(|n| n.head() == Some(k))
    }

    fn text(&self, k: usize) -> Result<&str> {
        self.list().get(k).and_then(Node::atom).ok_or_else(|| bad("missing field"))
    }

    fn number(&self, k: usize) -> Result<f64> {
        let v = self.text(k)?.parse::<f64>().map_err(|_| bad("invalid number"))?;
        if !v.is_finite() {
            return Err(bad("non-finite number"));
        }
        Ok(v)
    }

    fn num(&self, key: &str) -> Result<f64> {
        self.child(key).ok_or_else(|| bad(key))?.number(1)
    }

    fn word(&self, key: &str) -> Result<&str> {
        self.child(key).ok_or_else(|| bad(key))?.text(1)
    }

    fn point(&self, key: &str) -> Result<[f64; 2]> {
        let n = self.child(key).ok_or_else(|| bad(key))?;
        Ok([n.number(1)?, n.number(2)?])
    }
}

fn bad(s: &str) -> Error {
    Error::Invalid(format!("PCB import: {s}"))
}

struct Parser<'a> {
    s: &'a [u8],
    at: usize,
    nodes: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.at < self.s.len() && self.s[self.at].is_ascii_whitespace() {
            self.at += 1;
        }
    }

    fn node(&mut self, depth: usize) -> Result<Node> {
        self.ws();
        self.nodes += 1;
        if depth > 64 || self.nodes > 8_000_000 {
            return Err(bad("nesting/node limit exceeded"));
        }
        let c = *self.s.get(self.at).ok_or_else(|| bad("unexpected end"))?;
        if c == b'(' {
            self.at += 1;
            let mut out = Vec::new();
            loop {
                self.ws();
                match self.s.get(self.at) {
                    Some(b')') => {
                        self.at += 1;
                        break;
                    }
                    None => return Err(bad("unclosed list")),
                    _ => out.push(self.node(depth + 1)?),
                }
            }
            return Ok(Node::List(out));
        }
        if c == b')' {
            return Err(bad("unexpected close"));
        }
        let mut out = Vec::new();
        if c == b'"' {
            self.at += 1;
            loop {
                let c = *self.s.get(self.at).ok_or_else(|| bad("unclosed string"))?;
                self.at += 1;
                match c {
                    b'"' => break,
                    b'\\' => {
                        let e = *self.s.get(self.at).ok_or_else(|| bad("incomplete escape"))?;
                        self.at += 1;
                        out.push(match e {
                            b'n' => b'\n',
                            b'r' => b'\r',
                            b't' => b'\t',
                            b'"' => b'"',
                            b'\\' => b'\\',
                            _ => return Err(bad("unsupported escape")),
                        });
                    }
                    _ => out.push(c),
                }
                if out.len() > 4096 {
                    return Err(bad("string too long"));
                }
            }
        } else {
            while let Some(c) = self.s.get(self.at) {
                if c.is_ascii_whitespace() || *c == b'(' || *c == b')' {
                    break;
                }
                out.push(*c);
                self.at += 1;
                if out.len() > 4096 {
                    return Err(bad("token too long"));
                }
            }
        }
        Ok(Node::Atom(String::from_utf8(out).map_err(|_| bad("invalid UTF-8"))?))
    }
}

/// Read KiCad text with explicit stackup (or a caller-supplied stackup in SI metres): tracks
/// (arcs as chords), vias, filled zones and the footprints' copper pads (a through-hole pad's
/// plated hole a via between the outer copper layers).
/// ```
/// assert!(emcraft_io::pcb::read("not a board", None).is_err());
/// ```
pub fn read(text: &str, override_layers: Option<Vec<Layer>>) -> Result<Board> {
    if text.len() > MAX_BYTES {
        return Err(bad("file exceeds 64 MiB"));
    }
    let mut parser = Parser { s: text.as_bytes(), at: 0, nodes: 0 };
    let root = parser.node(0)?;
    parser.ws();
    if root.head() != Some("kicad_pcb") || parser.at != parser.s.len() {
        return Err(bad("expected one kicad_pcb document"));
    }
    let layers = match override_layers {
        Some(layers) if !layers.is_empty() => layers,
        _ => stackup(&root)?,
    };
    let mut nets = BTreeMap::new();
    for n in root.list().iter().filter(|n| n.head() == Some("net")) {
        nets.insert(n.text(1)?.to_owned(), n.text(2)?.to_owned());
    }
    let net = |n: &Node| -> Result<String> {
        let key = n.word("net")?;
        nets.get(key).filter(|s| !s.is_empty()).cloned().ok_or_else(|| bad("unknown or unassigned net"))
    };
    let mut board = Board {
        name: "KiCad board".into(),
        layers,
        segments: Vec::new(),
        vias: Vec::new(),
        copper: Vec::new(),
        notes: vec!["Packages, solder mask and copper thickness are not included in this routing model; pads are copper of their nets.".into()],
        region: None,
    };
    for n in root.list() {
        match n.head() {
            Some("segment") => board.segments.push(Segment {
                id: n
                    .child("uuid")
                    .or_else(|| n.child("tstamp"))
                    .and_then(|n| n.text(1).ok())
                    .map_or_else(|| format!("segment:{}", board.segments.len()), str::to_owned),
                net: net(n)?,
                layer: n.word("layer")?.into(),
                start: n.point("start")?.map(|v| Length::Millimetre.to_si(v)),
                end: n.point("end")?.map(|v| Length::Millimetre.to_si(v)),
                width: Length::Millimetre.to_si(n.num("width")?),
            }),
            Some("via") => {
                let ls = n.child("layers").ok_or_else(|| bad("via layers missing"))?;
                board.vias.push(Via {
                    net: net(n)?,
                    at: n.point("at")?.map(|v| Length::Millimetre.to_si(v)),
                    layers: [ls.text(1)?.into(), ls.text(2)?.into()],
                    diameter: Length::Millimetre.to_si(n.num("size")?),
                    drill: n.num("drill").ok().map(|d| Length::Millimetre.to_si(d)),
                });
            }
            Some("arc") => {
                // A curved track: chords through its three points, within a twentieth of its
                // width of the circle.
                let mm = |p: [f64; 2]| p.map(|v| Length::Millimetre.to_si(v));
                let (start, mid, end) = (mm(n.point("start")?), mm(n.point("mid")?), mm(n.point("end")?));
                let width = Length::Millimetre.to_si(n.num("width")?);
                let id = n.child("uuid").or_else(|| n.child("tstamp")).and_then(|n| n.text(1).ok()).map(str::to_owned);
                let id = id.unwrap_or_else(|| format!("arc:{}", board.segments.len()));
                let (layer, net) = (n.word("layer")?.to_owned(), net(n)?);
                for (k, [a, b]) in arc_chords(start, mid, end, 0.05 * width).into_iter().enumerate() {
                    board.segments.push(Segment { id: format!("{id}:{k}"), net: net.clone(), layer: layer.clone(), start: a, end: b, width });
                }
            }
            Some("footprint") => pads(n, &board.layers.iter().map(|l| l.name.clone()).collect::<Vec<_>>(), &net, &mut board)?,
            Some("zone") => zone(n, &net, &mut board)?,
            _ => {}
        }
    }
    board.check().map_err(|e| bad(&e.to_string()))?;
    Ok(board)
}

/// The copper layers of the board's stackup, from the top: each one's height z (m) and the
/// dielectric below it (one homogeneous ply between two copper layers).
fn stackup(root: &Node) -> Result<Vec<Layer>> {
    let mut layers: Vec<Layer> = Vec::new();
    let stack =
        root.child("setup").and_then(|n| n.child("stackup")).ok_or_else(|| bad("stackup is missing: supply copper layers with z and eps_below"))?;
    let mut z = 0.0;
    let mut eps: Option<f64> = None;
    let mut tan_d: Option<f64> = None;
    for n in stack.list().iter().filter(|n| n.head() == Some("layer")) {
        let name = n.text(1)?;
        if name.ends_with(".Cu") {
            if let Some(previous) = layers.last_mut() {
                previous.eps_below = eps.ok_or_else(|| bad("dielectric epsilon_r is missing"))?;
                previous.loss_below = tan_d.unwrap_or(0.0);
            }
            layers.push(Layer { name: name.into(), z, eps_below: 1.0, loss_below: 0.0 });
            eps = None;
            tan_d = None;
        } else if n.child("epsilon_r").is_some() || n.child("type").and_then(|n| n.text(1).ok()).is_some_and(|t| t == "prepreg" || t == "core") {
            let er = n.num("epsilon_r")?;
            if eps.is_some_and(|p| (p - er).abs() > 1e-9) {
                return Err(bad("mixed dielectric plies: provide an explicitly homogenized stackup"));
            }
            eps = Some(er);
            // The ply's loss tangent, where the stackup gives one (else lossless).
            if n.child("loss_tangent").is_some() {
                let t = n.num("loss_tangent")?;
                if tan_d.is_some_and(|p| (p - t).abs() > 1e-12) {
                    return Err(bad("mixed dielectric plies: provide an explicitly homogenized stackup"));
                }
                tan_d = Some(t);
            }
            z += Length::Millimetre.to_si(n.num("thickness")?);
        }
    }
    Ok(layers)
}

/// A zone's filled polygons as copper of its net (a note when it was never filled).
fn zone(n: &Node, net: &dyn Fn(&Node) -> Result<String>, board: &mut Board) -> Result<()> {
    let filled: Vec<_> = n.list().iter().filter(|p| p.head() == Some("filled_polygon")).collect();
    if filled.is_empty() {
        if board.notes.len() < 128 {
            board.notes.push("An unfilled zone was skipped; refill copper in the PCB editor before importing.".into());
        }
        return Ok(());
    }
    for poly in filled {
        let layer = poly
            .child("layer")
            .and_then(|l| l.text(1).ok())
            .or_else(|| n.child("layer").and_then(|l| l.text(1).ok()))
            .ok_or_else(|| bad("filled polygon has no layer"))?;
        let pts = poly.child("pts").ok_or_else(|| bad("filled polygon has no points"))?;
        let polygon = pts
            .list()
            .iter()
            .filter(|p| p.head() == Some("xy"))
            .map(|p| Ok([Length::Millimetre.to_si(p.number(1)?), Length::Millimetre.to_si(p.number(2)?)]))
            .collect::<Result<Vec<_>>>()?;
        board.copper.push(Copper { net: net(n)?, layer: layer.into(), polygon });
    }
    Ok(())
}

/// The chords of the arc from `a` through `m` to `b` (m), each within `sag` of the circle (at
/// most 64); the straight segment when the three points are in a line.
fn arc_chords(a: [f64; 2], m: [f64; 2], b: [f64; 2], sag: f64) -> Vec<[[f64; 2]; 2]> {
    // The circle through the three points.
    let (bx, by, cx, cy) = (m[0] - a[0], m[1] - a[1], b[0] - a[0], b[1] - a[1]);
    let d = 2.0 * (bx * cy - by * cx);
    let span = cx.hypot(cy).max(bx.hypot(by));
    if !(d.abs() > 1e-12 * span * span) {
        return vec![[a, b]];
    }
    let (b2, c2) = (bx * bx + by * by, cx * cx + cy * cy);
    let centre = [a[0] + (cy * b2 - by * c2) / d, a[1] + (bx * c2 - cx * b2) / d];
    let r = (a[0] - centre[0]).hypot(a[1] - centre[1]);
    let angle = |p: [f64; 2]| (p[1] - centre[1]).atan2(p[0] - centre[0]);
    let (t0, tm, t1) = (angle(a), angle(m), angle(b));
    // The sweep from a to b that passes m.
    let tau = std::f64::consts::TAU;
    let ccw = |x: f64| (x - t0).rem_euclid(tau);
    let sweep = if ccw(tm) <= ccw(t1) { ccw(t1) } else { ccw(t1) - tau };
    let step = 2.0 * (1.0 - (sag / r).min(1.0)).acos();
    let n = if step > 0.0 { ((sweep.abs() / step).ceil() as usize).clamp(1, 64) } else { 64 };
    let at = |k: usize| {
        if k == n {
            return b;
        }
        let t = t0 + sweep * k as f64 / n as f64;
        [centre[0] + r * t.cos(), centre[1] + r * t.sin()]
    };
    (0..n).map(|k| [at(k), at(k + 1)]).collect()
}

/// A footprint's copper pads on the board's `layers` (`*.Cu` all of them), each a polygon of
/// its net: rectangles (rounded ones as rectangles), circles and ovals by chords; a pad with a
/// plated hole also a via between the board's first and last layers. Pads without a net, and
/// shapes not read (custom, trapezoid: their bounding rectangle), are noted.
fn pads(fp: &Node, layers: &[String], net: &dyn Fn(&Node) -> Result<String>, board: &mut Board) -> Result<()> {
    let mm = |v: f64| Length::Millimetre.to_si(v);
    let at = fp.child("at").ok_or_else(|| bad("footprint position missing"))?;
    let origin = [mm(at.number(1)?), mm(at.number(2)?)];
    let turn = at.number(3).unwrap_or(0.0).to_radians();
    // KiCad turns counterclockwise on screen with y down: x′ = x cos θ + y sin θ, y′ = −x sin θ + y cos θ.
    let rotate = |p: [f64; 2], t: f64| [p[0] * t.cos() + p[1] * t.sin(), -p[0] * t.sin() + p[1] * t.cos()];
    let mut note = |text: &str| {
        if board.notes.len() < 128 && !board.notes.iter().any(|n| n == text) {
            board.notes.push(text.into());
        }
    };
    for pad in fp.list().iter().filter(|n| n.head() == Some("pad")) {
        let kind = pad.text(2).unwrap_or("");
        let shape = pad.text(3).unwrap_or("");
        if kind == "np_thru_hole" {
            continue;
        }
        let Ok(pad_net) = net(pad) else {
            note("Pads without a net were skipped.");
            continue;
        };
        let p_at = pad.child("at").ok_or_else(|| bad("pad position missing"))?;
        let centre = rotate([mm(p_at.number(1)?), mm(p_at.number(2)?)], turn);
        let centre = [origin[0] + centre[0], origin[1] + centre[1]];
        // The pad's own angle is absolute (it includes the footprint's) when given.
        let angle = p_at.number(3).map_or(turn, f64::to_radians);
        let size = pad.child("size").ok_or_else(|| bad("pad size missing"))?;
        let (w, h) = (mm(size.number(1)?), mm(size.number(2)?));
        if !(w > 0.0 && h > 0.0) {
            return Err(bad("pad size must be positive"));
        }
        let local: Vec<[f64; 2]> = match shape {
            "circle" => (0..24).map(|k| std::f64::consts::TAU * k as f64 / 24.0).map(|t| [0.5 * w * t.cos(), 0.5 * w * t.sin()]).collect(),
            "oval" => {
                // A stadium along the longer side.
                let (r, l) = (0.5 * w.min(h), 0.5 * (w - h).abs());
                let along = w >= h;
                let mut ring = Vec::new();
                for (c, start) in [(l, -std::f64::consts::FRAC_PI_2), (-l, std::f64::consts::FRAC_PI_2)] {
                    for k in 0..=12 {
                        let t = start + std::f64::consts::PI * k as f64 / 12.0;
                        let q = [c + r * t.cos(), r * t.sin()];
                        ring.push(if along { q } else { [q[1], q[0]] });
                    }
                }
                ring
            }
            "rect" | "roundrect" => vec![[-0.5 * w, -0.5 * h], [0.5 * w, -0.5 * h], [0.5 * w, 0.5 * h], [-0.5 * w, 0.5 * h]],
            _ => {
                note("Custom and trapezoid pads were read as their bounding rectangles.");
                vec![[-0.5 * w, -0.5 * h], [0.5 * w, -0.5 * h], [0.5 * w, 0.5 * h], [-0.5 * w, 0.5 * h]]
            }
        };
        let polygon: Vec<[f64; 2]> = local.iter().map(|q| rotate(*q, angle)).map(|q| [centre[0] + q[0], centre[1] + q[1]]).collect();
        let on: Vec<&String> = match pad.child("layers") {
            Some(ls) => {
                let names: Vec<&str> = ls.list().iter().skip(1).filter_map(Node::atom).collect();
                layers.iter().filter(|l| names.iter().any(|n| *n == l.as_str() || (*n == "*.Cu"))).collect()
            }
            None => Vec::new(),
        };
        for l in &on {
            board.copper.push(Copper { net: pad_net.clone(), layer: (*l).clone(), polygon: polygon.clone() });
        }
        if kind == "thru_hole"
            && let (Some(first), Some(last)) = (layers.first(), layers.last())
            && first != last
        {
            let drill = pad.child("drill").and_then(|d| d.list().iter().skip(1).find_map(|v| v.atom()?.parse::<f64>().ok())).map(mm);
            board.vias.push(Via {
                net: pad_net,
                at: centre,
                layers: [first.clone(), last.clone()],
                diameter: w.min(h),
                drill: drill.filter(|d| *d <= w.min(h)),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    const BOARD: &str = include_str!("../tests/fixtures/route.kicad_pcb");
    #[test]
    fn explicit_stackup_and_filled_copper_are_read_in_si() {
        let b = read(BOARD, None).unwrap();
        assert_eq!(b.segments.len(), 2);
        assert_eq!(b.copper.len(), 1);
        assert_eq!(b.segments[0].net, "DATA");
        approx::assert_abs_diff_eq!(b.layers[1].z, 0.00018, epsilon = 1e-15);
        approx::assert_abs_diff_eq!(b.segments[0].width, 0.0003, epsilon = 1e-15);
        let unfilled = BOARD.replace("filled_polygon", "unfilled_polygon");
        let b = read(&unfilled, None).unwrap();
        assert!(b.copper.is_empty());
        assert!(b.notes.iter().any(|n| n.contains("unfilled")));
    }

    /// A ply's loss tangent is the dielectric's below its upper copper; without one, lossless;
    /// out of range, refused by the board's check.
    #[test]
    fn a_plys_loss_tangent_is_read() {
        let b = read(&BOARD.replace("(epsilon_r 4.2)", "(epsilon_r 4.2) (loss_tangent 0.02)"), None).unwrap();
        assert_eq!((b.layers[0].eps_below, b.layers[0].loss_below), (4.2, 0.02));
        assert_eq!(read(BOARD, None).unwrap().layers[0].loss_below, 0.0);
        assert!(read(&BOARD.replace("(epsilon_r 4.2)", "(epsilon_r 4.2) (loss_tangent -1)"), None).is_err());
    }

    #[test]
    fn arcs_become_chords_and_pads_copper() {
        let extra = r#"
 (arc (start 45 10) (mid 47.9289 11.2132) (end 49.1421 14.1421) (width 0.3) (layer "F.Cu") (net 1) (uuid "bend"))
 (footprint "R" (layer "F.Cu") (at 30 15 90)
  (pad "1" smd roundrect (at -1 0 90) (size 1 0.6) (layers "F.Cu" "F.Paste" "F.Mask") (net 1 "DATA"))
  (pad "2" thru_hole circle (at 1 0 90) (size 1.2 1.2) (drill 0.6) (layers "*.Cu" "*.Mask") (net 2 "GND"))
  (pad "" np_thru_hole circle (at 0 2) (size 1 1) (drill 1) (layers "*.Cu"))
  (pad "3" smd rect (at 0 1) (size 1 1) (layers "F.Cu")))
)"#;
        let text = BOARD.trim_end().trim_end_matches(')').to_owned() + extra;
        let b = read(&text, None).unwrap();
        // A quarter circle of radius 4.142 mm round (45, 14.142): chords within 15 µm.
        let arc: Vec<_> = b.segments.iter().filter(|s| s.id.starts_with("bend:")).collect();
        assert!(arc.len() >= 4 && arc.len() <= 64, "{}", arc.len());
        assert!((arc[0].start[0] - 0.045).abs() < 1e-12 && (arc.last().unwrap().end[1] - 0.0141421).abs() < 1e-9);
        let centre = [0.045, 0.0141421];
        for s in &arc {
            let r = (s.start[0] - centre[0]).hypot(s.start[1] - centre[1]);
            assert!((r - 0.0041421).abs() < 1e-7, "{r}");
        }
        // The SMD pad on F.Cu; the through-hole pad on both layers with a via; the unnamed and
        // netless ones skipped.
        let pads: Vec<_> = b.copper.iter().skip(1).collect();
        assert_eq!(pads.len(), 3);
        assert_eq!(b.vias.len(), 1);
        approx::assert_abs_diff_eq!(b.vias[0].drill.unwrap(), 0.0006, epsilon = 1e-15);
        // Footprint at (30, 15) turned 90°: pad 1's (−1, 0) lands at (30, 16).
        let p1 = &pads[0];
        let c = p1.polygon.iter().fold([0.0, 0.0], |s, q| [s[0] + q[0] / 4.0, s[1] + q[1] / 4.0]);
        assert!((c[0] - 0.030).abs() < 1e-12 && (c[1] - 0.016).abs() < 1e-12, "{c:?}");
        assert!(b.notes.iter().any(|n| n.contains("without a net")));
    }

    #[test]
    fn hostile_or_unsupported_input_is_refused_without_panics() {
        for text in [
            "".into(),
            "(".repeat(100),
            "(kicad_pcb (x ".to_owned() + &"a".repeat(5000) + "))",
            BOARD.replace("0.3)", "NaN)"),
            BOARD.replace("(epsilon_r 4.2)", ""),
            BOARD.replace("(net 1) (uuid", "(net 99) (uuid"),
        ] {
            assert!(std::panic::catch_unwind(|| read(&text, None)).unwrap().is_err());
        }
    }
}
