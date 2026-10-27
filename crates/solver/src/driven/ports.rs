//! Waveguide ports prepared for solving: their faces, their modes (closed forms, or the
//! cross-section solver's of [`crate::portmode`]) and the parity the model's symmetry planes
//! keep.

use std::collections::HashSet;

use emcraft_fem::Order;
use emcraft_fem::modes::{self, Mode};
use emcraft_fem::tri::TRI_EDGES;
use emcraft_model::{Boundary, BoundaryFace, End, Material, Mesh, ModeSolver, Model, Point3, Port, PortShape, Symmetry, SymmetryKind, Vec3};

use super::assembly::{FaceRule, Space, port_rule_points};
use super::{NONE, Result, SolveError, check_frequency, invalid, wavenumber};
use crate::portmode::CrossSection;

/// Why a port may not lie in a dispersive material.
const DISPERSIVE_PORT: &str =
    "not supported yet: a port in a dispersive material (end the guide with a section of a non-dispersive, lossless material)";
/// Why a port may not lie in a lossy material.
const LOSSY_PORT: &str = "not supported yet: a port in a lossy material (end the guide with a lossless section)";

/// A port prepared for solving.
#[derive(Clone, Debug)]
pub struct PortData {
    pub name: String,
    pub end: End,
    pub modes: PortModes,
    /// The impedance S is referred to at this port (Ω), if any.
    pub reference: Option<f64>,
    /// How far (m) inside the model the reference plane lies.
    pub deembed: f64,
}

/// A port's modes: closed forms, or the numerical solver's on its cross-section.
#[derive(Clone, Debug)]
pub enum PortModes {
    /// A whole rectangle or circle of one material (εr, μr): each mode with its vector
    /// v_m,i = ∫ Nᵢ · e_m dS (unknown, value) over the faces meshed; `fraction` of the
    /// aperture is meshed (1, or ½ per symmetry plane), so a mode's norm there is that fraction
    /// of the whole aperture's.
    Analytic { eps_r: f64, mu_r: f64, modes: Vec<(Mode, Vec<(usize, f64)>)>, fraction: f64 },
    /// Any other cross-section; its modes depend on the frequency and are solved with it.
    Numeric(Box<NumericPort>),
}

/// A port whose modes come from [`crate::portmode`].
#[derive(Clone, Debug)]
pub struct NumericPort {
    pub section: CrossSection,
    /// The mesh's boundary face of each of the section's triangles.
    pub(super) faces: Vec<usize>,
    /// Modes in the port condition.
    pub(super) count: usize,
    /// The unknowns with a tangential trace on the port, ascending.
    pub(super) support: Vec<usize>,
}

/// A port mode at a frequency: its name and β² (rad²/m²; negative when evanescent).
#[derive(Clone, Debug, PartialEq)]
pub struct ModeInfo {
    pub name: String,
    pub beta2: f64,
    /// Whether the closed forms gave it.
    pub closed_form: bool,
    /// The characteristic impedance by power and current (Ω), for a propagating mode of a
    /// cross-section with one conductor besides the ground (a TEM or quasi-TEM line).
    pub impedance: Option<f64>,
}

fn what(p: &Port) -> String {
    format!("port `{}`", p.name)
}

/// The aperture whose closed-form modes a port takes, `None` when the cross-section solver
/// gives them (asked for, or not a whole rectangle or circle of one material).
fn closed_form_shape(model: &Model, p: &Port) -> Option<PortShape> {
    if p.mode_solver == ModeSolver::Numeric { None } else { model.port_shape_of(p).ok() }
}

fn refuse_dispersive(p: &Port, m: &Material) -> Result<()> {
    if m.is_dispersive() {
        return Err(invalid(what(p), DISPERSIVE_PORT));
    }
    Ok(())
}

/// A port's medium must be lossless and not dispersive (its modes are those of a real
/// permittivity).
fn refuse_lossy_or_dispersive(p: &Port, m: &Material) -> Result<()> {
    if m.loss_tangent != 0.0 {
        return Err(invalid(what(p), LOSSY_PORT));
    }
    refuse_dispersive(p, m)
}

/// Which of a port face point's basis values have a tangential trace on the face (the plane
/// z = const): only the functions of the face's own edges and the face do; the others are
/// normal to it (zero but for rounding), and keeping them would make the dense port block
/// several times larger.
fn tangential_trace(values: &[Vec3]) -> Vec<bool> {
    let scale = values.iter().map(|v| v.norm()).fold(0.0, f64::max);
    values.iter().map(|v| v.x.hypot(v.y) > 1e-9 * scale).collect()
}

/// A port's boundary faces: those tagged for its end, or those on its face's rectangle (all
/// three corners on the plane and inside the rectangle, to a millionth of its size).
pub(super) fn faces_of_port(mesh: &Mesh, p: &Port) -> Result<Vec<usize>> {
    let out: Vec<usize> = match &p.face {
        None => {
            let tag = match p.end {
                End::Start => Boundary::Port(0),
                End::End => Boundary::Port(1),
            };
            (0..mesh.boundary.len()).filter(|&i| mesh.boundary[i].tag == tag).collect()
        }
        Some(f) => {
            let tol = 1e-6 * f.a.max(f.b);
            let inside =
                |q: Point3| (q.z - f.z).abs() <= tol && q.x >= f.x0 - tol && q.x <= f.x0 + f.a + tol && q.y >= f.y0 - tol && q.y <= f.y0 + f.b + tol;
            (0..mesh.boundary.len())
                .filter(|&i| mesh.boundary[i].nodes.iter().all(|n| mesh.nodes.get(*n as usize).is_some_and(|q| inside(*q))))
                .collect()
        }
    };
    if out.is_empty() {
        return Err(invalid(what(p), "no boundary face lies there (is the guide closed by a face on that plane?)"));
    }
    Ok(out)
}

/// Port `p` on its boundary `faces`: the closed forms' vectors once, or the numerical modes'
/// support (their vectors come with each frequency).
pub(super) fn prepare(model: &Model, p: &Port, space: &Space, faces: &[usize]) -> Result<PortData> {
    let modes = match closed_form_shape(model, p) {
        Some(shape) => analytic_port(model, p, shape, space, faces)?,
        None => PortModes::Numeric(Box::new(numeric_port(model, p, space, faces)?)),
    };
    Ok(PortData { name: p.name.clone(), end: p.end, modes, reference: p.reference, deembed: p.deembed })
}

/// The quadrature of each of a port's faces with the port rule.
fn port_face_rules<'a>(space: Space<'a>, faces: &'a [usize]) -> impl Iterator<Item = Result<FaceRule>> + 'a {
    let n = port_rule_points(space.order());
    faces.iter().filter_map(move |&i| space.mesh.boundary.get(i)).map(move |b| space.face_rule(b, n))
}

/// A port of a whole rectangle or circle filled with one material: its closed-form modes and
/// their vectors.
fn analytic_port(model: &Model, p: &Port, shape: PortShape, space: &Space, faces: &[usize]) -> Result<PortModes> {
    let m = model.port_material(p.end)?;
    refuse_lossy_or_dispersive(p, m)?;
    let list = port_mode_list(model, p, shape)?;
    let n = space.dofs.unknowns;
    let mut vectors: Vec<Vec<f64>> = vec![vec![0.0; n]; list.len()];
    let mut touched = vec![false; n];
    for face in port_face_rules(*space, faces) {
        let FaceRule { el, unknowns, points, .. } = face?;
        for (l, wa, _) in points {
            let x = el.point(l);
            let values = el.values(l);
            let traced = tangential_trace(&values);
            for (mi, mode) in list.iter().enumerate() {
                let e = mode.field(x.x, x.y);
                let e = Vec3::new(e[0], e[1], 0.0);
                for ((u, v), _) in unknowns.iter().zip(&values).zip(&traced).filter(|((u, _), t)| **u != NONE && **t) {
                    vectors[mi][*u as usize] += wa * v.dot(e);
                    touched[*u as usize] = true;
                }
            }
        }
    }
    let support: Vec<usize> = (0..n).filter(|&u| touched[u]).collect();
    let modes = list.into_iter().zip(vectors).map(|(mode, v)| (mode, support.iter().map(|&u| (u, v[u])).collect())).collect();
    Ok(PortModes::Analytic { eps_r: m.eps_r, mu_r: m.mu_r, modes, fraction: model.symmetry_fraction() })
}

/// A closed-form port's modes: the lowest `p.modes` of its aperture, of those the model's
/// symmetry planes keep ([`kept_by`]).
fn port_mode_list(model: &Model, p: &Port, shape: PortShape) -> Result<Vec<Mode>> {
    let count = p.modes as usize;
    // Each plane keeps about half the modes: enough candidates for the count wanted.
    let candidates = if model.symmetry.is_empty() { count } else { 4 * count + 8 };
    let all: Vec<Mode> = match shape {
        PortShape::Rect { x0, y0, a, b } => modes::lowest(a, b, candidates).into_iter().map(|md| md.at(x0, y0)).collect(),
        PortShape::Circle { cx, cy, r } => modes::lowest_circular(r, candidates).into_iter().map(|md| md.at(cx, cy)).collect(),
    };
    let list: Vec<Mode> = all.into_iter().filter(|md| kept_by(md, &model.symmetry)).take(count).collect();
    if list.is_empty() {
        return Err(invalid(what(p), "no mode of its aperture has the symmetry the model's planes ask for"));
    }
    Ok(list)
}

/// Whether a mode has the parity every symmetry plane keeps: about a magnetic wall its normal E
/// is odd and its tangential E even, about an electric wall the reverse. Tested at points of
/// the aperture against their mirror images (the closed forms all have a definite parity about
/// the aperture's middle).
pub fn kept_by(mode: &Mode, planes: &[Symmetry]) -> bool {
    let samples: Vec<(f64, f64)> = match mode.aperture {
        modes::Aperture::Rect { a, b, x0, y0 } => {
            [(0.13, 0.29), (0.31, 0.71), (0.07, 0.47), (0.43, 0.11), (0.21, 0.93)].iter().map(|(u, v)| (x0 + u * a, y0 + v * b)).collect()
        }
        modes::Aperture::Circle { r, cx, cy, .. } => {
            [(0.13, 0.29), (0.31, -0.51), (-0.27, 0.47), (0.43, 0.11), (-0.21, -0.33)].iter().map(|(u, v)| (cx + u * r, cy + v * r)).collect()
        }
    };
    planes.iter().all(|pl| {
        let d = pl.axis.index();
        if d > 1 {
            return false;
        }
        let pairs: Vec<([f64; 2], [f64; 2])> = samples
            .iter()
            .map(|&(x, y)| {
                let (rx, ry) = if d == 0 { (2.0 * pl.at - x, y) } else { (x, 2.0 * pl.at - y) };
                (mode.field(x, y), mode.field(rx, ry))
            })
            .collect();
        let scale = pairs.iter().flat_map(|(e, _)| e.iter().map(|v| v.abs())).fold(0.0, f64::max);
        if !(scale > 0.0 && scale.is_finite()) {
            return false;
        }
        // The sign the normal component takes in the image; the tangential ones take the other.
        let normal = match pl.kind {
            SymmetryKind::Magnetic => -1.0,
            SymmetryKind::Electric => 1.0,
        };
        pairs.iter().all(|(e, r)| (0..2).all(|k| (r[k] - if k == d { normal } else { -normal } * e[k]).abs() <= 1e-9 * scale))
    })
}

/// A half line's impedance by power and current as the whole line's: under a magnetic wall
/// both halve, so Z = 2P/|I|² is the half's halved per plane; an electric plane separates
/// conductors the whole line holds two of, which has no single impedance.
pub(super) fn symmetric_impedance(planes: &[Symmetry], z: Option<f64>) -> Option<f64> {
    if planes.iter().any(|p| p.kind == SymmetryKind::Electric) {
        return None;
    }
    z.map(|z| z * 0.5f64.powi(planes.len() as i32))
}

/// A port of any other cross-section: its faces as the triangles of a [`crate::portmode`]
/// problem, materials from the tetrahedra behind them, and the unknowns with a trace there.
fn numeric_port(model: &Model, p: &Port, space: &Space, faces: &[usize]) -> Result<NumericPort> {
    let section = cross_section(model, p, space.mesh, space.order(), faces)?;
    // The unknowns with a tangential trace on the faces (as for the closed forms).
    let mut touched = std::collections::BTreeSet::new();
    for face in port_face_rules(*space, faces) {
        let FaceRule { el, unknowns, points, .. } = face?;
        for (l, _, _) in points {
            let values = el.values(l);
            let traced = tangential_trace(&values);
            touched.extend(unknowns.iter().zip(&traced).filter(|(u, t)| **u != NONE && **t).map(|(u, _)| *u as usize));
        }
    }
    Ok(NumericPort { section, faces: faces.to_vec(), count: p.modes as usize, support: touched.into_iter().collect() })
}

/// A triangle's edges, each by its two nodes in ascending order.
fn sorted_edges(t: [u32; 3]) -> [[u32; 2]; 3] {
    TRI_EDGES.map(|[i, j]| [t[i].min(t[j]), t[i].max(t[j])])
}

/// A port's cross-section from its faces: materials from the tetrahedra behind them, conductors
/// where wall faces meet it, edges bent as the volume's.
fn cross_section(model: &Model, p: &Port, mesh: &Mesh, order: Order, faces: &[usize]) -> Result<CrossSection> {
    let mut global: Vec<u32> = faces.iter().filter_map(|&f| mesh.boundary.get(f)).flat_map(|b| b.nodes).collect();
    global.sort_unstable();
    global.dedup();
    let local = |g: u32| global.binary_search(&g).map(|k| k as u32).map_err(|_| invalid(what(p), "a face's node is missing"));
    let nodes = global.iter().map(|&g| mesh.node(g).ok_or_else(|| invalid(what(p), "a face's node is missing"))).collect::<Result<Vec<_>>>()?;
    let mut tris = Vec::with_capacity(faces.len());
    let (mut eps, mut mu) = (Vec::with_capacity(faces.len()), Vec::with_capacity(faces.len()));
    for &f in faces {
        let b = mesh.boundary.get(f).ok_or_else(|| invalid(what(p), "a face is missing from the mesh"))?;
        tris.push([local(b.nodes[0])?, local(b.nodes[1])?, local(b.nodes[2])?]);
        let m = mesh
            .tet_material
            .get(b.tet as usize)
            .and_then(|&m| model.materials.get(m as usize))
            .ok_or_else(|| invalid(what(p), "a face has no material behind it"))?;
        refuse_lossy_or_dispersive(p, m)?;
        eps.push(m.eps_r);
        mu.push(m.mu_r);
    }
    // Edges of the faces bent onto a curved wall, by local nodes.
    let mut curved = std::collections::BTreeMap::new();
    for [a, b] in tris.iter().flat_map(|t| sorted_edges(*t)) {
        if let Some(m) = mesh.midpoint(global[a as usize], global[b as usize]) {
            curved.insert([a, b], m);
        }
    }
    // The section's edges (by local nodes) that also bound one of the boundary faces `on`
    // admits.
    let edges_on = |on: &dyn Fn(usize, &BoundaryFace) -> bool| -> Vec<[u32; 2]> {
        let set: HashSet<[u32; 2]> = mesh.boundary.iter().enumerate().filter(|(i, b)| on(*i, b)).flat_map(|(_, b)| sorted_edges(b.nodes)).collect();
        tris.iter().flat_map(|t| sorted_edges(*t)).filter(|[a, b]| set.contains(&[global[*a as usize], global[*b as usize]])).collect()
    };
    // Conductors inside the cross-section (a strip ending on the port): the section's edges that
    // also bound a wall face. (Its rim is a wall anyway.)
    let own: HashSet<usize> = faces.iter().copied().collect();
    let conductors = edges_on(&|i, b| !own.contains(&i) && b.tag == Boundary::Pec);
    // Its rim on a magnetic symmetry plane is free (the natural condition), not a wall.
    let magnetic = edges_on(&|_, b| b.tag == Boundary::Symmetry(SymmetryKind::Magnetic));
    CrossSection::with_free_rim(order, nodes, tris, eps, mu, &conductors, &magnetic, &curved)
}

/// Each port's modes at `frequency` (Hz), from the meshed model, without assembling the
/// volume: closed forms where they apply, the cross-section solver elsewhere.
pub fn port_modes(model: &Model, order: Order, frequency: f64) -> Result<Vec<(String, Vec<ModeInfo>)>> {
    check_frequency(frequency)?;
    let mesh = model.mesh.as_ref().ok_or_else(|| SolveError::NotReady("no mesh".into()))?;
    if model.ports.is_empty() {
        return Err(SolveError::NotReady("no ports: create one with model.port.create".into()));
    }
    let k0 = wavenumber(frequency);
    model
        .ports
        .iter()
        .map(|p| {
            let list = match closed_form_shape(model, p) {
                Some(shape) => {
                    let m = model.port_material(p.end)?;
                    refuse_dispersive(p, m)?;
                    let k = k0 * (m.eps_r * m.mu_r).sqrt();
                    let modes = port_mode_list(model, p, shape)?;
                    modes
                        .iter()
                        .map(|md| ModeInfo { name: md.name(), beta2: k * k - md.cutoff().powi(2), closed_form: true, impedance: None })
                        .collect()
                }
                None => {
                    let section = cross_section(model, p, mesh, order, &faces_of_port(mesh, p)?)?;
                    section
                        .modes(k0, p.modes as usize)?
                        .into_iter()
                        .enumerate()
                        .map(|(i, m)| {
                            let impedance = symmetric_impedance(&model.symmetry, section.impedance(&m, k0)?);
                            Ok(ModeInfo { name: format!("mode {}", i + 1), beta2: m.beta2, closed_form: false, impedance })
                        })
                        .collect::<Result<Vec<_>>>()?
                }
            };
            Ok((p.name.clone(), list))
        })
        .collect()
}

/// A port mode's transverse electric field on a grid over the port's face.
#[derive(Clone, Debug, PartialEq)]
pub struct ModePattern {
    pub name: String,
    /// β² (rad²/m²): positive for a propagating mode.
    pub beta2: f64,
    /// The grid's span (model x and y, m) and size.
    pub x: (f64, f64),
    pub y: (f64, f64),
    pub nx: usize,
    pub ny: usize,
    /// e_t at each grid point, row-major from the lowest y; NaN outside the cross-section (in a
    /// conductor).
    pub e: Vec<[f64; 2]>,
}

/// Most grid points a mode pattern samples along a side.
pub const MAX_PATTERN_SIDE: usize = 400;

/// Mode `mode` (from 0) of port `port` at `frequency`, its transverse E sampled on a grid of
/// about `side` points along the face's longer side: the closed form, or the cross-section's
/// solution (as `port_modes` finds them).
pub fn port_mode_pattern(model: &Model, order: Order, frequency: f64, port: usize, mode: usize, side: usize) -> Result<ModePattern> {
    check_frequency(frequency)?;
    let side = side.clamp(8, MAX_PATTERN_SIDE);
    let p = model.ports.get(port).ok_or_else(|| invalid("port mode", format!("no port {}", port + 1)))?;
    let grid = |x: (f64, f64), y: (f64, f64)| {
        let (w, h) = (x.1 - x.0, y.1 - y.0);
        let (nx, ny) =
            if w >= h { (side, ((side as f64 * h / w).round() as usize).max(8)) } else { (((side as f64 * w / h).round() as usize).max(8), side) };
        let points: Vec<(f64, f64)> =
            (0..ny).flat_map(|j| (0..nx).map(move |i| (x.0 + w * i as f64 / (nx - 1) as f64, y.0 + h * j as f64 / (ny - 1) as f64))).collect();
        (nx, ny, points)
    };
    if let Some(shape) = closed_form_shape(model, p) {
        let list = port_mode_list(model, p, shape)?;
        let md = list.get(mode).ok_or_else(|| invalid(what(p), format!("has {} modes, not {}", list.len(), mode + 1)))?;
        let m = model.port_material(p.end)?;
        let k = wavenumber(frequency) * (m.eps_r * m.mu_r).sqrt();
        let (x, y, inside): ((f64, f64), (f64, f64), Box<dyn Fn(f64, f64) -> bool>) = match shape {
            PortShape::Rect { x0, y0, a, b } => ((x0, x0 + a), (y0, y0 + b), Box::new(|_, _| true)),
            PortShape::Circle { cx, cy, r } => {
                ((cx - r, cx + r), (cy - r, cy + r), Box::new(move |x: f64, y: f64| (x - cx).hypot(y - cy) <= r * (1.0 + 1e-9)))
            }
        };
        let (nx, ny, points) = grid(x, y);
        let e = points.iter().map(|&(px, py)| if inside(px, py) { md.field(px, py) } else { [f64::NAN; 2] }).collect();
        return Ok(ModePattern { name: md.name(), beta2: k * k - md.cutoff().powi(2), x, y, nx, ny, e });
    }
    let mesh = model.mesh.as_ref().ok_or_else(|| SolveError::NotReady("no mesh".into()))?;
    let section = cross_section(model, p, mesh, order, &faces_of_port(mesh, p)?)?;
    let modes = section.modes(wavenumber(frequency), p.modes as usize)?;
    let md = modes.get(mode).ok_or_else(|| invalid(what(p), format!("has {} modes, not {}", modes.len(), mode + 1)))?;
    let (lo, hi) = section
        .nodes
        .iter()
        .fold(([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]), |(l, h), q| ([l[0].min(q.x), l[1].min(q.y)], [h[0].max(q.x), h[1].max(q.y)]));
    let (x, y) = ((lo[0], hi[0]), (lo[1], hi[1]));
    let (nx, ny, points) = grid(x, y);
    let mut e = Vec::with_capacity(points.len());
    for &(px, py) in &points {
        let q = Point3::new(px, py, 0.0);
        e.push(match section.locate(q) {
            Some(t) => {
                let (et, _) = section.fields(md, t, q)?;
                [et.x, et.y]
            }
            None => [f64::NAN; 2],
        });
    }
    Ok(ModePattern { name: format!("mode {}", mode + 1), beta2: md.beta2, x, y, nx, ny, e })
}
