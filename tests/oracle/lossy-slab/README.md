# Lossy dielectric slab

A 20 mm slab of a lossy dielectric (εr = 2.2, tan δ = 0.02) filling a WR-90 guide between
10 mm of empty guide on each side. It absorbs 10–15 % of the incident power. The exact solution
is the TE10 cascade with a complex propagation constant in the slab; the references were
evaluated with ABCD matrices separately from EMCraft's code. The solver's own tests also check
the discrete Poynting theorem: 1 − |S11|² − |S21|² equals the power ½ωε₀ε″∫|E|² the slab
absorbs, to 1e-9.
