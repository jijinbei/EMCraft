# H-plane radius bend

A 90° bend of WR-90 in the plane of its broad dimension, as the `h-bend-radius` template: inner
radius r = 28.6 mm, outer r + a = 51.46 mm, straight arms of l = 20 mm, 10.16 mm high (E across
the height). The solid is a prism: the outline extruded along z, the ports on the arms' ends,
across y (port 1) and across x (port 2). Second-order elements of about 3 mm, five frequencies
across 8.2–12.4 GHz.

**The arc.** The template draws each wall as 18 chords inscribed in the circle; their mean
distance from the centre is short of the radius by about rα²/3 (α = 2.5°, half a chord's angle),
which makes the bend 0.3° fast. The recipe puts the chords' vertices on the radius s·r,
s = 1/(cos α · (T√(1 + T²) + asinh T)/2T), T = tan α (s = 1.00063), so that each chord's mean
distance from the centre is the arc's. A finer polygon would do the same (72 chords: within
0.08°), but every vertex adds grid lines to the mesh: 97 000 tetrahedra against 14 000.

**Reference.** The bend is uniform across the height, E = E_z(ρ, φ) ẑ, and the curved guide's
modes separate: E_z = u(ρ) e^{−jνφ}, ρ²u″ + ρu′ + (k²ρ² − ν²)u = 0 with u = 0 on both walls
(Cochran and Pecina 1966). ν₀ is found by Chebyshev collocation (`emcraft_theory::curved::
dominant`): from 4.111 at 8.2 GHz to 8.864 at 12.4 GHz, settled to 1e-12 by order 20. To leading
order S21 = e^{−jν₀π/2} e^{−2jβl}; the junctions between the straight arms and the curved guide
reflect a little and shift the phase by O(|S11|). Rice's matrix theory (1948) gives both: the
curved modes expanded in the straight guide's, matched mode by mode at each junction
(`curved::bend`, 80 modes; 40 modes agree to 1e-6). The reference is that S21; the leading term
alone is within 0.18° of it. A single junction reflects at most 0.0041 (`curved::junction`, ∝
(a/R)² for gentle bends), so the bend's |S11| stays under 2 × 0.0041: the case bounds it by
0.01. `cargo run -p emcraft-theory --example bend_radius_reference` prints the numbers.

Measured: ∠S21 within 0.08–0.12° of the reference, |S11| (0.00004–0.0053) within 7e-4 of the
mode matching's.
