# Roadmap

EMCraft's goal is electromagnetic simulation on par with CST Studio Suite and HFSS for microwave
and RF components, open, verified and scriptable. Each milestone lands with oracle cases against
exact solutions or published benchmarks.

| Milestone | Scope | Status |
|---|---|---|
| M0 Foundation | Workspace, quality gates (layers, no-panic lints), SI model with display units, command engine with rollback and panic guard, scripts, CLI | done |
| M1 Waveguide high-pass filter | Structured tetrahedral meshes of rectangular waveguides in sections; Nédélec elements of orders 1 and 2 with exact element matrices; modal waveguide ports (TE and TM modes); multifrontal complex symmetric LDLᵀ with Bunch–Kaufman pivots, nested dissection, relaxed supernodes, blocked threaded fronts; power-normalised S-parameters; E and H fields; closed-form TE10 references; oracle cases (empty guide, dielectric step, high-pass filter) with fixtures; convergence-rate tests; field pictures (H-plane, E-plane, cross-section) and S-parameter charts as PNG; egui desktop app with phase animation | done |
| M2 Discontinuities | H-plane and E-plane steps, irises and posts (cross-sections that change along z): mode-matching references, several port modes; iris-coupled band-pass filters against published designs | planned |
| M3 General geometry | Tetrahedral meshing of arbitrary bodies (from SolidCraft, through FieldCraft's mesher), curved boundaries, size fields per wavelength | planned |
| M4 Materials and losses | Lossy dielectrics (tan δ), finite-conductivity walls (surface impedance), dispersive materials; Q factors | planned |
| M5 Eigenmodes | Cavity resonances (generalised eigenproblem with the gradient null space removed); port mode solver for arbitrary cross-sections | planned |
| M6 Time domain | Finite integration technique / FDTD on hexahedral grids with waveguide ports and broadband S-parameters, checked against the frequency domain | planned |
| M7 Radiation | Absorbing boundaries and PML, far fields, antennas | planned |
