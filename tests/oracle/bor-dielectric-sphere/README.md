# Dielectric sphere in a spherical cavity as a body of revolution

A spherical cavity of radius 10 mm with a concentric sphere of εr = 2.2 and radius 5 mm inside
it (an inclusion), solved on its meridian half-plane by `solve.revolution` in order m = 0. The
model's profile is the semicircle and the inclusion's arc, their materials taken from the model.
TE101 (E_φ only) is compared with the root of the two-layer condition: E_φ ∝ j₁(k₁r) inside,
j₁(k₀r) y₁(k₀a) − y₁(k₀r) j₁(k₀a) outside, E_φ and (r E_φ)′ continuous at r = b.

The root's script was checked in its limits: an inner sphere vanishing (b → 0) gives the empty
cavity's x = 4.493409, one filling it (b → a) the filled cavity's k₁a = 4.493409, εr = 1 the
empty one's; the bisection, with closed-form derivatives, agrees to 6e-10 with the gate test's
numerical one.
