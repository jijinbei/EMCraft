# E-plane radius bend

A 90° bend of WR-90 in the plane of its narrow dimension, as the `e-bend-radius` template: inner
radius r = 35 mm, outer r + b = 45.16 mm, straight arms of l = 20 mm, 22.86 mm high (E in the
bend's plane). The solid is a prism, the ports on the arms' ends across y and x. Second-order
elements of about 3 mm, five frequencies across 8.2–12.4 GHz. The arc is 18 chords with their
vertices on the radius whose chords' mean distance from the centre is the arc's, as in
`hplane-bend-radius` (the template's inscribed chords are slightly fast).

**Reference.** Every field goes as sin(πz/a) across the broad dimension; from the magnetic
Hertz potential ψ ẑ, ψ = u(ρ) e^{−jνφ} sin(πz/a), the curved guide's LSE modes satisfy
ρ²u″ + ρu′ + (k_c²ρ² − ν²)u = 0, k_c² = k² − (π/a)², with u′ = 0 on both walls (Cochran and
Pecina 1966; Lewin, Chang and Kuester 1977). ν₀ by Chebyshev collocation
(`emcraft_theory::curved::dominant`): from 4.130 at 8.2 GHz to 8.865 at 12.4 GHz, settled to
1e-13 by order 16. The junctions by Rice's matrix theory (`curved::bend`, 80 modes; 40 agree to
1e-9); the leading term −ν₀π/2 − 2βl alone is within 0.16° of it. E is radial in the bend: the
ports' E along +x (port 1) and +y (port 2) is the radial field reversed at port 1 and kept at
port 2, so S21 carries a further 180°. A single junction reflects at most 0.0019, (b/R)² = 0.064
times a small factor; the case bounds |S11| by 0.006.

Measured: ∠S21 within 0.06° of the reference, |S11| (0.0008–0.0036) within 8e-4 of the mode
matching's.
