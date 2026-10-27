# Floquet slab

A unit cell 10 × 10 mm, periodic in x and y, holding 6 mm of air, a 5 mm slab of εr = 4 and
6 mm of air, with Floquet ports on its ends: an infinite slab under a plane wave. At normal
incidence and at θ = 30°, φ = 20° in both polarizations (the phase k_x a, k_y b across the cell
then complex, the system solved by LU), S11 and S21 against the transfer-matrix closed form of
the three layers, evaluated by a script of its own. The errors are the plane wave's dispersion
on 2.5 mm second-order elements, about 1e-3, falling as h⁴ (1e-4 at 1.25 mm; the heavy test
`slab_converges` in `crates/solver/tests/periodic.rs`).
