# E-plane right-angle corner

A square-cornered 90° bend of WR-90 in the plane of its narrow dimension: two arms 20 mm long,
10.16 mm wide, meeting in a 10.16 mm square, 22.86 mm high (the broad dimension, along z; E lies
in the plane of the bend). The solid is a prism, the ports on the arms' ends across y and x. The
mesh is adapted at 10 GHz (`mesh.adapt`, four passes from 4 mm), then solved with second-order
elements at 8 and 10 GHz.

**Reference.** Every field goes as sin(πz/a) across the broad dimension: from the magnetic Hertz
potential ψ ẑ, ψ = f(x, y) sin(πz/a), E = −jωμ ∇ × (ψ ẑ) lies in the plane of the bend, f obeys
(∇² + k² − (π/a)²) f = 0 with ∂f/∂n = 0 on the walls (the LSE modes, cos(nπx/b)). The corner is
solved independently by field superposition as the H-plane one (`emcraft_theory::bend::right_angle`,
2000 modes per guide, within about 1e-6); E in each guide is the derivative of f, so
S11 = −b₀/a₀ and S21 = −c₀/a₀ in f's amplitudes, ports' TE10 pointing along +x and +y.

Measured: |S11| 1e-3 low at both frequencies (the singular E at the re-entrant edge, which the
adaptive passes refine towards), phases within 0.05°; on a uniform 3 mm mesh the error is about
8e-3.
