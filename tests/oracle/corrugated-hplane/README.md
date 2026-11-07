# H-plane corrugated guide

WR-90 whose broad wall waves sinusoidally, a(z) = 22.86 + 2 sin(2π(z − 10)/10) mm, for four
periods between two straight leads 10 mm long (60 mm in all). The guide is a SolidCraft design
compiled into EMCraft (`crates/templates/designs/corrugated-h.solidcraft`, written by
`make-corrugated.rs.txt`): the air as one body, a profile extruded across the narrow wall, its
waving side an equation curve fitted within 1e-4 mm and placed again from the formula when a
parameter changes. Imported as the region (`interior`), meshed with 2 mm tetrahedra, ports on
its end faces, solved at order 2.

The reference is independent of the finite elements: the smooth wall itself, solved as the
two-dimensional problem it is (`emcraft_theory::wall`, E_y zero on the wall, the wall exact):
∠S21 −172.366°. Mode matching of the wall cut into steps agrees with it within 0.1° from 1280
steps (the field is zero on the wall, so steps model it fairly). The finite elements' flat
triangles stand 0.28° off (−172.646°).

What it shows: a broad wall waving faster than the wave (period 10 mm against a guide
wavelength of 39.7 mm) *advances* the phase, by about 11.6° against a straight guide's
−βL = −543.98°. The slow-variation estimate ∫β(a(z)) dz gives only 2.0° of that: the rest is
the wave's coupling to the evanescent modes the wall stirs up, which the steps and the
finite elements both carry. A narrow wall waving the same way delays it instead
(`corrugated-eplane`).
