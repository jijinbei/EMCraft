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
crates in strictly lower layers, or on a crate of its own layer that `INTRA_LAYER_ORDER` puts
before it (theory and mesh on fem). Points and vectors are `Point3` and `Vec3`, complex phasor
vectors `CVec3` (`emcraft_model::geom`), from the model up.

| Layer | Crate | Role |
|---|---|---|
| L0 | `emcraft-model` | Solver-neutral model: SI units and the display conversions (`units`), materials, the waveguide geometry (sections with their own aperture across x), ports, the tetrahedral mesh (materials per tetrahedron, boundary faces tagged PEC or port, the structured grid for point location), the model check. |
| L0 | `emcraft-tetmesh` | The unstructured tetrahedral mesher, ported from FieldCraft's: exact and adaptive predicates, boundary-surface preparation and checks, Bowyer–Watson Delaunay, constrained boundary recovery (Diazzi et al. 2023), the quality stage (graded size field, Delaunay refinement, ODT smoothing, Klingner–Shewchuk improvement, snapping to exact surfaces), element quality. No workspace dependencies (a small compatibility module stands in for what it took from FieldCraft's model; its own `Point3` and `Vec3`, converted to the model's in `emcraft-mesh`). |
| L0 | `emcraft-macros` | Declarative macros any crate may use, no workspace dependencies (the model, also L0, comes after it): `named_enum!` (an enum whose variants each have a text id, aliases and a label), `named_serde!` (serde by id), the `Named` trait they implement. |
| L1 | `emcraft-mesh` | Structured tetrahedral meshes of waveguides (each grid cell split into six tetrahedra about its diagonal, conforming by construction; section boundaries and aperture walls are grid planes; cells outside an aperture left out; lines graded geometrically towards the re-entrant edges of steps and irises), global edge and face numbering with ascending nodes (`Topology`), point location (grid or buckets); primitive bodies (box, cylinder, sphere) tessellated with exact surfaces and meshed by `emcraft-tetmesh` (`solid`).; unstructured meshes of primitive bodies through `emcraft-tetmesh`, with inclusions as regions joined node for node, Boolean features (`csg`: a mesh Boolean on exact predicates) and edges bent onto curved surfaces; adaptive refinement (`adapt`: Dörfler marking, the marked tetrahedra as graded local sizes of a new mesh, `solid::solid_with` with `SolidOptions::refine`); imported bodies' faces flush with the region's moved off them (`flush`), the points a Boolean leaves inside straight creases collapsed (`crease`), the air a part's metal bodies enclose (`cavity`); every mesh checked for degenerate or inverted tetrahedra (`check_tetrahedra`) |
| L1 | `emcraft-fem` | Nédélec elements of the first kind, orders 1 (Whitney, 6 functions) and 2 (Webb's hierarchical basis, 20 functions), as polynomials in barycentric coordinates whose products integrate exactly; curved (quadratic, covariant Piola) elements integrated by quadrature; Gauss–Legendre and collapsed simplex quadrature; Bessel functions; rectangular and circular waveguide modes (TE/TM fields, cut-offs, propagation constants, modal port factors). |
| L1 | `emcraft-theory` | References: the Huygens-surface near-to-far transform both solvers share (`radiation::Huygens`); the TE₁₀ wave through a guide filled in sections (closed-form transmission-line cascade), S-parameters and fields; Bessel-function zeros and the resonances of cylindrical and spherical cavities; mode matching through H-plane steps and inductive irises (TE_m0 modes) and E-plane steps and capacitive irises (LSE^x_1n modes), generalised scattering matrices, Redheffer star products; the full-height post across a guide by a Nyström method on its 2D integral equation (`post`); plane waves through layer stacks at oblique incidence (TE/TM) and Bloch waves of periodically loaded guides (`floquet`). |
| L1 | `emcraft-io` | Geometry import: closed triangulated surfaces from STL (binary, ASCII) and 3MF packages (components flattened, model parts followed), in metres, grouped into faces by the angle between neighbouring triangles; each face's exact surface (plane, cylinder, sphere, cone, torus) recovered by least-squares fits to its vertices (`fit`), so the mesher and the curved elements follow imported curved faces; untrusted files capped and checked. |
| L1 | `emcraft-render` | Pictures without a GPU: canvas with anti-aliased lines, arrows and text (DejaVu Sans through `ab_glyph`, `text`; a bitmap font as fallback); colour maps; the colours of pictures by role (`palette`: a `Palette` per theme, `Pen`s resolved when drawn — the palettes themselves come from theme files); field plots (colour map + arrows + colour bar + legend); line charts; 3D views (`view3d`: flat-shaded triangles and lines with a depth buffer, an orbit camera, perspective or orthographic, deterministic); PNG. |
| L2 | `emcraft-fdtd` | The time-domain solver: finite integration (Yee) on the tensor grid of the waveguide's aperture and section lines, CPML along z, a TE10 pulse in feeds beyond the ports, S-parameters by Fourier transforms of the TE10 amplitudes on the end faces against a uniform-guide reference run; z-slabs on threads that live for the run (atomics and barriers, no `unsafe`, results independent of the thread count). Modules: `yee` (the staggered fields, CPML grading, pulse, running transforms and the slab-threaded leapfrog both runs share), `guide` (the waveguide run), `dispersion` (the slots of dispersive and lossy edges: ADE poles, conductivity by the exponential update, the walls' branches of surface current; magnetic losses' decay), `surface` (walls of finite conductivity: Leontovich's admittance as a sum of first-order branches), `open` (open problems: a box lined with CPML on all faces, lossy, dispersive and magnetic bodies, lumped ports, dipoles, a Huygens box). Loss tangents become conductivities matched at the band's centre (`Losses`), ports in lossy and dispersive media take the complex TE10 γ. |
| L2 | `emcraft-gpu` | The solvers on a GPU or the CPU's SIMD through CubeCL ([ADR 0002](adr/0002-gpu.md)): `emcraft-fdtd`'s `Plan` (the guide, with dispersive edges) and `OpenPlan` (the open box: CPML on every face, lumped ports, current elements, the Huygens box's transforms) stepped by kernels in single or double precision, the projections and ports read back a batch at a time; `Fronts`, an `emcraft_solver::accel::Accelerator` that holds a factorization's large fronts and runs their interchanges and block updates in complex double. Modules: `device` (backends, precisions, the panic guard), `buffer`, `guide`, `open`, `front`, `kernels/{guide,open,front}`. Backends: CUDA (the `cuda` feature) and CubeCL's CPU runtime. |
| L2 | `emcraft-solver` | Complex sparse assembly; coordinate nested dissection; multifrontal complex symmetric LDLᵀ (Bunch–Kaufman pivots within supernodes, static pivoting fallback, relaxed supernode amalgamation, blocked fronts updated on threads); iterative refinement with the backward error; the driven waveguide analysis (`Driven`): PEC elimination, modal ports, power-normalised S-parameters, E and H at points, L² errors, absorbed powers; walls of finite conductivity (Leontovich); cavity eigenmodes and Q factors (`eigen`); a residual a-posteriori error indicator per tetrahedron (`adapt`); terminal ports, a cross-section of several strips read per strip by its modes' conductor currents and voltages (`driven::terminals`); periodic cells: the master/slave reduction Tᴴ A T of phase-shifted faces (`driven::periodic`), Floquet ports (`driven::floquet`), a multifrontal LU for the reduced unsymmetric systems (`lu`, on the LDLᵀ's analysis and multifrontal driver, `multifrontal`) and dispersion diagrams by Hermitian shift-invert subspace iteration (`bloch`). |
| L6 | `emcraft-engine` | Commands: the table in `cmd/table.rs` (`CommandSpec`: id, label, documented parameters with their kind, unit and choices, effect on the job, run), the commands themselves by area (`geometry`, `meshing`, `solve`, `results`, `open`, `project`), their parameters read through `Params` (checked, converted to SI at the boundary, expressions of the project's variables evaluated by `expr`); parameter studies (`study`: variables, sweeps and Nelder–Mead optimization on models rebuilt from the history); `Session::execute` with rollback and a panic guard, the session's `Results`, scripts, samples, the workflow's steps and statuses (`workflow`), pictures by subject (`view`: field planes and their samples, port modes, the references S-parameters are compared with, S-parameter, TDR, dispersion, pattern and study charts, and the `view.*` commands, in a theme's colours); themes as files (`theme`: the format of docs/themes.md, the built-in `themes/light.json` and `dark.json` compiled in, `theme.check`); progress and cancellation of long commands (`progress`). |
| L7 | `emcraft-ui-egui` | The desktop UI (eframe, glow), in SolidCraft's style: ribbon of engine commands, browser, dialogs generated from the commands' parameter specs, results views (field and chart textures from `emcraft-render`, phase animation, tables), long commands on a worker thread with progress and Cancel (`emcraft_engine::progress`), the control channel's methods, a CPU renderer of whole frames (`ui.render`) and headless scenarios. The workflow (docs/ux/workflow.md): a step bar (Intent … Results) with each step's status from `project.status`, step panels with checks and their fixes, the dialogs docked there, the port-mode preview, mesh statistics and `project.estimate`, Run's preflight, stale results, Touchstone export and checks; the 3D view (`scene3d` builds the session's scene per overlay — materials, ports, walls and PML, mesh edges — `view3d` shows `emcraft-render`'s picture with orbit, pan, zoom, fit and standard views); File (new, open, open recent, samples, save, save as, history as script) and Edit (undo, redo); the start page (`home`: New project and Open, recent projects, the samples drawn by the 3D view, the settings new projects start with) and the preferences (`prefs`, a JSON file in the user's configuration directory); themes (`theme`: the window's tokens, the 3D view's colours and the pictures' palette of the current theme, built-in or the user's files in `<config dir>/themes`, docs/themes.md); projects as `.emcraft` files (`files`, docs/project-file.md) through the host's file dialog (`files::Picker`) or a path field. |
| testkit | `emcraft-testkit` | The verification oracle (`tests/oracle`), its runner and docs/oracle.md. |
| apps | `emcraft`, `emcraft-cli` | Desktop (with the control channel's loopback server and token, docs/control-protocol.md) and command line. |

Rules beyond the layer order: nothing below L7 uses egui, eframe, winit, wgpu or glow; the
testkit is a dev-dependency only.

## Shared building blocks

Before writing a helper, look here: these exist so that no crate writes them again.
`cargo xtask dupes` (exact copies) and `cargo xtask dupes --loose` (copies with names or scalar
types changed) find what slipped through.

| Where | What |
|---|---|
| `emcraft_macros::named_enum!`, `named_serde!`, `Named` | an enum named by text, its ids written once (views, tabs, units, axes, the choices command parameters take, backends): `ALL`, `IDS`, `id`, `label`, `parse` (aliases too), `Display`, its ids checked distinct when it compiles; `named_serde!` reads and writes it by id; `Named` is what generic code knows of one (`Params::pick`, `ParamSpec::one_of(&T::IDS)`) |
| `emcraft_model::units` | `Unit` (lengths and frequencies to and from SI), the display units |
| `emcraft_model::shape::Axis` | x, y, z with `ALL`, `index`, `from_index`, `name` |
| `emcraft_mesh::shared` | `DisjointSets` (root at the smaller index), `winding` of a closed surface |
| `emcraft_render::chart::Layout`, `plot::Layout` | a chart's or plot's area and the pixel ↔ axis maps |
| `emcraft_render::palette` | `Palette` (a picture's colours: ground, inks, grid, series, …) and `Pen` (a series' or arrows' colour by role, resolved when drawn); `Rgb::contrast_ratio` (WCAG) |
| `emcraft_engine::theme` | theme files (docs/themes.md): `Colour` (`#rrggbb[aa]`), `check`/`parse` with their caps, the built-in themes (`Base::theme`, `Base::palette`), `palette_named` for a command's `theme` |
| `emcraft_ui_egui::theme` | `Tokens::get()`, `scene()`, `palette()` of the current theme; `Themes` (built-in and user); colour names read from one table (`colour_names!`) |
| `emcraft_solver::sparse::Csr` | rows (`row`, `entries`), `Csr::builder`, `with_symmetric_pattern` |
| `emcraft_solver::dense` | `Scalar` (f64 and complex), `Cholesky<T>` with its triangular solves, `jacobi` |
| `emcraft_solver::subspace` | the eigen solvers' shared skeleton: `Coefficient`, `gradients`, `Deflation`, `start_vectors` |
| `emcraft_solver::driven` | `Slot` (an unknown or eliminated), `ModeTerm` and `read_s` (S from any port's terms) |
| `emcraft_fdtd` | `Axis`, `Cpml`, `invalid`, `to_u32`; `yee::Pulse`, `yee::Settling`; `Plan::validate`, `OpenPlan::validate` |
| `emcraft_engine::text` | `clip` (hostile text cut short), `unknown` (the "unknown X (there are …)" message) |
| `emcraft_engine::Step` | a script step `{command, params}` (the history, scripts, oracle recipes; `.emcraft` files hold them) |
| `emcraft_ui_egui::commands::UI_COMMANDS` | each command's button label, glyph and whether it runs long |
| `emcraft_ui_egui::Action`, `EmcraftApp::apply` | what a click, a key or a control request asks of the app (run, open a dialog, show a view, open a step, …): drawing pushes actions, `apply` performs them once the frame is drawn (the module docs of `lib.rs` tell what drawing may still edit in place) |
| `emcraft_ui_egui::UiExt`, `Handle`, `Fixed` | a control that publishes its handle (`tab:MODEL`, `toolbar:<id>`, `field:save:video`, …), shows its hint and asks for an action; the handles' names are typed, the fixed ones listed once |
| `emcraft_ui_egui` `viewport::ChartView`, `show_chart` | a result view around one chart: its key, controls, chart and what goes under it |

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
   polynomials) and cached for repeated shapes; K, M (with the complex permittivity
   εr(1 − j tan δ)) and, for walls of finite conductivity, the tangential wall mass matrix are
   assembled once; each frequency adds −k₀²M, the wall term (jωμ₀/Z_s)W and the port terms
   (κ_m/μr) v_m v_mᵀ/‖e_m‖² of every port mode. A numerically solved port's modes come from its
   cross-section at each frequency (`emcraft-solver::portmode`): real for lossless materials,
   the unit filling's with a complex γ for one lossy filling, hybrid lossy modes (complex εr and
   μr per triangle, complex γ and fields, `portmode::lossy`) for several materials one of which
   absorbs or disperses; their terms (γ_m/D_m) v_m v_mᵀ with complex v and the unconjugated D.
4. **Factorization.** Nested dissection, postorder, fundamental then relaxed supernodes, then
   one dense front per supernode: pivots chosen in blocks of 32 columns, each block updating the
   rest of the front at once on threads (deterministic).
5. **Solves.** Each port is excited by a unit TE₁₀ wave from the same factorization, with
   iterative refinement; S_pq is read by projecting E on TE₁₀ at port p. A terminal port
   (several strips) excites each of its quasi-TEM modes, and its modal S is converted to one
   entry per strip by the modes' conductor currents and voltages.
6. **Eigenmodes** (`emcraft-solver::eigen`). The guide closed into a cavity (ports left out,
   walls perfectly conducting), K x = k² M x by shift-invert subspace iteration on the same
   LDLᵀ, Rayleigh–Ritz with a dense Jacobi eigensolver; Q factors by perturbation from the walls'
   conductivity and the dielectrics' loss tangents.
7. **Adaptive refinement** (`mesh.adapt`). Passes of mesh, solve at one frequency and compare S
   with the last pass, until ΔS is below a tolerance: a solid's tetrahedra marked by the residual
   indicator (Dörfler) and the solid meshed again with them at half their size; a structured
   grid refined uniformly. Open problems adapt the same way, each pass solved as
   `solve.radiation` does: the layer and the point and line sources' tetrahedra are left out of
   the indicator (the layer keeps its mesh), a plane wave's incident field is the scattered
   field's data, and the passes compare the lumped ports' and feed's reflections or, with
   sources alone, the far field in 48 directions.
8. **Results.** Fields at any point from the element basis (H = −∇×E/(jωμ)); field plots sample
   a plane once and draw any component at any phase.
