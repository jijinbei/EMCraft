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
| L0 | `emcraft-model` | Solver-neutral model: SI units and the display conversions (`units`), materials, the bodies (`body`: one list in the order placed, each a shape with what it is made of and how the region takes it; `MeshView` the features and inclusions the mesher reads), the domain (`domain`: the region, its filling, outer walls or open boundary, symmetry, periodic and rotational planes; ADR 0006), the waveguide geometry (sections with their own aperture across x; held as bodies, its box extruded along z with its slabs and cells, read back by `Waveguide::of_bodies`), a printed circuit board (a body of its own shape, `Shape::Board`), ports (on z ends or on faces across x, y or z, each in its own axes: `port::Frame`), prisms (`prism`: a polygon extruded, caps by ear clipping), the tetrahedral mesh (materials per tetrahedron, boundary faces tagged PEC or port, the structured grid for point location), the model check. |
| L0 | `emcraft-tetmesh` | The unstructured tetrahedral mesher, ported from FieldCraft's: exact and adaptive predicates, boundary-surface preparation and checks, Bowyer–Watson Delaunay, constrained boundary recovery (Diazzi et al. 2023), the quality stage (graded size field, Delaunay refinement, ODT smoothing, Klingner–Shewchuk improvement, snapping to exact surfaces), element quality. No workspace dependencies (a small compatibility module stands in for what it took from FieldCraft's model; its own `Point3` and `Vec3`, converted to the model's in `emcraft-mesh`). |
| L0 | `emcraft-i18n` | Procedural macros checking the window's texts against the translations when a crate compiles (`text!`, `template!`: each English text in every complete `locales/*.json` with the same placeholders), expanding to the literal; no workspace dependencies (docs/languages.md). |
| L0 | `emcraft-macros` | Declarative macros any crate may use, no workspace dependencies (the model, also L0, comes after it): `named_enum!` (an enum whose variants each have a text id, aliases and a label), `named_serde!` (serde by id), the `Named` trait they implement. |
| L1 | `emcraft-mesh` | How a model is meshed (`Plan`: a board's region, a guide, a general region); one extrusion mesher (`extrude`: a cross-section, a structured grid or a triangulation, extruded through levels along z, each cell filled or left out), under the board's stack (`planar::extrude`) and the structured tetrahedral meshes of waveguides (each grid cell split into six tetrahedra about its diagonal, conforming by construction; section boundaries and aperture walls are grid planes; cells outside an aperture left out; lines graded geometrically towards the re-entrant edges of steps and irises), global edge and face numbering with ascending nodes (`Topology`), point location (grid or buckets); primitive bodies (box, cylinder, sphere) tessellated with exact surfaces and meshed by `emcraft-tetmesh` (`solid`).; unstructured meshes of primitive bodies through `emcraft-tetmesh`, with inclusions as regions joined node for node, Boolean features (`csg`: a mesh Boolean on exact predicates) and edges bent onto curved surfaces; adaptive refinement (`adapt`: Dörfler marking, the marked tetrahedra as graded local sizes of a new mesh, `solid::solid_with` with `SolidOptions::refine`); imported bodies' faces flush with the region's moved off them (`flush`, primitives in imported regions too), unions sharing a plane with the region's faces grown past it, united and cut back (`clip`), grid lines of tessellated bodies kept off other bodies' planes (`solid::grid`), gaps under a tenth of the element size filled with elements as small as the gap within the tetrahedron limit (`solid::narrow`), the points a Boolean leaves inside straight creases collapsed (`crease`), the air a part's metal bodies enclose (`cavity`); every mesh checked for degenerate or inverted tetrahedra (`check_tetrahedra`) |
| L1 | `emcraft-fem` | Nédélec elements of the first kind, orders 1 (Whitney, 6 functions) and 2 (Webb's hierarchical basis, 20 functions), as polynomials in barycentric coordinates whose products integrate exactly; curved (quadratic, covariant Piola) elements integrated by quadrature; Gauss–Legendre and collapsed simplex quadrature; Bessel functions; rectangular and circular waveguide modes (TE/TM fields, cut-offs, propagation constants, modal port factors). |
| L1 | `emcraft-theory` | References: the Huygens-surface near-to-far transform both solvers share (`radiation::Huygens`); the TE₁₀ wave through a guide filled in sections (closed-form transmission-line cascade), S-parameters and fields; Bessel-function zeros and the resonances of cylindrical and spherical cavities; mode matching through H-plane steps and inductive irises (TE_m0 modes) and E-plane steps and capacitive irises (LSE^x_1n modes), generalised scattering matrices, Redheffer star products; the full-height post across a guide by a Nyström method on its 2D integral equation (`post`); square-cornered H-plane and E-plane bends by field superposition (`bend`); plane waves through layer stacks at oblique incidence (TE/TM) and Bloch waves of periodically loaded guides (`floquet`). |
| L1 | `emcraft-io` | Geometry import: closed triangulated surfaces from STL (binary, ASCII) and 3MF packages (components flattened, model parts followed), in metres, grouped into faces by the angle between neighbouring triangles; each face's exact surface (plane, cylinder, sphere, cone, torus) recovered by least-squares fits to its vertices (`fit`), so the mesher and the curved elements follow imported curved faces; untrusted files capped and checked. SolidCraft designs (`.sc`, `.solidcraft`) through `solidcraft-exchange` at a pinned commit ([ADR 0003](adr/0003-solidcraft-exchange.md)): their parameters set before they are evaluated, each face's exact surface from the design. |
| L1 | `emcraft-templates` | The templates, all in one place: the sample models' command scripts (`scripts/*.json`), whose dimensions are project variables, their list (`TEMPLATES`: name, label, script), and the geometry builders the scripts call when a dimension is a count (`iris_sections`, behind `geometry.iris_filter`), and the SolidCraft designs templates are built from, compiled in (`DESIGNS`, opened by `geometry.import` `template:<file>` through `design`, no file read). `file.sample` and the start page open them. |
| L1 | `emcraft-render` | Pictures without a GPU: canvas with anti-aliased lines, arrows and text (DejaVu Sans through `ab_glyph`, `text`; a bitmap font as fallback); colour maps; the colours of pictures by role (`palette`: a `Palette` per theme, `Pen`s resolved when drawn — the palettes themselves come from theme files); field plots (colour map + arrows + colour bar + legend); line charts; the 3D view's scene without drawing it (`view3d`: triangles with colour, opacity and tag, lines, an orbit camera, perspective or orthographic, picking by ray through a bounding-volume hierarchy, view changes, a coarser copy for moving pictures, deterministic; `view3d::section`: a scene cut by a plane, closed bodies capped — `emcraft-gpu3d` draws it); PNG. |
| L2 | `emcraft-fdtd` | The time-domain solver: finite integration (Yee) on the tensor grid of the waveguide's aperture and section lines, CPML along z, a TE10 pulse in feeds beyond the ports, S-parameters by Fourier transforms of the TE10 amplitudes on the end faces against a uniform-guide reference run; z-slabs on threads that live for the run (atomics and barriers, no `unsafe`, results independent of the thread count). Modules: `yee` (the staggered fields, CPML grading, pulse, running transforms and the slab-threaded leapfrog both runs share), `guide` (the waveguide run), `dispersion` (the slots of dispersive and lossy edges: ADE poles, conductivity by the exponential update, the walls' branches of surface current; magnetic losses' decay), `surface` (walls of finite conductivity: Leontovich's admittance as a sum of first-order branches), `open` (open problems: a box lined with CPML on all faces, lossy, dispersive and magnetic bodies, lumped ports, dipoles, a Huygens box). Loss tangents become conductivities matched at the band's centre (`Losses`), ports in lossy and dispersive media take the complex TE10 γ. |
| L2 | `emcraft-gpu` | The solvers on a GPU or the CPU's SIMD through CubeCL ([ADR 0002](adr/0002-gpu.md)): `emcraft-fdtd`'s `Plan` (the guide, with dispersive edges) and `OpenPlan` (the open box: CPML on every face, lumped ports, current elements, the Huygens box's transforms) stepped by kernels in single or double precision, the projections and ports read back a batch at a time; `Fronts`, an `emcraft_solver::accel::Accelerator` that holds a factorization's large fronts and runs their interchanges and block updates in complex double. Modules: `device` (backends, precisions, the panic guard), `buffer`, `guide`, `open`, `front`, `kernels/{guide,open,front}`. Backends: CUDA (the `cuda` feature) and CubeCL's CPU runtime (the `cubecl-cpu` feature, on in the tests, off in the apps). |
| L2 | `emcraft-solver` | Complex sparse assembly; coordinate nested dissection; multifrontal complex symmetric LDLᵀ (Bunch–Kaufman pivots within supernodes, static pivoting fallback, relaxed supernode amalgamation, blocked fronts updated on threads); iterative refinement with the backward error; the driven waveguide analysis (`Driven`): PEC elimination, modal ports, power-normalised S-parameters, E and H at points, L² errors, absorbed powers; walls of finite conductivity (Leontovich); cavity eigenmodes and Q factors (`eigen`); a residual a-posteriori error indicator per tetrahedron (`adapt`); terminal ports, a cross-section of several strips read per strip by its modes' conductor currents and voltages (`driven::terminals`); periodic cells: the master/slave reduction Tᴴ A T of phase-shifted faces (`driven::periodic`), Floquet ports (`driven::floquet`), a multifrontal LU for the reduced unsymmetric systems (`lu`, on the LDLᵀ's analysis and multifrontal driver, `multifrontal`) and dispersion diagrams by Hermitian shift-invert subspace iteration (`bloch`). |
| L6 | `emcraft-engine` | Commands: the table in `cmd/table.rs` (`CommandSpec`: id, label, documented parameters with their kind, unit and choices, effect on the job, run), the commands themselves by area (`geometry`, `meshing`, `solve`, `results`, `open`, `project`), their parameters read through `Params` (checked, converted to SI at the boundary, expressions of the project's variables evaluated by `expr`); parameter studies (`study`: variables, sweeps and Nelder–Mead optimization on models rebuilt from the history); `Session::execute` with rollback and a panic guard, the session's `Results`, scripts, opening a template (`emcraft-templates`), the workflow's steps and statuses (`workflow`), pictures by subject (`view`: field planes and their samples, port modes, the references S-parameters are compared with, S-parameter, TDR, dispersion, pattern and study charts, and the `view.*` commands, in a theme's colours); how results are looked at, saved with the project (`views`: each chart's style — axes, scales, labels, curves' colours and widths, grid, legend — applied to the window's charts and the PNGs alike by `style_chart`; the named field probes); the run's time before it starts (`estimate`: `project.estimate` of the run as set up, from this machine's last sweep or a reference machine's); themes as files (`theme`: the format of docs/themes.md, the built-in `themes/*.json` compiled in, `theme.check`); progress and cancellation of long commands (`progress`); the analysis history (`archive`: every solve kept with its model and results, and as the project file keeps it — `archive::kept`, what a solve found as shown and compared, `archive::file`, the file's form and its checks). |
| L6 | `emcraft-ui-core` | What the window decides, whatever draws it (after the engine; no iced, winit, wgpu or rfd below it): the languages and the texts looked up by their English (`lang`, `t!`), the charts' words (`chart_words`), the dialogs' parameter labels (`param_labels`), the window's names for what it shows — glyphs, pages and settings sections, the workflow as the window shows it (`steps`: a check's words and fix, the Setup form and its check into `project.setup`, the statuses with what the window knows on top, the preflight, the devices, the band's diagram, the kinds' cards), the 3D view's tools — what the pointer snaps to on the face under it, where a port goes, what a click asks for, the reference plane's drag (`tools`) — the views and where each shows, the handles scenarios and the control channel find things by (`glyph`, `page`, `tools`, `view`, `handle`), every command for agents (`commands::catalogue`) and how scenarios check what a window shows (`scenario`), what a frame asks of the window — every action a click, a key or a request makes, and what they carry: a pick in the 3D view, a selection click, the window's layers, the file actions, a browser item and the browser's rows grouped under the workflow's steps with what each step adds and runs and which commands a kind of analysis offers (`outline`, docs/adr/0009-model-tree-window.md), the board's pen, the navigation choices (`action`, `pick`, `selection`, `layer`, `files`, `browser`, `outline`, `pen`, `navigation`, `prefs`, `steps`; the window performs them), points and rectangles on screen (`geom`, converted at the toolkit's edge); the status line's text and the message log (`status`, `messages`), the font that shows the language (`fonts`), crash reports and recordings (`crash`, `record`), and a system's chain as the window shows it (`system`: its stages' kinds and numbers, the band's bar as `project.setup`'s settings, the canvas' `layout` from the model alone and the pointer's `gesture`s as a pure state machine ending in commands). What the 3D view draws (`scene3d`: the scene built from the session for one overlay — the model meshed or not, the parts' walls, ports, sources, the open boundary's layer, the field's planes, the legend — a function of the session, the overlay and the theme's colours, each part a layer of its own merged in order). Its rules are pure and tested without a window. |
| L7 | `emcraft-gpu3d` | The 3D view drawn by the GPU (wgpu) with a depth buffer, below the window (docs/adr/0004-gpu-viewport.md): the scene packed for the GPU, the passes and WGSL shaders, picking by the tags under a pixel, the field's colour map from its phasors, a device of the process's own read back into pictures (`headless`). A window draws it on its own device through `Renderer::prepare` and `Renderer::show` in its render pass (iced's shader widget, `emcraft-ui-iced::view3d::shader`). |
| L6 | `emcraft-control` | The control channel the window serves (docs/control-protocol.md): a request and where its reply goes (`ControlRequest`), the loopback JSON-lines server and its per-launch token (`server`, `auth`). No GUI toolkit. |
| L7 | `emcraft-picker` | The system's file dialog (rfd) for the window: projects to open and save, parts to import, each asked on a thread of its own (`emcraft_ui_core::files::Picker`). |
| L7 | `emcraft-ui-iced` | The desktop window, in iced (docs/adr/0007-iced-window.md, [ADR 0008](adr/0008-retire-egui.md)): its state, its messages and the view drawn from them (`app`), every change an engine command and every rule from `emcraft-ui-core`; the workflow (`browser`: the model tree at the window's left, the kind of analysis at its head and a node a step with its status, its ＋ and ▶, docs/adr/0009-model-tree-window.md; `steps`: the statuses and opening a step; `step_panel`: the open step's panel — checks with their fixes, the Setup form over the band's diagram, the device, Run's preflight, the results' checks and export), its pages — the model in 3D (iced's shader widget drawing `gpu3d`), the S-parameters, eigenmodes, port modes, analyses and the PCB diagnosis, a system's node editor (`system`), the start and settings pages — the browser, the command search (`search`), the dialogs (in the properties panel at the right on the project's pages), files and autosave, charts drawn by the GPU from `emcraft-render`'s shapes (`chart`), the message area (`messages`), Escape's layers (`escape`), recording, replay and crash reports (`record`, `reports`), the language's font (`language`), the control channel's methods (`control`), the window drawn without one (`headless`: `ui.render` on the CPU, the 3D view offscreen) and headless scenarios through iced's simulator (`scenario`, `tests/scenarios/`). |
| testkit | `emcraft-testkit` | The verification oracle (`tests/oracle`), its runner and docs/oracle.md. |
| apps | `emcraft`, `emcraft-cli` | Desktop (the iced window, with the control channel's loopback server and token, docs/control-protocol.md) and command line. |

Rules beyond the layer order: nothing below L7 uses iced, winit, wgpu or rfd; the
testkit is a dev-dependency only.

## Shared building blocks

Before writing a helper, look here: these exist so that no crate writes them again.
`cargo xtask dupes` (exact copies) and `cargo xtask dupes --loose` (copies with names or scalar
types changed) find what slipped through.

A function takes at most six arguments, `self` included (`clippy.toml`). Before adding a
seventh, see whether one is not needed (the callee can read it: the app, its tokens), whether some travel together as a type of their own (`Caps`, `Shell`, `BoxGrid`, a
coefficient set holding its `dt`), whether an out-parameter is the return value, and whether the
function is really a method of that type or a widget (an iced `Program` or `Widget`). The GPU kernels and
the fits' closures are allowed where they are written.

| Where | What |
|---|---|
| `emcraft_macros::named_enum!`, `named_serde!`, `Named` | an enum named by text, its ids written once (views, tabs, units, axes, the choices command parameters take, backends): `ALL`, `IDS`, `id`, `label`, `parse` (aliases too), `Display`, its ids checked distinct when it compiles; `named_serde!` reads and writes it by id; `Named` is what generic code knows of one (`Params::pick`, `ParamSpec::one_of(&T::IDS)`) |
| `emcraft_model::units` | `Unit` (lengths and frequencies to and from SI), the display units |
| `emcraft_model::quantity` | checked numbers, parsed once where they enter and stored as their type: `Positive`, `Hertz`, `Metres`, `Ohms`, `Conductivity` (`MAX_CONDUCTIVITY`), `LossTangent`; `new`, `named`, `TryFrom<f64>`, `get`/`Deref`, arithmetic giving `f64`, serialized as plain numbers; read from command parameters by `Params::checked`, `hertz`, `metres`, `edit_metres`. A box's sides: `shape::BoxSize` (a checked `Vec3`) |
| `emcraft_model::shape::Axis` | x, y, z with `ALL`, `index`, `from_index`, `name` |
| `emcraft_model::port::Frame` | a port face's own axes (u, v across it in the order x, y, z; w = u × v): `local`, `global`, `global_vec`, `global_cvec`, `axes`, `across` |
| `emcraft_model::measure` | `measure(Mark, Mark)`: distance, Δ, the gap and angle between picked points, planes and lines (`geometry.measure`) |
| `emcraft_mesh::shared` | `DisjointSets` (root at the smaller index), `winding` of a closed surface |
| `emcraft_render::axes::Layout` (`chart::layout`, `plot::layout_of`) | a chart's or field plot's area and the pixel ↔ axis maps (linear or logarithmic), one type for both |
| `emcraft_engine::views::style_chart` | a chart in the project's style (`view.chart.style`), for the window and the PNGs alike |
| `emcraft_render::view3d` | `Scene` (`tri`, `quad`, `cuboid`, `line`, `tagged`, `bounds`), `render` (CPU, depth buffer), `flatten` (painter's order for the GPU), `pick`, `hits` (the triangle under a pixel and where it was hit); `section::section` with `Section` (`across`, `new`, `flipped`, `keeps`): a section view as a scene |
| `emcraft_render::palette` | `Palette` (a picture's colours: ground, inks, grid, series, …) and `Pen` (a series' or arrows' colour by role, resolved when drawn); `Rgb::contrast_ratio` (WCAG) |
| `emcraft_engine::theme` | theme files (docs/themes.md): `Colour` (`#rrggbb[aa]`), `check`/`parse` with their caps, the built-in themes (`Base::theme`, `Base::palette`), `palette_named` for a command's `theme` |
| `emcraft_ui_core::theme`, `emcraft_ui_iced::look` | the window's tokens, the 3D view's colours and the pictures' palette of a theme; `Themes` (built-in and user); iced's styles as functions of the tokens |
| `emcraft_solver::sparse::Csr` | rows (`row`, `entries`), `Csr::builder`, `with_symmetric_pattern` |
| `emcraft_solver::dense` | `Scalar` (f64 and complex), `Cholesky<T>` with its triangular solves, `jacobi` |
| `emcraft_solver::subspace` | the eigen solvers' shared skeleton: `Coefficient`, `gradients`, `Deflation`, `start_vectors` |
| `emcraft_solver::driven` | `Slot` (an unknown or eliminated), `ModeTerm` and `read_s` (S from any port's terms) |
| `emcraft_fdtd` | `Axis`, `Cpml`, `invalid`, `to_u32`; `yee::Pulse`, `yee::Settling`; `Plan::validate`, `OpenPlan::validate` |
| `emcraft_engine::text` | `clip` (hostile text cut short), `unknown` (the "unknown X (there are …)" message) |
| `emcraft_engine::Step` | a script step `{command, params}` (the history, scripts, oracle recipes; `.emcraft` files hold them) |
| `emcraft_ui_core::commands::UI_COMMANDS` | each command's button label, glyph and whether it runs long |
| `emcraft_ui_iced::app::Message`, `App::update` | what a click, a key or a control request asks of the window: each message changes the state, the view is drawn from it; every change to the project is a command |
| `emcraft_ui_iced::control::handles`, `inspect` | the window's handles (`tab:model`, `command:<id>`, `dialog:ok`, …) with the message each sends, and what it shows, for agents and scenarios |
| `emcraft_ui_core::results`, `emcraft_ui_iced::chart` | a result page's choices, tables and chart (the engine's), drawn by the GPU from `emcraft-render`'s shapes |

## The solver pipeline

The PCB workspace is a separate screening/analytical pipeline, documented in [pcb.md](pcb.md):
`model::pcb::Board` stores checked SI routing, stackup and filled copper; `io::pcb` reads the
published KiCad format; `theory::pcb` screens reference geometry and builds an explicitly ideal
microstrip cascade using the existing `microstrip` and `coupled` references. `engine::pcb`
owns the import/load/inspect/sample commands, embedded history and replies, reusing the engine's
TDR transform. `ui-iced::pcb` draws routing and displays those replies. It does
not send incomplete PCB data through the volume FEM solver. `model::units::Time` converts the
SI delays at the UI boundary. The PCB data/diagnosis commands and `view.pcb` navigation are shared by UI, control, scripts and CLI.
`Analysis::Pcb` declares or infers this workflow; three templates live in `emcraft-templates`
beside the other project scripts. Their `pcb.sample` steps retain parameter expressions in
history, while file imports embed checked board data. `render::pcb` shares even-odd copper
spans between the interactive view and bounded CPU top-view previews (render depends on the
L0 model for the board schema). Worker-driven file opens choose their automatic landing view
after the loaded model is known, while preserving a user's view change made during the job.

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

UI machine choices use explicit lifecycle states: compute selection is Pending until Setup recommends a backend, Automatic afterwards, and Explicit after a user device command; opening a project does not reset it. View restoration and pointer gestures dispatch on result/interaction enum states through exhaustive matches. Input validation and geometric predicates remain guards rather than being cast into artificial UI states.

PCB workflow navigation is `engine::pcb::Stage`, stored in `Views::pcb_stage`. `view.pcb` validates stage/net changes without calculating a report; `pcb.inspect` advances to Results. The UI reuses the existing workflow status marks for the PCB bar, and displays stage-specific controls plus a schematic layer-stack section.

PCB coupling uses `theory::pcb::pairs` for bounded, non-numerical parallel-overlap geometry. Engine replies expose typed pair endpoints/ids and units; `Views::pcb_illustration` saves role and display choices. `viewport::pcb_coupling` supplies copper/dielectric geometry and conceptual curve descriptors to the existing GPU renderer. WGSL generates tubular arches/loops/reference lines and their smooth normals in both colour and depth passes. These are explanatory annotations, never invented field samples. The initial view magnifies a local section; ordinary callouts attach net/layer names to the projected conductors and reference plane. Geometry comparison snapshots are temporary UI state.
