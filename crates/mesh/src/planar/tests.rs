use super::*;

const MM: f64 = 1e-3;

fn names(n: usize, what: &str) -> Vec<String> {
    (1..=n).map(|k| format!("{what}{k}")).collect()
}

fn request<'a>(layers: &'a [String], nets: &'a [String], outlines: &'a [Outline]) -> Request<'a> {
    Request { lo: [0.0, 0.0], hi: [4.0 * MM, 3.0 * MM], layers, nets, outlines, lines: &[], size: 0.6 * MM, edge_size: 0.1 * MM }
}

fn rectangle(lo: [f64; 2], hi: [f64; 2]) -> Vec<[f64; 2]> {
    vec![lo, [hi[0], lo[1]], hi, [lo[0], hi[1]]]
}

/// Every triangle counterclockwise, the areas summing to the region's, each inner edge shared by
/// two triangles and the region a disc (V − E + F = 1).
fn sound(m: &Planar, area: f64) {
    let mut edges: Vec<[u32; 2]> =
        m.triangles.iter().flat_map(|t| (0..3).map(move |k| [t[k].min(t[(k + 1) % 3]), t[k].max(t[(k + 1) % 3])])).collect();
    assert!(m.triangles.iter().all(|t| m.area(t) > 0.0));
    let total: f64 = m.triangles.iter().map(|t| m.area(t)).sum();
    assert!((total - area).abs() < 1e-9 * area, "{total} {area}");
    edges.sort_unstable();
    assert!(edges.windows(3).all(|w| !(w[0] == w[1] && w[1] == w[2])), "an edge is held by more than two triangles");
    edges.dedup();
    assert_eq!(m.points.len() as i64 - edges.len() as i64 + m.triangles.len() as i64, 1, "V − E + F");
}

#[test]
fn an_empty_region_is_triangulated_to_its_size() {
    let (layers, nets) = (names(1, "L"), names(1, "n"));
    let m = triangulate(&request(&layers, &nets, &[])).unwrap();
    sound(&m, 12.0 * MM * MM);
    assert!(m.nets[0].iter().all(|&n| n == NO_NET));
    // Delaunay refinement to the size: no edge much longer than it.
    let longest = m.triangles.iter().flat_map(|t| (0..3).map(move |k| (t[k], t[(k + 1) % 3]))).map(|(a, b)| {
        let (p, q) = (m.points[a as usize], m.points[b as usize]);
        (q[0] - p[0]).hypot(q[1] - p[1])
    });
    assert!(longest.fold(0.0, f64::max) < 1.5 * 0.6 * MM);
}

#[test]
fn a_trace_covers_its_rectangle_with_fine_triangles_along_its_edges() {
    let (layers, nets) = (names(1, "L"), names(1, "n"));
    let w = 0.3 * MM;
    let outlines = [Outline { layer: 0, net: 0, ring: offset::trace([-MM, 1.5 * MM], [5.0 * MM, 1.5 * MM], w, 1e-6) }];
    let m = triangulate(&request(&layers, &nets, &outlines)).unwrap();
    sound(&m, 12.0 * MM * MM);
    assert!((m.copper_area(0) - 4.0 * MM * w).abs() < 1e-15, "{}", m.copper_area(0));
    // The triangles on the trace's edges are about the edge size.
    let on_edge = m.triangles.iter().filter(|t| t.iter().any(|&v| (m.points[v as usize][1] - 1.65 * MM).abs() < 1e-12));
    let widest = on_edge.map(|t| m.area(t).sqrt()).fold(0.0, f64::max);
    assert!(widest < 0.15 * MM, "{widest}");
}

#[test]
fn overlapping_copper_of_one_net_is_its_union() {
    let (layers, nets) = (names(1, "L"), names(1, "n"));
    // A cross of two strips and a pad over its middle: the union is the cross.
    let outlines = [
        Outline { layer: 0, net: 0, ring: rectangle([0.5 * MM, 1.0 * MM], [3.5 * MM, 1.5 * MM]) },
        Outline { layer: 0, net: 0, ring: rectangle([1.5 * MM, 0.5 * MM], [2.0 * MM, 2.5 * MM]) },
        Outline { layer: 0, net: 0, ring: rectangle([1.6 * MM, 1.1 * MM], [1.9 * MM, 1.4 * MM]) },
    ];
    let m = triangulate(&request(&layers, &nets, &outlines)).unwrap();
    sound(&m, 12.0 * MM * MM);
    let union = 3.0 * 0.5 + 0.5 * 2.0 - 0.5 * 0.5;
    assert!((m.copper_area(0) - union * MM * MM).abs() < 1e-15, "{}", m.copper_area(0));
    // The pad's edges, inside the cross, are gone: none of its corners is a point.
    let corners = [[1.6, 1.1], [1.9, 1.1], [1.9, 1.4], [1.6, 1.4]];
    assert!(m.points.iter().all(|p| corners.iter().all(|c| (p[0] - c[0] * MM).hypot(p[1] - c[1] * MM) > 1e-9)));
}

#[test]
fn copper_on_two_layers_is_kept_apart() {
    let (layers, nets) = (names(2, "L"), names(2, "n"));
    let outlines = [
        Outline { layer: 0, net: 0, ring: rectangle([0.5 * MM, 1.0 * MM], [3.5 * MM, 1.5 * MM]) },
        Outline { layer: 1, net: 1, ring: rectangle([1.5 * MM, 0.5 * MM], [2.0 * MM, 2.5 * MM]) },
    ];
    let m = triangulate(&request(&layers, &nets, &outlines)).unwrap();
    sound(&m, 12.0 * MM * MM);
    assert!((m.copper_area(0) - 1.5 * MM * MM).abs() < 1e-15);
    assert!((m.copper_area(1) - 1.0 * MM * MM).abs() < 1e-15);
    assert!(m.nets[0].iter().all(|&n| n == 0 || n == NO_NET) && m.nets[1].iter().all(|&n| n == 1 || n == NO_NET));
}

#[test]
fn two_nets_overlapping_on_a_layer_are_refused() {
    let (layers, nets) = (names(1, "L"), vec!["GND".to_string(), "CLK".to_string()]);
    let outlines = [
        Outline { layer: 0, net: 0, ring: rectangle([0.5 * MM, 1.0 * MM], [3.5 * MM, 1.5 * MM]) },
        Outline { layer: 0, net: 1, ring: rectangle([1.5 * MM, 0.5 * MM], [2.0 * MM, 2.5 * MM]) },
    ];
    let e = triangulate(&request(&layers, &nets, &outlines)).unwrap_err().to_string();
    assert!(e.contains("GND") && e.contains("CLK") && e.contains("L1"), "{e}");
}

#[test]
fn the_same_board_gives_the_same_mesh_whatever_the_order() {
    let (layers, nets) = (names(2, "L"), names(2, "n"));
    let mut outlines = vec![
        Outline { layer: 0, net: 0, ring: offset::trace([0.3 * MM, 0.4 * MM], [3.1 * MM, 2.2 * MM], 0.2 * MM, 1e-6) },
        Outline { layer: 0, net: 0, ring: offset::trace([3.1 * MM, 2.2 * MM], [3.8 * MM, 0.3 * MM], 0.2 * MM, 1e-6) },
        Outline { layer: 1, net: 1, ring: offset::disc([2.0 * MM, 1.5 * MM], 0.6 * MM, 1e-6) },
    ];
    let a = triangulate(&request(&layers, &nets, &outlines)).unwrap();
    let b = triangulate(&request(&layers, &nets, &outlines)).unwrap();
    assert_eq!(a, b);
    outlines.reverse();
    for o in &mut outlines {
        o.ring.reverse();
    }
    let c = triangulate(&request(&layers, &nets, &outlines)).unwrap();
    assert_eq!(a, c);
}

#[test]
fn hostile_requests_are_refused() {
    let (layers, nets) = (names(1, "L"), names(1, "n"));
    let good = request(&layers, &nets, &[]);
    for bad in [
        Request { size: f64::NAN, ..good },
        Request { size: 0.0, ..good },
        Request { edge_size: 1.0, ..good },
        Request { edge_size: 1e-12, ..good },
        Request { hi: [f64::INFINITY, 1.0], ..good },
        Request { hi: [-1.0, 1.0], ..good },
        Request { hi: [10.0, 10.0], ..good },
        Request { size: 1e-6, edge_size: 1e-6, ..good },
        Request { layers: &[], ..good },
    ] {
        assert!(triangulate(&bad).is_err());
    }
    let nan = [Outline { layer: 0, net: 0, ring: vec![[0.0, 0.0], [f64::NAN, 1.0], [1.0, 1.0]] }];
    assert!(triangulate(&request(&layers, &nets, &nan)).is_err());
    let elsewhere = [Outline { layer: 3, net: 0, ring: rectangle([0.0, 0.0], [1e-3, 1e-3]) }];
    assert!(triangulate(&request(&layers, &nets, &elsewhere)).is_err());
    // Degenerate copper (a sliver, a point, outside the region) is taken or dropped, never a panic.
    let odd = [
        Outline { layer: 0, net: 0, ring: vec![[1e-3, 1e-3]; 5] },
        Outline { layer: 0, net: 0, ring: vec![[1e-3, 1e-3], [2e-3, 1e-3], [3e-3, 1e-3]] },
        Outline { layer: 0, net: 0, ring: rectangle([10.0, 10.0], [11.0, 11.0]) },
    ];
    let m = triangulate(&request(&layers, &nets, &odd)).unwrap();
    assert_eq!(m.copper_area(0), 0.0);
}

#[test]
fn many_traces_at_any_angle_mesh_soundly() {
    // A plain linear congruential sequence: traces of one net per layer criss-crossing, vias as
    // discs on both layers.
    let mut seed = 12345u64;
    let mut next = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (seed >> 11) as f64 / (1u64 << 53) as f64
    };
    let (layers, nets) = (names(2, "L"), names(2, "n"));
    let mut outlines = Vec::new();
    for k in 0..120 {
        let (a, b) = ([4.0 * MM * next(), 3.0 * MM * next()], [4.0 * MM * next(), 3.0 * MM * next()]);
        outlines.push(Outline { layer: k % 2, net: (k % 2) as u32, ring: offset::trace(a, b, (0.05 + 0.1 * next()) * MM, 1e-6) });
    }
    for _ in 0..10 {
        let c = [4.0 * MM * next(), 3.0 * MM * next()];
        for layer in 0..2 {
            outlines.push(Outline { layer, net: layer as u32, ring: offset::disc(c, 0.25 * MM, 1e-6) });
        }
    }
    let r = Request { size: 0.3 * MM, edge_size: 0.05 * MM, ..request(&layers, &nets, &outlines) };
    let m = triangulate(&r).unwrap();
    sound(&m, 12.0 * MM * MM);
    for layer in 0..2 {
        let a = m.copper_area(layer);
        assert!(a > 0.3 * 12.0 * MM * MM && a < 12.0 * MM * MM, "{a}");
    }
}

#[test]
fn the_extruded_board_conforms_with_its_copper_inside() {
    use crate::{NO_TET, Topology};
    use extrude::{Slab, Stack, extrude};
    let (layers, nets) = (names(1, "L"), names(1, "n"));
    let outlines = [Outline { layer: 0, net: 0, ring: offset::trace([-MM, 0.7 * MM], [5.0 * MM, 2.1 * MM], 0.3 * MM, 1e-6) }];
    let planar = triangulate(&request(&layers, &nets, &outlines)).unwrap();
    let stack = Stack {
        z0: -1.6 * MM,
        slabs: vec![Slab { thickness: 1.6 * MM, material: 1, cells: 4, ratio: 1.0 }, Slab { thickness: 2.0 * MM, material: 0, cells: 3, ratio: 1.5 }],
        sheets: vec![(0, 1)],
        holes: vec![],
    };
    let (mesh, sheet_nets) = extrude(&planar, &stack).unwrap();
    crate::check_tetrahedra(&mesh).unwrap();
    assert_eq!(mesh.tets.len(), 3 * planar.triangles.len() * 7);
    let volume: f64 = (0..mesh.tets.len())
        .map(|t| {
            let c = mesh.corners(t).unwrap();
            emcraft_model::geom::triple(c[1] - c[0], c[2] - c[0], c[3] - c[0]) / 6.0
        })
        .sum();
    assert!((volume - 12.0 * MM * MM * 3.6 * MM).abs() < 1e-9 * volume, "{volume}");
    // Conforming: every face held by one or two tetrahedra, the outer ones exactly those tagged.
    let topo = Topology::new(&mesh);
    let outer = topo.face_tets.iter().filter(|o| o[1] == NO_TET).count();
    assert_eq!(outer + sheet_nets.len(), mesh.boundary.len());
    // The copper sheet: the covered triangles at z = 0, its area the trace's.
    let sheet: f64 = mesh
        .boundary
        .iter()
        .filter(|f| f.nodes.iter().all(|&n| mesh.nodes[n as usize].z == 0.0))
        .map(|f| {
            let p = f.nodes.map(|n| mesh.nodes[n as usize]);
            0.5 * (p[1] - p[0]).cross(p[2] - p[0]).norm()
        })
        .sum();
    assert!((sheet - planar.copper_area(0)).abs() < 1e-15, "{sheet}");
    assert!(sheet_nets.iter().all(|&n| n == 0) && !sheet_nets.is_empty());
    // Refused: no slabs, no thickness, too many cells, a sheet on a layer the board lacks.
    for bad in [
        Stack { slabs: vec![], ..stack.clone() },
        Stack { slabs: vec![Slab { thickness: 0.0, material: 0, cells: 1, ratio: 1.0 }], ..stack.clone() },
        Stack { slabs: vec![Slab { thickness: 1e-3, material: 0, cells: 1000, ratio: 1.0 }], ..stack.clone() },
        Stack { slabs: vec![Slab { thickness: 1e-3, material: 0, cells: 2, ratio: f64::NAN }], ..stack.clone() },
        Stack { sheets: vec![(5, 1)], ..stack.clone() },
    ] {
        assert!(extrude(&planar, &bad).is_err());
    }
}
