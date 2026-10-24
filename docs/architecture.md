# Architecture

EMCraft turns a component description into a finite element model of Maxwell's equations,
solves it in the frequency domain and shows S-parameters and fields.

```
waveguide in sections ──mesh──▶ tetrahedra ──▶ Nédélec DOFs ──▶ K − k0² M + ports ──▶ LDLᵀ
 (model, SI)              (Kuhn split,      (edges, faces;    (exact element      (multifrontal,
                           tagged faces)     PEC eliminated)   matrices, modal     complex symmetric)
                                                               port terms)               │
            pictures ◀── fields on planes, S-parameters, comparison with theory ◀───────┘
```

## Crates and layers

`cargo xtask layers` enforces the table in `xtask/src/layers.rs`: a crate may depend only on
crates in strictly lower layers.

| Layer | Crate | Role |
|---|---|---|
| L0 | `emcraft-model` | Solver-neutral model: SI units and the display conversions (`units`), materials, the waveguide geometry (sections with their own aperture across x), ports, the tetrahedral mesh (materials per tetrahedron, boundary faces tagged PEC or port, the structured grid for point location), the model check. |
| L1 | `emcraft-mesh` | Structured tetrahedral meshes of waveguides (each grid cell split into six tetrahedra about its diagonal, conforming by construction; section boundaries and aperture walls are grid planes; cells outside an aperture left out; lines graded geometrically towards the re-entrant edges of steps and irises), global edge and face numbering with ascending nodes (`Topology`), point location. |
| L1 | `emcraft-fem` | Nédélec elements of the first kind, orders 1 (Whitney, 6 functions) and 2 (Webb's hierarchical basis, 20 functions), as polynomials in barycentric coordinates whose products integrate exactly; Gauss–Legendre and collapsed simplex quadrature; rectangular waveguide modes (TE/TM fields, cut-offs, propagation constants, modal port factors). |
| L1 | `emcraft-theory` | References: the TE₁₀ wave through a guide filled in sections (closed-form transmission-line cascade), S-parameters and fields; mode matching of TE_m0 modes through H-plane steps and irises (generalised scattering matrices, Redheffer star products). |
| L1 | `emcraft-render` | Pictures without a GPU: canvas with anti-aliased lines, arrows and a bitmap font; colour maps; field plots (colour map + arrows + colour bar + legend); line charts; PNG. |
| L2 | `emcraft-solver` | Complex sparse assembly; coordinate nested dissection; multifrontal complex symmetric LDLᵀ (Bunch–Kaufman pivots within supernodes, static pivoting fallback, relaxed supernode amalgamation, blocked fronts updated on threads); iterative refinement with the backward error; the driven waveguide analysis (`Driven`): PEC elimination, modal ports, power-normalised S-parameters, E and H at points, L² errors. |
| L6 | `emcraft-engine` | Commands (`CommandSpec`), `Session::execute` with rollback and a panic guard, scripts, the `highpass` sample, plane samples and field plots, S-parameter charts. |
| L7 | `emcraft-ui-egui` | The desktop UI (eframe, glow): model editing, background sweep, field and chart textures from `emcraft-render`, phase animation. |
| testkit | `emcraft-testkit` | The verification oracle (`tests/oracle`), its runner and docs/oracle.md. |
| apps | `emcraft`, `emcraft-cli` | Desktop and command line. |

Rules beyond the layer order: nothing below L7 uses egui, eframe, winit, wgpu or glow; the
testkit is a dev-dependency only.

## The solver pipeline

1. **Mesh.** `mesh.generate` puts grid lines through every aperture edge and divides each
   section along z on its own, so every material interface and aperture wall is a grid plane;
   cells outside a section's aperture are left out (an iris' or a step's metal), and lines close
   in geometrically on re-entrant edges, where the field is singular. Boundary faces at z = 0
   and z = L are ports, the rest perfect conductors. An H-plane device may use one element layer
   across y (`size_y`), exact for its y-independent fields.
2. **Degrees of freedom.** One per edge (order 1); two per edge and two per face (order 2).
   Edges and faces are oriented by ascending global node, and each element is built on its
   corners sorted the same way, so tangential continuity needs no sign bookkeeping. DOFs on
   conducting faces are eliminated (tangential E = 0).
3. **Assembly.** Element curl–curl and mass matrices are exact (products of barycentric
   polynomials) and cached for repeated shapes; K and M are assembled once; each frequency adds
   −k₀²M and the port terms (κ_m/μr) v_m v_mᵀ/‖e_m‖² of every port mode.
4. **Factorization.** Nested dissection, postorder, fundamental then relaxed supernodes, then
   one dense front per supernode: pivots chosen in blocks of 32 columns, each block updating the
   rest of the front at once on threads (deterministic).
5. **Solves.** Each port is excited by a unit TE₁₀ wave from the same factorization, with
   iterative refinement; S_pq is read by projecting E on TE₁₀ at port p.
6. **Results.** Fields at any point from the element basis (H = −∇×E/(jωμ)); field plots sample
   a plane once and draw any component at any phase.
