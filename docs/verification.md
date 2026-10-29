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
   asymmetric H-plane step, an E-plane step and a capacitive iris against converged mode
   matching (TE_m0 modes, or the LSE^x_1n modes for E-plane changes), power conservation and reciprocity
   across ports of different aperture, the gain of grading towards edges (5× smaller error at the
   same element size), and (heavy) convergence with graded meshes.
4. **Loss tests** (`crates/solver/tests/losses.rs`): a lossy slab against the complex TE₁₀
   cascade; walls of finite conductivity against the conductor attenuation α_c (within 0.2 %);
   the discrete Poynting theorem — 1 − |S11|² − |S21|² equal to the power the dielectric or the
   walls absorb — to 1e-9; a dispersive slab (a Debye and a Lorentz pole) against the same
   cascade with ε(ω) at each frequency, its Poynting balance with ε″(ω), and the fast sweep's
   reduced basis against full solves (oracle `dispersive-slab`). Ports in lossy media and
   magnetic losses (`crates/solver/tests/lossy_ports.rs`): guides filled with a lossy dielectric
   or a magnetically lossy material with their ports in it, closed-form and numerically solved
   port modes alike, against S21 = e^{−γL}; a step from air into a filling lossy both ways
   against the cascade; the Poynting balance with ½ωμ₀∫μ″|H|²; de-embedding by the complex γ;
   the fast sweep; the Q of a magnetically lossy cavity, exact for the discrete problem (oracles
   `lossy-line`, `magnetic-loss-line`, `lossy-step`, `cavity-magnetic-loss`). Ports across
   several materials, one of them lossy — hybrid lossy modes
   (`crates/solver/tests/hybrid_lossy_ports.rs`, `crates/solver/src/portmode/lossy.rs`): the
   complex mode solver against the real one for lossless materials, a degenerate lossy pair made
   orthogonal, the lossy slab-loaded guide against Collin's LSE/LSM equations solved in complex
   arithmetic (`emcraft-theory::slab`); a stripline of two lossy plies against the exact TEM
   γ = jk₀√(εr(1 − j tan δ)) (to 1e-6) and S21 = e^{−γL}; the lossy slab-loaded guide's S21
   against Collin's γ (α within 1 %); a shielded microstrip on FR-4 decaying by its port's own α,
   matched, reciprocal and passive; a uniform lossy filling solved by either path alike;
   terminal ports in lossy media refused (oracles `lossy-stripline`, `lossy-slab-guide`, and
   `lossy-microstrip` against Pozar's α_d with Hammerstad–Jensen's ε_eff).
5. **Eigenmode tests** (`crates/solver/tests/eigen.rs`): the four lowest resonances of a
   rectangular cavity (within 1e-4), a dielectric-filled cavity (frequencies scaled by 1/√εr,
   Q_d = 1/tan δ to 1e-6), the TE101 Q of copper walls (−1.2 % at 3 mm, converging: heavy).
6. **Time-domain tests** (`crates/fdtd/tests/waveguides.rs`): the high-pass filter against the
   exact cascade (2.5e-2 at 1.5 mm cells; heavy: 1.16e-2 → 2.8e-3 from 1 to 0.5 mm, second
   order), the inductive iris against mode matching (heavy), the dispersive slab's auxiliary
   differential equations against the cascade (heavy: second order, 3.5e-2 → 1.6e-2 from 1 to
   0.7 mm), passivity, hostile requests. On devices (`crates/gpu/tests/`): a
   serial stepper written from the plan alone matches the threads; CubeCL's CPU runtime
   matches them to 1e-9 in double and 1e-4 in single for the guide, the dispersive slab and
   the open box with ports and a dipole (in the gate); CUDA, on a machine with an NVIDIA GPU,
   the same, the same bits twice, and the factorization's fronts held by the device solving
   alike to 1e-9 (`cargo test -p emcraft-gpu --features cuda -- --ignored`). The solver's
   own test holds fronts in a host-side device written from `DeviceFront`'s documentation.
7. **Unstructured meshes** (`crates/solver/tests/unstructured.rs`): a box cavity (2.6e-5) and a
   box waveguide (S21 within 2.4e-4) meshed without structure; cylindrical and spherical cavities
   against Bessel-zero resonances, a dielectric-loaded sphere (two regions joined node for node)
   against the two-layer closed form, and a circular waveguide against the TE11 phase. Their
   tetrahedra on curved walls and interfaces are curved (quadratic maps): 4e-5 for the cylinder
   at 3 mm, converging faster than h² (heavy), against the O(h²) 5.6e-3 of flat facets, which a
   test keeps measuring. Bodies combined by Booleans: the mesh Boolean's volumes exact for boxes in
   general position and through-holes, a post across a guide (power, reciprocity), and an iris
   made of two subtracted plates against mode matching (oracle `boolean-iris`); an H-plane step read
   from STL against mode matching (oracle `imported-step`) and the readers' hostile-file tests; the
   same iris imported from SolidCraft as plates touching the guide's walls (oracle
   `imported-iris-touching`) and as the metal block around its channel, the region the air it
   encloses (`region: cavity`, oracle `imported-iris-cavity`), both against the same mode matching;
   every mesh checked for degenerate or inverted tetrahedra where it is made. The curved elements' identities — duality to curved edges, gradients
   in the curl's kernel, tangential continuity across a curved face — are tested in
   `emcraft-fem`. The mesher's own 126 tests came with it (`crates/tetmesh`).
8. **Adaptive refinement** (`crates/solver/tests/adapt.rs`, the oracle): the residual indicator
   falls at the elements' rate on a smooth guide (its square ×4.05 at order 1 and ×13.9 at
   order 2 per halving, against 4 and 16), is the same bit for bit for the same input, marks
   the tetrahedra with the most error and the remeshed solid takes their sizes; `mesh.adapt`'s
   passes are deterministic (the same final mesh bit for bit). The inductive post against an
   independent Nyström solution of its 2D integral equation (`emcraft-theory::post`, converged
   to 1e-11): the passes settle within the curved surface's geometric floor of about 1e-4
   (oracle `adaptive-post`); the Boolean iris against mode matching: from 4 mm to |S11| within
   6e-4 in six passes, four times closer than a uniform mesh of the same unknowns, the uniform
   1.2 mm mesh's error reached with a third of its unknowns (oracle `adaptive-iris`). Open
   problems (`crates/solver/tests/adapt_open.rs`, engine tests, the oracle): the indicator is
   zero in the perfectly matched layer and at a current element, takes a conductor's lifted
   trace (without it the elements on a sphere read 30 times the error) and a dielectric's
   incident-field source (without it 143 times); the layer keeps its mesh through the passes. A
   plane wave on a conducting sphere adapted from three elements per wavelength until the far
   field changes by under 2 %: σ within 0.51 % of the Mie series with 62 530 unknowns, where a
   uniform mesh is 1.04 % off with 92 528 (oracle `adaptive-pec-sphere`); the half-wave wire
   dipole fed by a lumped port until its reflection changes by under 0.02: the power balance
   0.9974 against the uniform mesh's 0.9948 at about the same unknowns, the directivity within
   the thick wire's 3 % of the closed form (oracle `adaptive-halfwave-dipole`).
9. **Open problems** (engine tests, the oracle): a Hertzian dipole in a box lined with a perfectly
   matched layer — radiated power within 0.2 % and directivity within 1–4 % at six elements per
   wavelength (oracle `dipole-radiation`); the same dipole at the centre of a sphere whose outer
   shell is a spherical layer, through a Huygens sphere — power within 0.8 % and directivity
   within 1–3 % at eight per wavelength, with the box case's unknowns (half the box's at the same
   size; oracle `dipole-radiation-spherical-pml`) — and a plane wave on a ceramic sphere against the Mie
   series, which itself meets Bohren and Huffman's BHMIE listing to 1e-5: scattering
   cross-section within 0.6 %, bistatic radar cross-section within 0.1–1.6 % (oracle
   `dielectric-sphere-scattering`); the same for a perfectly conducting sphere, its
   back-scatter at the ka ≈ 1 peak within 0.5 % (oracle `pec-sphere-scattering`). Lumped ports:
   three short gaps against the exact Hertzian self resistance and mutual impedances (oracle
   `lumped-port-coupling`), and a half-wave wire dipole's power balance and directivity (oracle
   `halfwave-dipole`; its input impedance, 111 + j27 Ω converged, has no closed form at this
   thickness and is compared with a moment-method estimate in the case's README); an open-ended waveguide fed by a
   port on a face, its accepted and radiated powers (oracle `open-waveguide`); a two-gap array driven in phase,
   its pattern, null and active resistance against the exact Hertzian array (oracle `gap-array`).
   The time domain's open problems: two gaps on a Yee grid with a CPML, their self resistance,
   mutual impedance and pattern within 0.3 % of the closed forms (oracle `time-domain-gaps`).
10. **Time-domain reflectometry** (`crates/engine/src/tdr.rs`, engine tests): the transform of
   a closed-form step, ρ e^{−j2πfτ}, steps to ρ at τ and stays there (low pass) and peaks at τ
   with |ρ| (band pass); a stripline narrowing from 3 to 1 mm steps at its round trip to the ρ
   of its two ends' cross-section impedances, 0.339 against 0.346 (heavy).
11. **Parameter studies** (`crates/engine/src/study/tests.rs`, the oracle): a variable changed
   rebuilds the model (geometry and mesh) from the history, undo goes back, the project saved
   and opened is the same, expressions and all; hostile expressions and studies are refused.
   A sweep of the high-pass filter's middle section over 10, 20 and 30 mm against the exact
   TE₁₀ cascade at each length (within 5e-4 dB of |S21|; oracle `study-sweep-highpass`); the
   Nelder–Mead optimizer on a quarter-wave matching section between an empty and an εr = 2.2
   guide finds the length within 0.002 mm of the closed form's λg/4 = 11.1326 mm, |S11| 1.3e-4
   at the model it leaves (oracle `study-optimize-quarter-wave`; order 1 at 5 mm within 2 % in
   the gate); the simplex itself on a quadratic, on a minimum outside its box, on its budget.
12. **Periodic structures** (`crates/solver/tests/periodic.rs`, `emcraft-solver::bloch`,
   `emcraft-solver::lu`, the oracle): the unsymmetric LU against random systems with zero
   diagonals; an infinite dielectric slab as a unit cell with Floquet ports at normal and
   oblique incidence, TE and TM, against the transfer-matrix closed form — 6e-4 at 2.5 mm,
   1.0e-4 at 1.25 mm (heavy: h⁴, as the guide's S) (oracle `floquet-slab`); an empty WR-90
   periodic along z against TE10's (and TE20's) dispersion, the degenerate pair at the zone's
   edge included (2e-6 to 1.3e-4; oracle `periodic-guide-dispersion`); WR-90 loaded
   periodically with a dielectric against the closed-form Bloch band cos ψ = (A + D)/2 (within
   4e-5; oracle `loaded-periodic-guide`).
13. **Coupled lines** (`crates/solver/tests/ports.rs`, `emcraft-theory::coupled`, the
   oracle): two strips' degenerate TEM modes split into the even and odd modes (equal and
   opposite currents) against Cohn's Z0e and Z0o, whole and under each symmetry plane
   (oracle `coupled-stripline-impedance`, −0.15 % and −0.28 %); an open-circuited coupled
   section's |S21| at the quarter wave, the inverter (Z0e − Z0o)/2 (oracle
   `coupled-line-coupler`, +0.0014); a three-resonator parallel-coupled band-pass filter
   designed by Cohn's and Matthaei–Young–Jones' equations against the ideal cascade of its
   sections' chain matrices, band edges within 0.45 % (oracle `coupled-line-bandpass`); terminal
   ports (one S entry per strip, the modal S converted by the strips' currents and voltages):
   a quarter-wave 10 dB stripline coupler with all four strip ends ports against the ideal TEM
   coupler — coupling within 0.06 dB, through, isolation (−60 dB), match (−50 dB), the
   coupled and through waves 90.0° apart, S unitary and symmetric to 3e-12 (oracle
   `coupled-line-coupler-4port`); the conversion itself against the closed-form coupler from an
   ideal modal S (`emcraft-solver::driven::terminals`); coupled microstrips, whose modes travel
   apart, unitary and reciprocal after per-mode de-embedding (engine tests).
14. **The oracle** (`tests/oracle/<case>/`, results in [oracle.md](oracle.md)): each case is our
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
- The whole oracle fits a standard CI runner (4 cores, 16 GB): a case stays well under a few
  GB and a few minutes. Since results are deterministic bit for bit, CI runs the oracle only
  when its inputs change (every crate the oracle is built from, the cases, the lock file and
  the compiler version, hashed into a cache key): a change to the UI, the apps or the docs
  reuses the last pass. The heavy convergence studies are skipped the same way, keyed on the
  solver and the crates below it; the gate's tests run in release, sharing the oracle's build.

```
cargo xtask oracle                      # every case; writes docs/oracle.md
cargo xtask oracle --case highpass-filter
cargo xtask oracle --bless highpass-filter
cargo xtask test --heavy                # the convergence studies
```
