//! Waveguide ports prepared for solving: their faces, their modes (closed forms, or the
//! cross-section solver's of [`crate::portmode`]) and the parity the model's symmetry planes
//! keep.
//!
//! **Ports in lossy media.** A port whose cross-section is filled with one absorbing or
//! dispersive material — a loss tangent, a magnetic loss tangent, poles — has modes of a
//! complex k² = k₀² εr(ω) μr(1 − j tan δ_m): with one material across the aperture the modes'
//! transverse patterns and cut-offs k_c are those of the lossless guide, and only their
//! propagation constants γ = √(k_c² − k²) = α + jβ and the factors κ = γ (TE, TEM) or −k²/γ (TM)
//! become complex (J.-M. Jin, *The Finite Element Method in Electromagnetics*, 3rd ed., Wiley
//! 2014, §8.5; Pozar §3.1). Closed-form apertures take γ directly; a cross-section solved
//! numerically is solved with a unit filling and each mode told TE (or TEM) from TM by its
//! scalar part (a pair of TE and TM modes of equal cut-off mixed by the solver is solved as
//! below instead).
//!
//! **Ports across lossy layers.** A cross-section of several materials of which one absorbs or
//! disperses — a microstrip or a stripline on a lossy substrate, a guide loaded with a lossy slab
//! — has truly hybrid lossy modes: they are solved at each frequency with each triangle's complex
//! εr(ω) and μr ([`CrossSection::modes_lossy`]), each with its complex γ, and the port condition
//! takes them as the lossless hybrid modes with γ in place of jβ and the unconjugated D
//! ([`super::terms`]).

use std::collections::HashSet;

use emcraft_fem::Order;
use emcraft_fem::modes::{self, Mode};
use emcraft_fem::tri::TRI_EDGES;
use emcraft_model::{
    Boundary, BoundaryFace, End, MAX_TERMINALS, Material, Mesh, ModeSolver, Model, Point3, Port, PortShape, Symmetry, SymmetryKind, Terminals, Vec3,
};

use super::assembly::{FaceRule, Space, port_rule_points};
use super::{NONE, Result, SolveError, check_frequency, invalid, wavenumber};
use num_complex::Complex64 as C;

use crate::portmode::CrossSection;

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
    /// Terminals: the conductors of a cross-section holding two or more (one S entry each,
    /// [`super::terminals`]); 0 for a port read in its first mode.
    pub terminals: usize,
}

/// A port's modes: closed forms, or the numerical solver's on its cross-section.
#[derive(Clone, Debug)]
pub enum PortModes {
    /// A whole rectangle or circle of one material (εr, μr): each mode with its vector
    /// v_m,i = ∫ Nᵢ · e_m dS (unknown, value) over the faces meshed; `fraction` of the
    /// aperture is meshed (1, or ½ per symmetry plane), so a mode's norm there is that fraction
    /// of the whole aperture's. `medium` holds the material when it absorbs or disperses (its
    /// complex εr(ω) and μr then set the modes' γ at each frequency).
    Analytic { eps_r: f64, mu_r: f64, medium: Option<Box<Material>>, modes: Vec<(Mode, Vec<(usize, C)>)>, fraction: f64 },
    /// Any other cross-section; its modes depend on the frequency and are solved with it.
    Numeric(Box<NumericPort>),
    /// A Floquet port on an end of a periodic cell ([`super::floquet`]): plane-wave harmonics
    /// whose vectors depend on the frequency and the incidence.
    Floquet(Box<super::floquet::FloquetPort>),
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
    /// What fills the cross-section: lossless materials, one lossy or dispersive material
    /// (the section is then solved with a unit filling, and the modes' γ come from its complex
    /// k², [`super::terms`]), or several, one of them lossy or dispersive (hybrid lossy modes).
    pub(super) medium: PortMedium,
    /// Its terminals (0: read in its first mode).
    pub(super) terminals: usize,
}

/// What fills a numerically solved port's cross-section.
#[derive(Clone, Debug)]
pub enum PortMedium {
    /// Lossless, non-dispersive materials: the real modes of [`CrossSection::modes`].
    Lossless,
    /// One material throughout, lossy or dispersive.
    Uniform(Material),
    /// Several materials, one of them lossy or dispersive: each triangle's.
    Mixed(MixedMedia),
}

impl PortMedium {
    /// Whether the medium absorbs or disperses.
    pub fn is_lossy(&self) -> bool {
        !matches!(self, PortMedium::Lossless)
    }
}

/// The materials of a cross-section's triangles.
#[derive(Clone, Debug)]
pub struct MixedMedia {
    /// The distinct materials, in the model's order.
    materials: Vec<Material>,
    /// Each triangle's, an index into `materials`.
    of: Vec<usize>,
}

impl MixedMedia {
    /// One material on every one of `triangles`.
    pub(super) fn uniform(m: &Material, triangles: usize) -> MixedMedia {
        MixedMedia { materials: vec![m.clone()], of: vec![0; triangles] }
    }

    /// Each triangle's complex εr(ω) and μr(1 − j tan δ_m) at `frequency` (Hz).
    pub(crate) fn at(&self, frequency: f64) -> (Vec<C>, Vec<C>) {
        let per: Vec<(C, C)> = self.materials.iter().map(|m| (m.eps_at(frequency), m.mu_c())).collect();
        self.of.iter().map(|&k| per.get(k).copied().unwrap_or((C::new(1.0, 0.0), C::new(1.0, 0.0)))).unzip()
    }
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
    /// In a lossy or dispersive medium, the complex propagation constant γ = α + jβ (1/m);
    /// `beta2` is then −Re γ² = β² − α².
    pub gamma: Option<num_complex::Complex64>,
}

/// k² = k₀² εr(ω) μr(1 − j tan δ_m) (1/m²) and μr(1 − j tan δ_m) of a port's lossy medium at
/// `frequency` (Hz).
pub(super) fn lossy_k2(m: &Material, frequency: f64) -> (num_complex::Complex64, num_complex::Complex64) {
    let k0 = wavenumber(frequency);
    let mu = m.mu_c();
    (m.eps_at(frequency) * mu * (k0 * k0), mu)
}

/// A port's medium when it absorbs or disperses, `None` when it is lossless.
fn lossy_medium(m: &Material) -> Option<Material> {
    m.is_lossy().then(|| m.clone())
}

fn what(p: &Port) -> String {
    format!("port `{}`", p.name)
}

/// The aperture whose closed-form modes a port takes, `None` when the cross-section solver
/// gives them (asked for, or not a whole rectangle or circle of one material).
fn closed_form_shape(model: &Model, p: &Port) -> Option<PortShape> {
    if p.mode_solver == ModeSolver::Numeric { None } else { model.port_shape_of(p).ok() }
}

/// Which of a port face point's basis values have a tangential trace on the face (the plane
/// z = const): only the functions of the face's own edges and the face do; the others are
/// normal to it (zero but for rounding), and keeping them would make the dense port block
/// several times larger.
pub(super) fn tangential_trace(values: &[Vec3]) -> Vec<bool> {
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
    let (modes, terminals) = match closed_form_shape(model, p) {
        Some(shape) => (analytic_port(model, p, shape, space, faces)?, 0),
        None => {
            let n = numeric_port(model, p, space, faces)?;
            let t = n.terminals;
            (PortModes::Numeric(Box::new(n)), t)
        }
    };
    Ok(PortData { name: p.name.clone(), end: p.end, modes, reference: p.reference, deembed: p.deembed, terminals })
}

/// How many terminals a port whose cross-section is `section` has: its conductors when it asks
/// for terminals and holds two or more, else 0. Refuses more than [`MAX_TERMINALS`], and terminal
/// ports where they are not supported yet (a lossy medium, an open problem).
fn terminal_count(model: &Model, p: &Port, section: &CrossSection, lossy: bool) -> Result<usize> {
    if p.terminals == Terminals::Modal {
        return Ok(0);
    }
    let n = section.conductors().len();
    if n < 2 {
        return Ok(0);
    }
    if n > MAX_TERMINALS {
        return Err(invalid(
            what(p),
            format!("its cross-section holds {n} conductors; a terminal port takes at most {MAX_TERMINALS} (or set `terminals` modal)"),
        ));
    }
    if lossy {
        // The terminal transform needs power-orthogonal modes (½ T_Vᵀ T_I* = 1); lossy modes are
        // not, and the general theory with the modes' cross powers is not implemented.
        return Err(invalid(
            what(p),
            "not supported yet: a terminal port (several strips) in a lossy medium (its lossy modes are not power-orthogonal); set the port's `terminals` modal",
        ));
    }
    if model.open.is_some() {
        return Err(invalid(what(p), "not supported yet: a terminal port (several strips) in an open problem; set the port's `terminals` modal"));
    }
    Ok(n)
}

/// The quadrature of each of a port's faces with the port rule.
pub(super) fn port_face_rules<'a>(space: Space<'a>, faces: &'a [usize]) -> impl Iterator<Item = Result<FaceRule>> + 'a {
    let n = port_rule_points(space.order());
    faces.iter().filter_map(move |&i| space.mesh.boundary.get(i)).map(move |b| space.face_rule(b, n))
}

/// Visits each point of the port rule on a port's `faces` with its position, its weight and the
/// basis values there of the unknowns with a tangential trace on the face ([`tangential_trace`]):
/// (unknown, value), in the element's order.
pub(super) fn for_each_port_point(space: Space, faces: &[usize], mut visit: impl FnMut(Point3, f64, &[(usize, Vec3)])) -> Result<()> {
    let n = space.dofs.unknowns;
    let mut traced: Vec<(usize, Vec3)> = Vec::new();
    for face in port_face_rules(space, faces) {
        let FaceRule { el, unknowns, points, .. } = face?;
        for (l, wa, _) in points {
            let values = el.values(l);
            let kept = tangential_trace(&values);
            traced.clear();
            traced.extend(
                unknowns
                    .iter()
                    .zip(&values)
                    .zip(&kept)
                    .filter(|((u, _), t)| **u != NONE && **t && (**u as usize) < n)
                    .map(|((u, v), _)| (*u as usize, *v)),
            );
            visit(el.point(l), wa, &traced);
        }
    }
    Ok(())
}

/// A port of a whole rectangle or circle filled with one material: its closed-form modes and
/// their vectors.
fn analytic_port(model: &Model, p: &Port, shape: PortShape, space: &Space, faces: &[usize]) -> Result<PortModes> {
    let m = model.port_material(p.end)?;
    let list = port_mode_list(model, p, shape)?;
    let n = space.dofs.unknowns;
    let mut vectors: Vec<Vec<f64>> = vec![vec![0.0; n]; list.len()];
    let mut touched = vec![false; n];
    for_each_port_point(*space, faces, |x, wa, traced| {
        for (mi, mode) in list.iter().enumerate() {
            let e = mode.field(x.x, x.y);
            let e = Vec3::new(e[0], e[1], 0.0);
            for &(u, v) in traced {
                vectors[mi][u] += wa * v.dot(e);
                touched[u] = true;
            }
        }
    })?;
    let support: Vec<usize> = (0..n).filter(|&u| touched[u]).collect();
    let modes = list.into_iter().zip(vectors).map(|(mode, v)| (mode, support.iter().map(|&u| (u, C::new(v[u], 0.0))).collect())).collect();
    let medium = lossy_medium(m).map(Box::new);
    Ok(PortModes::Analytic { eps_r: m.eps_r, mu_r: m.mu_r, medium, modes, fraction: model.symmetry_fraction() })
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

/// A port of any other cross-section: its faces as the triangles of a [`crate::portmode`]
/// problem, materials from the tetrahedra behind them, and the unknowns with a trace there.
fn numeric_port(model: &Model, p: &Port, space: &Space, faces: &[usize]) -> Result<NumericPort> {
    let (section, medium) = cross_section(model, p, space.mesh, space.order(), faces)?;
    let terminals = terminal_count(model, p, &section, medium.is_lossy())?;
    // The unknowns with a tangential trace on the faces (as for the closed forms).
    let mut touched = std::collections::BTreeSet::new();
    for_each_port_point(*space, faces, |_, _, traced| touched.extend(traced.iter().map(|(u, _)| *u)))?;
    let count = (p.modes as usize).max(terminals);
    Ok(NumericPort { section, faces: faces.to_vec(), count, support: touched.into_iter().collect(), medium, terminals })
}

/// A triangle's edges, each by its two nodes in ascending order.
fn sorted_edges(t: [u32; 3]) -> [[u32; 2]; 3] {
    TRI_EDGES.map(|[i, j]| [t[i].min(t[j]), t[i].max(t[j])])
}

/// A port's cross-section from its faces: materials from the tetrahedra behind them, conductors
/// where wall faces meet it, edges bent as the volume's; with the material filling it when that
/// one absorbs or disperses (the section then has a unit filling, see the module's notes), or
/// each triangle's materials when several fill it and one of them absorbs or disperses.
fn cross_section(model: &Model, p: &Port, mesh: &Mesh, order: Order, faces: &[usize]) -> Result<(CrossSection, PortMedium)> {
    let mut global: Vec<u32> = faces.iter().filter_map(|&f| mesh.boundary.get(f)).flat_map(|b| b.nodes).collect();
    global.sort_unstable();
    global.dedup();
    let local = |g: u32| global.binary_search(&g).map(|k| k as u32).map_err(|_| invalid(what(p), "a face's node is missing"));
    let nodes = global.iter().map(|&g| mesh.node(g).ok_or_else(|| invalid(what(p), "a face's node is missing"))).collect::<Result<Vec<_>>>()?;
    let mut tris = Vec::with_capacity(faces.len());
    let (mut eps, mut mu) = (Vec::with_capacity(faces.len()), Vec::with_capacity(faces.len()));
    let mut kinds = std::collections::BTreeSet::new();
    let mut of = Vec::with_capacity(faces.len());
    for &f in faces {
        let b = mesh.boundary.get(f).ok_or_else(|| invalid(what(p), "a face is missing from the mesh"))?;
        tris.push([local(b.nodes[0])?, local(b.nodes[1])?, local(b.nodes[2])?]);
        let k = mesh.tet_material.get(b.tet as usize).copied().ok_or_else(|| invalid(what(p), "a face has no material behind it"))?;
        let m = model.materials.get(k as usize).ok_or_else(|| invalid(what(p), "a face has no material behind it"))?;
        kinds.insert(k);
        of.push(k);
        eps.push(m.eps_r);
        mu.push(m.mu_r);
    }
    let lossy: Vec<&Material> = kinds.iter().filter_map(|&k| model.materials.get(k as usize)).filter(|m| m.is_lossy()).collect();
    let medium = match (kinds.len(), lossy.first()) {
        (_, None) => PortMedium::Lossless,
        (1, Some(m)) => PortMedium::Uniform((*m).clone()),
        _ => {
            let list: Vec<u32> = kinds.iter().copied().collect();
            let materials = list
                .iter()
                .map(|&k| model.materials.get(k as usize).cloned().ok_or_else(|| invalid(what(p), "a face has no material behind it")))
                .collect::<Result<Vec<_>>>()?;
            let of = of.iter().map(|k| list.binary_search(k).unwrap_or(0)).collect();
            PortMedium::Mixed(MixedMedia { materials, of })
        }
    };
    if matches!(medium, PortMedium::Uniform(_)) {
        // One material throughout: the section's modes with a unit filling.
        eps.iter_mut().chain(mu.iter_mut()).for_each(|v| *v = 1.0);
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
    Ok((CrossSection::with_free_rim(order, nodes, tris, eps, mu, &conductors, &magnetic, &curved)?, medium))
}

/// The mode information of a closed-form mode in a lossy medium of k² `k2`.
fn lossy_info(name: String, gamma: num_complex::Complex64) -> ModeInfo {
    ModeInfo { name, beta2: -(gamma * gamma).re, closed_form: true, impedance: None, gamma: Some(gamma) }
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
                    let modes = port_mode_list(model, p, shape)?;
                    if m.is_lossy() {
                        let (k2, _) = lossy_k2(m, frequency);
                        modes.iter().map(|md| lossy_info(md.name(), md.gamma_lossy(k2))).collect()
                    } else {
                        let k = k0 * (m.eps_r * m.mu_r).sqrt();
                        modes
                            .iter()
                            .map(|md| ModeInfo {
                                name: md.name(),
                                beta2: k * k - md.cutoff().powi(2),
                                closed_form: true,
                                impedance: None,
                                gamma: None,
                            })
                            .collect()
                    }
                }
                None => {
                    let (section, medium) = cross_section(model, p, mesh, order, &faces_of_port(mesh, p)?)?;
                    match crate::driven::terms::lossy_modes(&section, &medium, frequency, p.modes as usize)? {
                        Some(lossy) => lossy
                            .gammas()
                            .into_iter()
                            .enumerate()
                            .map(|(i, g)| ModeInfo { closed_form: false, ..lossy_info(format!("mode {}", i + 1), g) })
                            .collect(),
                        None => section
                            .modes_split(k0, p.modes as usize)?
                            .into_iter()
                            .enumerate()
                            .map(|(i, m)| {
                                let impedance = section.impedance_whole(&m, k0, &model.symmetry)?;
                                Ok(ModeInfo { name: format!("mode {}", i + 1), beta2: m.beta2, closed_form: false, impedance, gamma: None })
                            })
                            .collect::<Result<Vec<_>>>()?,
                    }
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
        let beta2 = if m.is_lossy() {
            let g = md.gamma_lossy(lossy_k2(m, frequency).0);
            -(g * g).re
        } else {
            let k = wavenumber(frequency) * (m.eps_r * m.mu_r).sqrt();
            k * k - md.cutoff().powi(2)
        };
        let (x, y, inside): ((f64, f64), (f64, f64), Box<dyn Fn(f64, f64) -> bool>) = match shape {
            PortShape::Rect { x0, y0, a, b } => ((x0, x0 + a), (y0, y0 + b), Box::new(|_, _| true)),
            PortShape::Circle { cx, cy, r } => {
                ((cx - r, cx + r), (cy - r, cy + r), Box::new(move |x: f64, y: f64| (x - cx).hypot(y - cy) <= r * (1.0 + 1e-9)))
            }
        };
        let (nx, ny, points) = grid(x, y);
        let e = points.iter().map(|&(px, py)| if inside(px, py) { md.field(px, py) } else { [f64::NAN; 2] }).collect();
        return Ok(ModePattern { name: md.name(), beta2, x, y, nx, ny, e });
    }
    let mesh = model.mesh.as_ref().ok_or_else(|| SolveError::NotReady("no mesh".into()))?;
    let (section, medium) = cross_section(model, p, mesh, order, &faces_of_port(mesh, p)?)?;
    use crate::driven::terms::LossyModes;
    // Each mode's transverse field at a point of a triangle (a hybrid lossy mode's real part:
    // its phase is fixed so ∫ e_t · e_t is real), and its β² (−Re γ² in a lossy medium).
    type Field<'a> = Box<dyn Fn(usize, Point3) -> Result<Vec3> + 'a>;
    let (field, md_beta2): (Field, f64) = match crate::driven::terms::lossy_modes(&section, &medium, frequency, p.modes as usize)? {
        Some(LossyModes::Uniform(list)) => {
            let n = list.len();
            let lm = list.into_iter().nth(mode).ok_or_else(|| invalid(what(p), format!("has {n} modes, not {}", mode + 1)))?;
            let b2 = -(lm.gamma * lm.gamma).re;
            let section = &section;
            (Box::new(move |t, q| Ok(section.fields(&lm.mode, t, q)?.0)), b2)
        }
        Some(LossyModes::Hybrid(list, _)) => {
            let n = list.len();
            let lm = list.into_iter().nth(mode).ok_or_else(|| invalid(what(p), format!("has {n} modes, not {}", mode + 1)))?;
            let b2 = lm.beta2();
            let section = &section;
            (Box::new(move |t, q| Ok(section.fields_lossy(&lm, t, q)?.0.re())), b2)
        }
        None => {
            let modes = section.modes_split(wavenumber(frequency), p.modes as usize)?;
            let n = modes.len();
            let md = modes.into_iter().nth(mode).ok_or_else(|| invalid(what(p), format!("has {n} modes, not {}", mode + 1)))?;
            let b2 = md.beta2;
            let section = &section;
            (Box::new(move |t, q| Ok(section.fields(&md, t, q)?.0)), b2)
        }
    };
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
                let et = field(t, q)?;
                [et.x, et.y]
            }
            None => [f64::NAN; 2],
        });
    }
    Ok(ModePattern { name: format!("mode {}", mode + 1), beta2: md_beta2, x, y, nx, ny, e })
}
