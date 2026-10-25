# EMCraft — instructions for agents

EMCraft is a clean-room, open-source, pure-Rust electromagnetic field simulator: it models
microwave components, solves Maxwell's equations with its own finite element solvers and shows
S-parameters and fields. It targets the work done today in CST Studio Suite, HFSS and similar
tools, and aims to beat them on openness, verification and agent control.

Siblings with the same conventions: `../fieldcraft` (CAE pre-processor and structural solvers)
and `../solidcraft` (parametric CAD). Code adapted from them (MIT OR Apache-2.0) is noted in
ATTRIBUTION.md.

## Product name
EMCraft everywhere: crates `emcraft-*`, binaries `emcraft` and `emcraft-cli`, `EMCRAFT_*`
environment variables, docs and CI.

## Start every session here
1. Read `ROADMAP.md` (milestones, what is next) and your task.
2. `docs/architecture.md`: crates, layers, the solver pipeline.
3. `docs/adr/`: library decisions (what we may depend on).
4. Solver work: `docs/solvers/literature.md` (the papers each method comes from) and
   `docs/verification.md`.

## Never crash
People trust EMCraft with their models; a crash loses their work. **This outranks feature work.**
- **No panics in non-test code:** no `unwrap()`, `expect()`, `panic!`, `unreachable!`, `todo!`,
  `unimplemented!`; no `unsafe` (`unsafe_code = "forbid"`).
- **Errors are `Result<T, E>`** through the crate's error type and `?`. An unfinished feature
  returns an error ("not supported yet: …"); it never panics.
- **Input-derived numbers are hostile** (command parameters, scripts, files): finite checks, caps
  on sizes (mesh tetrahedra, unknowns, frontal matrices, frequencies, picture sides).
- **Bound every iterative algorithm** (iterative refinement, searches) with explicit caps.
- **Last-resort guard:** `Session::execute` runs every command under `catch_unwind`; an error or
  an escaped panic restores the session and reports `EngineError`.
- **Prove it:** every crash fix lands with a regression test (see
  `hostile_parameters_are_refused_and_nothing_panics`).
- Every production crate root carries
  `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]`.

## Non-negotiables
- **Independent.** Behaviour is designed from public documentation, published papers, textbooks
  and user feedback. Never copy another product's code, icons, help text, templates or examples.
  Agents must not drive, script or extract data from commercial EM tools (CST, HFSS, FEKO,
  COMSOL, …): no test data, results or files produced with them.
- **Verification (docs/verification.md).** Every solver capability is checked against
  closed-form solutions (theory), published benchmarks and measurements, each case in
  `tests/oracle/` tagged with its source. A passing result is frozen as a fixture
  (`fixture.json`), so any later change that moves it fails CI. Reference values are computed
  independently of the code under test (the closed form, evaluated separately).
- **Algorithms come from papers**, cited in the module docs and `docs/solvers/literature.md`.
- **Never read, copy or link GPL/LGPL/AGPL code** (e.g. Elmer, openEMS, MEEP, GetDP, FreeFEM,
  NGSolve, deal.II, Gmsh). Permissively licensed code (MIT, BSD, Apache-2.0, MPL-2.0) may be read
  for reference; anything adapted is noted in ATTRIBUTION.md with its licence.
- **Assets** must be original or openly licensed and have a row in ATTRIBUTION.md.
- **Everything is a command.** User-visible behaviour = a command in `crates/engine/src/cmd.rs`
  (`CommandSpec`: dotted, lower-case id `<area>.<thing>`, label, documented parameters, JSON
  `run`) + tests. The UI, scripts and CLI all reach the same commands. Programmatic calls never
  open dialogs.
- **Layering** is enforced by `cargo xtask layers` (`docs/architecture.md`): L0 `model` → L1
  `mesh`, `fem`, `theory`, `render` → L2 `solver` → L6 `engine` → L7 `ui-egui` → apps. Nothing
  below L7 depends on egui/eframe/winit/wgpu/glow.
- **Determinism.** The same input gives the same mesh and the same solver result, bit for bit:
  no hash-order iteration into output, no thread-count-dependent sums (the factorization splits
  work by columns, each entry summed in one fixed order).
- **Units: store SI, convert only at boundaries through `crates/model/src/units.rs`** (docs/units.md).
  Command parameters and replies are in the session's display units (mm and GHz by default).
  Time dependence is e^{jωt}.
- **Rust only.**

## Workflow for every change
1. Work on a branch. While you work: `cargo check` and the tests of the crates you touch
   (`cargo test -p <crate>`). Commit after each working step; every commit builds and passes its
   tests.
2. When the change is done: `cargo fmt --all`, `cargo clippy -p <crate> --all-targets -- -D warnings`
   for each crate you touched. Regenerate what your change affects: `docs/features.md` when
   commands change (`cargo xtask features`), `docs/oracle.md` when oracle cases or solver numerics
   change (`cargo xtask oracle`).
3. Fold fixes into the commit they fix (`git commit --fixup`, `git rebase -i --autosquash`).
4. **Keep the test gate fast.** A test that takes more than a few seconds (convergence studies,
   fine meshes) is `#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]`, with a fast
   representative test left in the gate.

## Running
- `cargo run -p emcraft-cli -- commands` lists every command with its parameters.
- `cargo run -p emcraft-cli -- run script.json` runs a script (a JSON array of
  `{"command", "params"}`) and prints the replies and the model summary.
- `cargo run --release -p emcraft-cli -- sample highpass --out DIR` solves the high-pass filter
  sample, prints its S-parameters against the exact solution and writes the pictures.
- `cargo run --release -p emcraft -- --sample highpass` opens the desktop app with it.

## Running and looking at the app
- `cargo run --release -p emcraft -- --sample highpass --control PORT` (sample + control
  channel). Pick a free port. Never screenshot the user's desktop.
- Drive it with JSON lines on `127.0.0.1:PORT` (docs/control-protocol.md). The first request on a
  connection carries the launch's token as `"token"`: read it from
  `~/.local/share/emcraft/control/PORT.token`, or start the app with
  `EMCRAFT_CONTROL_TOKEN=$(openssl rand -hex 32)` and send that.
  - `{"id":1,"method":"engine.execute","params":{"command":"solve.frequency","params":{"start":6,"stop":9}}}`
  - `{"id":2,"method":"ui.click","params":{"handle":"toolbar:mesh.generate"}}` (`ui.handles` lists them)
  - `{"id":3,"method":"ui.render","params":{"path":"/tmp/shot.png"}}` then read the PNG.
- **For UI work, look at the result.** `ui.render` draws the whole window on the CPU, no window
  or display needed; `ui.screenshot` captures the presented window. Without a display, use the
  headless scenarios: `EMCRAFT_SCENARIO_SHOTS=DIR cargo test -p emcraft-ui-egui scenario` writes
  each scenario's `shot` steps as PNGs into DIR.
- Every UI behaviour lands with a scenario in `crates/ui-egui/tests/scenarios/` (real clicks,
  keys and text through the control requests, checks on `ui.inspect`), run by `cargo test`.
- Launch-check the real app with a timeout (`timeout 10 cargo run --release -p emcraft --
  --sample highpass`) and confirm it starts without errors.

## Roadmap
`ROADMAP.md` tracks milestones. Update it whenever a milestone task lands.
