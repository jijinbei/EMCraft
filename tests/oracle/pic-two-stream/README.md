# The two-stream instability

Particle-in-cell (`solve.particles`, docs/adr/0012-particles.md stage D): two cold electron
streams of 10¹⁶ m⁻³ each, at ±3·10⁷ m/s, fill a box periodic along every axis on a neutralising
background, the box one wavelength long at the fastest-growing k (k v₀ = (√3/2) ω_b, 38.58 mm in
32 cells); one stream carries a velocity wave of 10⁻³ v₀ along it as a seed. The wave grows at
the largest imaginary root of the cold streams' dispersion, ω_b/2 = 2.82·10⁹ /s (Birdsall &
Langdon §5-6), until the streams trap and saturate it.

The reply's `growth_rate` fits ½ ln W of the field's energy over the stretch where it first
rises from e⁻¹² to e⁻⁴ of its peak: 2.75·10⁹ /s, 2.5 % low. The kernel's test follows the
fundamental's own amplitude (within 2.7 %), and a weak beam (1 % of the density) through a plasma
at rest, which grows as the cube root of its density — Pierce's gain — within 0.5 %.
