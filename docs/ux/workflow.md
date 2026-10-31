# How an electromagnetic analysis runs, and the UI that follows it

This note sets out the steps of a frequency-domain (or time-domain) electromagnetic analysis as
engineers carry it out, from public tutorials and documentation of open and commercial tools
and from textbooks, and derives what EMCraft's desktop UI must do at each step. Behaviour is
our own; nothing here copies another product's screens, wording or icons.

Sources consulted (for the workflow only): the openEMS documentation and tutorials
(openems.readthedocs.io: simulation parameters → excitation → geometry and mesh → ports → mesh
finalisation → near-to-far box → run → post-processing; ports must sit on mesh lines), public
course notes and forum answers on adaptive meshing and convergence (the S-parameter change
between passes, "delta S", as the stopping test; meshing at the highest frequency of interest;
discrete versus interpolating sweeps), and general tutorials for waveguide, filter and antenna
modelling (geometry and materials → ports and boundaries → mesh → frequency range and solver →
run and check convergence → S-parameters, fields, far field → parameter studies). Common
pitfalls they name: skipping the mesh convergence check, and wrong boundary conditions.

## 1. The steps

| # | Step | What the engineer decides | What goes wrong |
|---|---|---|---|
| 0 | **Home** (the job's intent) | the kind of problem (guided-wave component, planar circuit, antenna, scatterer, resonator), units, **the frequency range** | a range that does not match the ports' cut-offs or the mesh |
| 1 | **Geometry** | shapes, Booleans, imported surfaces, layers and strips | non-closed or overlapping bodies, features flush with walls |
| 2 | **Materials** | εr, μr, loss tangent per body; wall conductivity | lossy material at a port; wrong units |
| 3 | **Excitation** | waveguide ports (and how many modes), lumped ports and their impedance, plane waves, dipoles; reference impedances | a port off its face, the first mode cut off in the band, too few modes near a discontinuity |
| 4 | **Boundaries** | conducting walls (perfect or finite σ), the open boundary (PML thickness, reflection), symmetry | the bodies touching the PML, the Huygens box crossing them |
| 5 | **Mesh** | element size (or elements per wavelength at the top frequency), grading at edges, element order | too coarse at the top of the band, singular edges unresolved |
| 6 | **Solver setup** | the solver (frequency domain, eigenmodes, radiation, time domain), the sweep (points, fast or full), the order | a sweep too coarse for a resonance |
| 7 | **Run** | — (watch progress, cancel) | runs that take too long without telling why |
| 8 | **Results** | S-parameters (dB, phase, Smith), fields on planes and surfaces, port modes and impedances, far field and RCS, Touchstone export | reading results of a model that changed since |
| 9 | **Verification** | convergence (order 1 → 2, a finer mesh), passivity and reciprocity, comparison with a closed form or a benchmark | trusting one unconverged run |
| 10 | **Iteration** | parameter sweeps, optimisation | rebuilding by hand |

EMCraft's strength is step 9: every solver capability is checked against theory in the oracle,
and the UI should make the same habits easy for the user's own models.

## 2. UX principles

1. **The workflow is visible.** A step bar shows steps 0–8 in order, each with a status of its
   own look, told apart by shape as well as colour, each mark meaning one thing so none is
   learned to be ignored:

   | Status | Look | Means |
   |---|---|---|
   | done | green disc, tick | nothing to do |
   | to do | hollow ring | not done yet, nothing wrong (a new project's geometry, a run not made) |
   | waiting | grey disc, dots; the name dimmed | waits on an earlier step (materials, boundaries and mesh on the geometry; results on a run) |
   | warning | amber disc, ! | worth a look now (a coarse mesh, a band starting below a cut-off); Run may accept it |
   | needs attention | red disc, × | something is wrong or blocks Run: a step a refused Run needed, a run that failed |
   | out of date | blue disc, ↻ | results older than the model |
   | not needed | grey ring, dash; the name dimmed | a step the chosen kind of analysis does without (a resonator's excitation, the time domain's mesh) |

   Run turns (an arc) while a run goes on. A to-do that only matters before a run (the band)
   ends with the run. Clicking a step opens its panel and puts the 3D view in that step's mode.
   Users can work in any order; the bar only tells them what is left.
2. **One model, one 3D view.** The geometry is always on screen; each step overlays what it is
   about: materials coloured (2), ports and their faces highlighted with their modes'
   arrows (3), walls and the PML shaded (4), the mesh's edges and its size (5), fields and
   patterns on the model (8). Standard views, orbit, pan, zoom, fit.
3. **The frequency range is set once, early,** and drives defaults: the mesh size by elements
   per wavelength at its top, the sweep, the port-mode preview frequency.
4. **A project opens on its Home** (step 0, the viewport's first tab, "Home"): what the job is
   for, said in plain words rather than as an "intent". Four large cards, one per kind of
   analysis, each with a small picture of what it gives (an S-parameter chart, resonance peaks,
   a radiation pattern, a pulse and its echo) and a line saying it; the band's start and stop
   with Apply and, for a rectangular port, its guide's standard band as a preset (WR-90:
   8.2–12.4 GHz; a guide of no standard width, or a filled one, its TE10 band, 1.25–1.9 × the
   cut-off; `emcraft_engine::guides`); the project's units; and the project at a glance —
   geometry, excitation, mesh and results, each with its step's mark (a click opens the step) —
   with the next step's button. Each choice runs the same command as elsewhere
   (`project.analysis`, `project.frequency`, `units.set`). A template, a new project or one
   opened without results lands here; one with results opens on them. Home takes the whole
   viewport: the step panel shows beside it only for a dialog docked there.
5. **See before solving.** Port modes (patterns, β, ε_eff, impedance, cut-offs) show as soon as
   a port and a mesh exist (`solve.port_modes`, seconds); the mesh's size, its element count and
   an estimate of the solve's unknowns, memory and time show before running.
6. **Preflight.** Run checks the model first and lists every problem with the step that fixes
   it (a link), refusing to start on blocking ones; warnings can be accepted.
7. **Never block.** Runs go to the background with progress (stage, frequency, solves done)
   and Cancel; the UI stays live. A finished run is kept with the model state it solved, and
   results of a changed model are marked stale, not thrown away.
8. **Everything is a command.** Each action is an engine command, recorded in the session's
   history; the history saves as a script that reproduces the model and its runs (the same
   scripts the CLI and the oracle run). Undo and redo step through it.
9. **Verification is one click.** Results offer: rerun at order 1 (or a finer mesh) and
   compare, show |S11|² + |S21|² and S12 − S21, compare with the closed form where one exists.
10. **Units everywhere.** Every field shows its unit; the session's display units (mm, GHz) are
   switchable.
11. **Hostile input is refused field by field,** with the reason next to the field; nothing
   panics (the engine's rule).

## 3. Screen layout

```
┌──────────────────────────────────────────────────────────────────────────────────────┐
│ EMCraft   File  Edit  View  Help                              [search commands  /]   │
│ [0 Home ✓]─[1 Geometry ✓]─  [2 Materials ✓]─[3 Excitation !]─[4 Boundaries ✓]─      │
│ [5 Mesh ✓]─[6 Setup ✓]─[7 Run ▶]─[8 Results ⟳ stale]                                │
├──────────────┬───────────────────────────────────────────────────┬───────────────────┤
│ Browser      │                                                   │ Step panel        │
│  Geometry    │                 3D view                           │  (the active      │
│  Materials   │      (the active step's overlay; view cube,       │   step's settings │
│  Ports       │       standard views, fit)                        │   and checks;     │
│  Boundaries  │                                                   │   dialogs dock    │
│  Mesh        │                                                   │   here)           │
│  Results     │                                                   │                   │
├──────────────┴───────────────────────────────────────────────────┴───────────────────┤
│ Messages · progress · timings                                         [Cancel]       │
└──────────────────────────────────────────────────────────────────────────────────────┘
```

The ribbon of phase 1 stays as the way to every command; the step bar is the way through a job,
and the ribbon's tabs follow it (MODEL, MATERIALS, PORTS & SOURCES, BOUNDARIES, MESH, SIMULATE,
RESULTS). A result view of a model changed after its run carries a band — ↻ "Out of date" (why
on hover) and Re-run — its picture dimmed; the browser marks those results too. A running job
shows its count ("Frequency sweep · 3/11 · 4 s") in the messages, at the top of the open step's
panel (or where the docked dialog that started it was) with Cancel, and left of the settings in
the app bar; the right column stays put while it runs.
What was built is changed where it is listed: a browser row of a material, a port, a body or a
source has a right-click menu (Edit, Delete, Show in 3D); a double-click edits, Delete deletes the
selected row's item (undoable; a refusal is an error message). Every dialog of something the
model already has opens on its values (`project.values`), and OK with them unchanged runs
nothing. An opened project's Setup step shows its last run (`project.runs`); while the fields
stay so, Run and Re-run repeat that run whole (`solve.rerun`).
Work is not lost: closing the window with unsaved changes asks Save / Don't save / Cancel (with a
job running, whether to stop it first); unsaved changes are copied aside every minute and
offered on the start page after a crash (Recover, or dismiss).
The 3D view cuts the model open: a section across x, y or z at a position along it, either side
kept, the cut faces filled in each body's colour.
Each control is in one place: opening a step switches the ribbon to that step's tab (its
commands), so the step panel holds only the step's checks, its own settings (the band, the
sweep, Run) and the dialog docked there; the views over the viewport are switched by its view
tabs, which list only views with something to show; files and samples are in the File menu and
on the start page; the 3D view's tools are icons (fit, home, face the field's plane, projection)
and a view cube (or the six signed axes, Settings ▸ 3D view ▸ Navigator).

**Navigating the 3D view** works as in SolidCraft, so the sibling apps feel alike
(`crates/ui-egui/src/navigation.rs`, `view_cube.rs`; scenario `navigation`):

| Input | Default mapping (Fusion) |
|---|---|
| Right drag, Shift+middle drag, Ctrl+left drag (every mapping) | orbit about the view's centre (Settings ▸ 3D view ▸ Orbit around: origin, or the point under the cursor; over empty space the middle of the model) |
| Middle drag, Space+left drag, two-finger scroll | pan |
| Wheel, pinch, Ctrl+wheel | zoom toward the point under the cursor (e^(−0.0015) a point scrolled) |
| Shift+two-finger scroll | orbit |
| Shift while orbiting | snap to the nearest of the cube's 26 views |
| Double-click empty space / middle double-click | fit / home (the SolidWorks mapping: fit) |
| Arrows (Shift) | turn 15° (90°), animated; held, 90°/s (180°/s) |
| Ctrl+arrows | pan a tenth of the view |
| Ctrl+1…7 | front, back, left, right, top, bottom, isometric |
| View cube: face, corner / arrows / Home, Fit / drag | look from it / quarter turns / home, fit / orbit (Shift snaps) |
| Click / Shift+click or Ctrl+click | select the part under the pointer (on nothing: clear) / add or take it out |
| Left drag left to right / right to left (Shift: add) | box select by window (the parts wholly inside) / by crossing (the parts it touches), as SolidCraft |
| Escape / Delete | clear the selection (a tool on: its picks, then the tool) / delete the selected items (each by its command, each undoable) |

The other mappings (SolidWorks, Inventor, Alias, Maya, Blender, Tinkercad, OpenSCAD, HFSS,
Touchpad) bind the buttons as SolidCraft's do. A plain left press on the field's plane or a
port's reference-plane grip drags them, as SolidCraft's in-view handles take a plain press; a
mapping's left-button navigation applies on empty space or with its keys held; a left drag no
mapping binds draws the selection box. Several parts selected are highlighted in 3D and in the
browser (Shift or Ctrl on a row adds or takes out its part), the card at the top says how many
and deletes the items among them; Edit is for one. View changes
(standard views, fit, home, face the plane, the cube) animate for 0.5 s, up to 1 s for turns past
a quarter turn, eased, the short way round, stepped by the frame's clock (the scenarios' clock is
their frames', and `Harness::settle` waits for the end).

## 4. What the engine provides for it

- `project.status`: per step, its status (`done`, `todo`, `waiting`, `warning`, `missing`, `stale`;
  the window adds what it knows: a refused Run marks the steps in its way `missing`, a failed
  run marks Run) and messages,
  each naming the command or field that fixes it. Computed from the model, the mesh and the
  results; cheap (no solve).
- `project.frequency`: the frequency range (start, stop) of the job, stored in the model, the
  default for meshing by wavelength, sweeps and port-mode previews.
- `project.analysis`: the kind of analysis chosen at the start (`frequency`, `eigenmodes`,
  `radiation`, `time_domain`), stored in the model; inferred from it until one is chosen. The
  checks follow it: a resonator needs no excitation (its ports are a warning), radiation wants
  the open boundary, a driven kind wants its excitation before Run. Home (step 0) shows the four as
  cards; Setup starts on the chosen kind's solver, SIMULATE puts its solve first and the steps a
  kind does not need are dimmed.
- `project.estimate`: from the mesh and the order, the unknowns, the factor's memory and a time
  estimate (from the recorded timings of earlier runs where available).
- `file.save` / `file.open`: the project as JSON (the model with its settings, and the command
  history as a script).
- `edit.undo` / `edit.redo`: through the history (session snapshots).
- `results.touchstone`: the sweep's S-parameters as a Touchstone file (`.sNp`, RI or MA, the
  reference impedances).
- `results.check`: passivity (largest eigenvalue of SᴴS), reciprocity (max |S − Sᵀ|), and the
  comparison with a closed form when one applies.

## 5. Phase 2 of the UI

1. The step bar and step panels, driven by `project.status`, with links to fixes.
2. The 3D view: a CPU renderer of triangles and lines with a depth buffer in `emcraft-render`
   (so the same pictures render headless for scenario tests and `ui.render`, deterministically),
   shown as a texture; orbit, pan, zoom, fit, standard views; per-step overlays (materials,
   ports, walls/PML, mesh edges, fields on planes). A GPU path may replace the texture later
   without changing what is drawn.
3. Port-mode preview in the Excitation step; mesh statistics and the estimate in the Mesh step.
4. Preflight before Run; stale results; the run's timings in the log.
5. File: save/open, history script export; Edit: undo/redo.
6. Results: Touchstone export, the verification checks.
