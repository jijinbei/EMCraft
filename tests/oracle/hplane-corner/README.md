# H-plane right-angle corner

A square-cornered 90° bend of WR-90 in the plane of its broad dimension: two arms 20 mm long,
22.86 mm wide, meeting in a 22.86 mm square, 10.16 mm high (E across the height). The solid is a
prism (`geometry.solid` with `shape` prism: the L-shaped outline extruded along z); the ports lie
on the arms' ends, across y (port 1) and across x (port 2). Second-order elements of about 3 mm,
8 and 10 GHz.

**Reference.** The fields do not vary across the height, E = E_z(x, y) ẑ, so the corner is the
two-dimensional Dirichlet problem of the TE_m0 modes, solved independently by field
superposition: the corner square's field as the sum of two modal expansions, each closed by one
of the outer walls, matched to both guides' modes on the square's open sides
(`emcraft_theory::bend::right_angle`, after Sieverding and Arndt 1992). It converges as N^{−4/3}
(the inner corner's r^{2/3} singularity); with 2000 modes per guide it is within about 1e-6, and
lossless to 1e-12. The reference planes are moved out to the ports by e^{−jβ·20 mm} each.
At 10 GHz the corner reflects |S11| = 0.432 (−7.3 dB), in line with the strong reflection of
square H-plane corners (N. Marcuvitz, Waveguide Handbook, 1951, ch. 5).

Measured: |S − S_ref| about 6e-4 at 8 GHz and 3e-4 at 10 GHz, phases within 0.07°.
