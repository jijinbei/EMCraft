//! Clean-room KiCad 6+ board reader, from the published S-expression format documentation:
//! https://dev-docs.kicad.org/en/file-formats/sexpr-pcb/ . No KiCad source code is used.
//! Only saved filled copper is treated as copper, never an unfilled zone outline.
use crate::{Error, Result};
use emcraft_model::pcb::{Board, Copper, Layer, Segment, Via};
use emcraft_model::units::Length;
use std::collections::BTreeMap;
const MAX_BYTES: usize = 8 * 1024 * 1024;
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
        if depth > 64 || self.nodes > 400_000 {
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
/// Read KiCad text with explicit stackup (or a caller-supplied stackup in SI metres).
/// Unsupported arcs fail visibly; pads and packages are explicitly excluded from diagnostics.
/// ```
/// assert!(emcraft_io::pcb::read("not a board", None).is_err());
/// ```
pub fn read(text: &str, override_layers: Option<Vec<Layer>>) -> Result<Board> {
    if text.len() > MAX_BYTES {
        return Err(bad("file exceeds 8 MiB"));
    }
    let mut parser = Parser { s: text.as_bytes(), at: 0, nodes: 0 };
    let root = parser.node(0)?;
    parser.ws();
    if root.head() != Some("kicad_pcb") || parser.at != parser.s.len() {
        return Err(bad("expected one kicad_pcb document"));
    }
    let mut layers = override_layers.unwrap_or_default();
    if layers.is_empty() {
        let stack = root
            .child("setup")
            .and_then(|n| n.child("stackup"))
            .ok_or_else(|| bad("stackup is missing: supply copper layers with z and eps_below"))?;
        let mut z = 0.0;
        let mut eps: Option<f64> = None;
        for n in stack.list().iter().filter(|n| n.head() == Some("layer")) {
            let name = n.text(1)?;
            if name.ends_with(".Cu") {
                if let Some(previous) = layers.last_mut() {
                    previous.eps_below = eps.ok_or_else(|| bad("dielectric epsilon_r is missing"))?;
                }
                layers.push(Layer { name: name.into(), z, eps_below: 1.0 });
                eps = None;
            } else if n.child("epsilon_r").is_some() || n.child("type").and_then(|n| n.text(1).ok()).is_some_and(|t| t == "prepreg" || t == "core") {
                let er = n.num("epsilon_r")?;
                if eps.is_some_and(|p| (p - er).abs() > 1e-9) {
                    return Err(bad("mixed dielectric plies: provide an explicitly homogenized stackup"));
                }
                eps = Some(er);
                z += Length::Millimetre.to_si(n.num("thickness")?);
            }
        }
    }
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
        notes: vec!["Pads, packages, solder mask and copper thickness are not included in this routing model.".into()],
    };
    for n in root.list() {
        match n.head() {
            Some("segment") => board.segments.push(Segment {
                id: n
                    .child("uuid")
                    .or_else(|| n.child("tstamp"))
                    .and_then(|n| n.text(1).ok())
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("segment:{}", board.segments.len())),
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
                });
            }
            Some("arc") => return Err(bad("curved tracks are not supported yet; no tracks were imported")),
            Some("zone") => {
                let filled: Vec<_> = n.list().iter().filter(|p| p.head() == Some("filled_polygon")).collect();
                if filled.is_empty() {
                    if board.notes.len() < 128 {
                        board.notes.push("An unfilled zone was skipped; refill copper in the PCB editor before importing.".into());
                    }
                    continue;
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
            }
            _ => {}
        }
    }
    board.check().map_err(|e| bad(&e.to_string()))?;
    Ok(board)
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
        assert!((b.layers[1].z - 0.00018).abs() < 1e-15);
        assert!((b.segments[0].width - 0.0003).abs() < 1e-15);
        let unfilled = BOARD.replace("filled_polygon", "unfilled_polygon");
        let b = read(&unfilled, None).unwrap();
        assert!(b.copper.is_empty());
        assert!(b.notes.iter().any(|n| n.contains("unfilled")));
    }
    #[test]
    fn hostile_or_unsupported_input_is_refused_without_panics() {
        for text in [
            "".into(),
            "(".repeat(100),
            "(kicad_pcb (x ".to_owned() + &"a".repeat(5000) + "))",
            BOARD.replace("0.3)", "NaN)"),
            BOARD.replace("(segment ", "(arc "),
            BOARD.replace("(epsilon_r 4.2)", ""),
            BOARD.replace("(net 1) (uuid", "(net 99) (uuid"),
        ] {
            assert!(std::panic::catch_unwind(|| read(&text, None)).unwrap().is_err());
        }
    }
}
