# H-plane T-junction

The `h-tee` template's geometry: a symmetric T-junction of WR-90 in the plane of its broad
dimension, the main guide along x and the side arm along y, each of the three arms 20 mm long,
22.86 mm wide, meeting in a 22.86 mm square, 10.16 mm high (E across the height). The solid is a
prism (the T outline extruded along z); ports 1 and 2 on the main guide's ends (across x), port 3
on the side arm's (across y). The mesh is adapted at 10.5 GHz (`mesh.adapt` from 3 mm, stopping
at its 0.002 tolerance), then solved with second-order elements at 8.5, 10.5 and 12 GHz (away
from 9.27 GHz, where the reference's expansions resonate).

**Reference.** E = E_z(x, y) ẑ, so the junction is the two-dimensional Dirichlet problem of the
TE_m0 modes, solved independently by field superposition: the junction square's field as the sum
of three modal expansions, one per open side, each vanishing on the other sides and the wall
(`emcraft_theory::tee::h_plane`, after Sieverding and Arndt 1992; the same method as
`hplane-corner` with three open sides). It converges as N^{−4/3} (the two re-entrant corners'
r^{2/3}); 1200 to 1500 modes per side moves S by under 3e-6, so 1500 modes are within about 1e-5.
It is lossless and reciprocal to 1e-12 and symmetric, S13 = S23. The reference planes are moved out
to the ports by e^{−jβ(l_i + l_j)}, l = 20 mm:
`cargo run --release -p emcraft-theory --example tee_reference -- h 1500 20 8.5 10.5 12`.

Measured: |S11| within 6.4e-4, its phase within 0.29°; |S31| within 1.1e-3 (at 12 GHz), its
phase within 0.16°; |S21| within 2e-4, |S33| within 7.5e-4; S32 = S31 to 0.01°. On uniform
meshes the errors are larger and shrink towards the reference (∠S11 at 10.5 GHz 1.18° off at
3 mm, 0.79° at 2 mm, 0.29° adapted).
