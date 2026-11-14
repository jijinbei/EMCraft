# A cold plasma's oscillation

Particle-in-cell (`solve.particles`, docs/adr/0012-particles.md stage B): electrons of
10¹⁶ m⁻³ fill a box periodic along every axis, 160 mm long in 32 cells of 5 mm, 16 macro-particles
a cell on a regular lattice over a neutralising background of immobile ions. Their velocities
carry a wave of 10 km/s, one wavelength along the box; the charge it bunches pulls them back,
and E_x at a quarter along rings at the plasma frequency ω_p = √(n e²/(ε₀ mₑ)), 0.8979 GHz,
whatever the wavelength (Birdsall & Langdon 1991, §4-2).

The grid's own frequency is a little lower: charge assigned and the field gathered with the
first-order shape (each sinc²(kh/2)), Gauss's difference (2/h) sin(kh/2) for k, and the leapfrog
sin(ωΔt/2) = ω_pΔt/2 together put it 0.23 % below ω_p at kh = 2π/32 and ω_pΔt = 0.052, as the
run gives (`emcraft_fdtd::pic`'s test checks this to 2e-4). The frequency is that of the upward
crossings of the probe's field through its mean.
