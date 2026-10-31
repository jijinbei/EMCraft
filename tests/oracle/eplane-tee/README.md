# E-plane T-junction

The `e-tee` template's geometry: a symmetric T-junction of WR-90 in the plane of its narrow
dimension, the main guide along x and the side arm along y, each of the three arms 20 mm long,
10.16 mm wide, meeting in a 10.16 mm square, 22.86 mm high (the broad dimension, along z; E lies
in the plane of the junction). The solid is a prism; ports 1 and 2 on the main guide's ends
(across x), port 3 on the side arm's (across y). The mesh is adapted at 10.5 GHz (`mesh.adapt`,
four passes from 4 mm), then solved with second-order elements at 8.5, 10.5 and 12 GHz (away from
9.87 GHz, where the reference's expansions resonate).

**Reference.** Every field goes as sin(πz/a) across the broad dimension: from the magnetic Hertz
potential ψ ẑ, ψ = f(x, y) sin(πz/a), f obeys (∇² + k² − (π/a)²) f = 0 with ∂f/∂n = 0 on the
walls (the LSE modes, cos(nπx/b)). The junction is solved independently by field superposition,
three expansions in the junction square, one per open side (`emcraft_theory::tee::e_plane`, after
Sieverding and Arndt 1992; as `eplane-corner` with three open sides), 1500 modes per side, within
about 1e-5. E in each guide is the derivative of f, so with every port's TE10 pointing along +x or
+y, S_ij = −σ_i σ_j b_i/a_j in f's amplitudes (σ = −1 on the square's left and top sides). It is
lossless and reciprocal to 1e-12, and antisymmetric in the side arm: S13 = −S23. The reference
planes are moved out to the ports by e^{−jβ(l_i + l_j)}, l = 20 mm:
`cargo run --release -p emcraft-theory --example tee_reference -- e 1500 20 8.5 10.5 12`.

Measured: |S11| within 3e-4, its phase within 0.21°; |S31| within 5e-4, its phase within 0.08°;
|S21| within 4e-4, |S33| within 7.3e-4; S32 = −S31 to 0.03°.
