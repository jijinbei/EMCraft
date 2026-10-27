# Dispersive dielectric slab

A 20 mm slab of a dispersive dielectric filling a WR-90 guide between 10 mm of empty guide on
each side. Its permittivity (time dependence e^{jωt}, ε = ε′ − jε″) is

ε(ω) = ε∞ + Δε_D / (1 + jωτ) + Δε_L ω₀² / (ω₀² − ω² + jωγ)

with ε∞ = 2, a Debye pole Δε_D = 0.6 relaxing at 1/(2πτ) = 20 GHz and a Lorentz pole
Δε_L = 0.4 at ω₀/2π = 16 GHz damped by γ/2π = 1.5 GHz (A. Taflove, S. C. Hagness,
*Computational Electrodynamics*, 3rd ed., §9.2 and §9.4): ε = 3.05 − 0.24j at 8 GHz,
3.13 − 0.30j at 10 GHz and 3.33 − 0.41j at 12 GHz. The slab absorbs 30–64 % of the incident
power.

The exact solution is the TE10 cascade with the complex propagation constant
β = √(k₀² ε(ω) − (π/a)²) in the slab. The references were evaluated with ABCD matrices and the
pole formula written out separately from EMCraft's code (neither `Material::eps_at` nor
`emcraft-theory`). The frequency-domain solver assembles each dispersive material's own mass
matrix once and scales it by ε(ω) at each frequency, so the fast sweep's reduced basis stays
exact too; the solver's tests also check the discrete Poynting theorem with ε″(ω), and the
time-domain solver's auxiliary differential equations against the same cascade.
