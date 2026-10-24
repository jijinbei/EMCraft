# Verification

Every solver capability is checked against references it cannot influence, and every accepted
result is frozen so it cannot drift.

| Kind | What it checks | Sources |
|---|---|---|
| Theory | The code solves the equations right | Closed-form solutions (waveguide modes, transmission-line cascades, cavity resonances…), identities of the discrete system (power conservation, reciprocity, symmetry), convergence rates |
| Published benchmarks | Agreement with established results on standard problems | Filter and discontinuity designs and data published in papers and textbooks |
| Experiment | The model predicts reality | Measurements published in papers |

## Three levels of tests

1. **Unit tests** in each crate: element functions dual to their edges, exact element matrices
   against the closed form and against quadrature, curls against finite differences, gradients
   in the kernel of the curl–curl matrix (the discrete de Rham property), tangential continuity
   across shared faces, linear fields spanned by order 2, mode orthogonality, factorization
   residuals (with 2 × 2 pivots), determinism, hostile inputs.
2. **Solver tests** (`crates/solver/tests/waveguide.rs`): the driven solver against the exact TE₁₀
   solutions — empty guide, dielectric step, the high-pass filter at both orders, E and H in L²
   and at points, |S11|² + |S21|² = 1, S12 = S21 and S11 = S22 to rounding; and, as heavy tests,
   the convergence rates: order 1 S-parameters at O(h²) or better and E at O(h), order 2 at
   O(h⁴) and O(h²) (measured 3.0 / 0.91 and 4.1 / 1.85).
3. **Discontinuity tests** (`crates/solver/tests/discontinuities.rs`): an inductive iris and an
   asymmetric H-plane step against converged mode matching, power conservation and reciprocity
   across ports of different aperture, the gain of grading towards edges (5× smaller error at the
   same element size), and (heavy) convergence with graded meshes.
4. **The oracle** (`tests/oracle/<case>/`, results in [oracle.md](oracle.md)): each case is our
   own model as a command script (`recipe.json`), its reference values with source, tolerance
   and the reason for the tolerance (`expected.json`), and our accepted values (`fixture.json`).

## Rules

- Reference values are computed **independently of the code under test**: the closed form,
  evaluated separately (the oracle's TE₁₀ references were computed by ABCD matrices, not with
  `emcraft-theory`, and agree with it to all digits), or another method converged far below the
  tolerance (mode matching with 640 modes for steps and irises, against the 3D FEM; its own
  tests check it reduces to the closed form for equal apertures, conserves power, is
  reciprocal and converges in the mode count).
- Each tolerance states why it is that size (mesh, element order, what converges how fast).
- A fixture is updated only deliberately (`cargo xtask oracle --bless <case>`), in a change
  that says why the result moved, never to hide a regression.

```
cargo xtask oracle                      # every case; writes docs/oracle.md
cargo xtask oracle --case highpass-filter
cargo xtask oracle --bless highpass-filter
cargo xtask test --heavy                # the convergence studies
```
