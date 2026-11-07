# E-plane corrugated guide

WR-90 whose narrow wall waves sinusoidally, b(z) = 10.16 + 2 sin(2π(z − 10)/10) mm, for four
periods between two straight leads 10 mm long (60 mm in all): the `corrugated-guide`
template's guide. A SolidCraft design compiled into EMCraft
(`crates/templates/designs/corrugated-e.solidcraft`, written by `make-corrugated.rs.txt`), its
waving side an equation curve; imported as the region, meshed with 2 mm tetrahedra, solved at
order 2.

The reference is independent of the finite elements: the smooth wall itself, solved as the
two-dimensional problem it is (`emcraft_theory::wall`): TE₁₀'s potential H_x between the side
walls obeys the Helmholtz equation in the y–z plane, its normal derivative zero on the wall,
the wall exact at every quadrature point of spectral elements: ∠S21 152.444°, converged to a
thousandth of a degree. The finite elements meet it within 0.07° (152.375° at 2 mm, 152.423° at 1.5 mm): the
wall's NURBS patch comes from the design, and the tetrahedra's edges on it are bent onto it.
As flat facets they stood 0.25° off — the field, normal to the wall, is singular at the
facets' corners. Mode matching of
the wall cut into steps, the first reference, does not converge here (each step's edge is
singular in the same way): it moved with the number of steps and of modes alike, 152.3°–152.5°.

What it shows: TE10's propagation constant does not depend on the narrow wall's height, so the
slow-variation estimate ∫β dz predicts no change at all; yet S21 lags a straight guide's
−βL = −543.98° by 23.6°. The corrugation loads the guide with the evanescent modes it stirs
up — reactively, as a capacitive iris does — and slows the wave: the delay grows from 13° at
8.2 GHz to 47° at 12.4 GHz, towards the Bragg band where 2β = 2π/p (about 16 GHz). A broad
wall waving the same way advances the phase instead (`corrugated-hplane`).
