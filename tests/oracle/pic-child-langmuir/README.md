# A planar diode's space-charge-limited current

Particle-in-cell (`solve.particles`, docs/adr/0012-particles.md stage B): a cathode at z = 0
emits electrons at rest as the field there allows (Gauss's law: each step the charge that brings
the normal field at the wall to zero, Birdsall & Langdon 1991 §16-5), an anode 1 cm away is held
1 MV above it by the outer circuit, and the box is one cell across, periodic: a planar diode.
The current settles at the space-charge-limited one; at 1 MV the electrons reach γ ≈ 3, and the
current is Jory & Trivelpiece's (1969), 0.85 of Child's slow-particle law.

The density near a cathode emitting at rest grows as z^(−2/3), so the current converges as
h^(2/3) in the cell: 3.4 % high at 80 cells and 2.2 % at the 160 of this case. The kernel's test
(`emcraft_fdtd::pic`) extrapolates two grids at that order to the closed form within 0.1 % at
100 kV.
