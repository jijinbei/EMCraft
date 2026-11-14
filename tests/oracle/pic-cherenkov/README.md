# Cherenkov's threshold in a dielectric-filled pillbox

Particle-in-cell (`solve.particles`, docs/adr/0012-particles.md stage C): a closed, perfectly
conducting pillbox 10 mm in radius and 39.18 mm long, filled with a lossless dielectric of
εr = 2.25 (n = 1.5); a 1 pC Gaussian bunch (σ_z = 2 mm) crosses it on its axis, entering through
one end wall and leaving through the other, first at 10 GeV (β = 1), then at 79 keV (β = 0.5).

Above Cherenkov's threshold (β > 1/n) the bunch radiates all along: the energy it leaves grows
with the cavity's length (9.41 V/pC for 13.06 mm, 25.71 for three times that), where the empty
pillbox's saturates (5.81, then 7.39). Below it (β = 0.5 < 1/n) the loss does not grow (0.89, then
0.74). Each is the modal sum q² Σ k_n e^{−(ω_n σ_t)²} over the dielectric pillbox's TM0np modes,
which the run follows to 0.65 % and 0.3 % at 0.5 mm cells.

The dielectric is the solid's own material (`geometry.solid … material`); each edge takes the
mean permittivity of the cells around it, and Gauss's law holds as ∇·(εr E) = ρ/ε₀.
