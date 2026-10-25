# Attribution

Every non-code asset in this repository is listed here with its author, source and licence;
code adapted from other projects is listed below with its licence.

**Policy:** EMCraft contains no iconography, images, artwork, fonts, templates, examples or
help text from commercial EM products. Every asset is original work by EMCraft contributors or
third-party material under an open licence.

## Assets

| File | Author | Source | Licence |
|---|---|---|---|
| docs/media/s-parameters.png, docs/media/h-8GHz.png, docs/media/e-6GHz.png, docs/media/cross-8GHz.png | EMCraft contributors | Original: drawn by `emcraft-cli sample highpass --out docs/media --frequency 6 --frequency 8` | MIT OR Apache-2.0 |
| docs/media/highpass-fem-fdtd.png | EMCraft contributors | Original: drawn by `view.sparameters` after the highpass sample's sweep and `solve.time_domain` | MIT OR Apache-2.0 |
| docs/media/cavity-te101-h.png | EMCraft contributors | Original: drawn by `emcraft-cli sample cavity` (mode1-h-90deg.png) | MIT OR Apache-2.0 |
| docs/media/sphere-rcs-e-plane.png, docs/media/sphere-rcs-h-plane.png, docs/media/dipole-directivity.png | EMCraft contributors | Original: drawn by `view.pattern` after the dielectric-sphere-scattering and dipole-radiation oracle recipes | MIT OR Apache-2.0 |
| docs/media/bandpass-s-parameters.png, docs/media/bandpass-h-10GHz.png | EMCraft contributors | Original: drawn by `emcraft-cli sample iris-bandpass --frequency 10` | MIT OR Apache-2.0 |

EMCraft's 5 × 7 bitmap font is FieldCraft's (below).

## Code adapted from sibling projects

FieldCraft (`../fieldcraft`, MIT OR Apache-2.0, Copyright (c) 2026 FieldCraft contributors):

| EMCraft file | Adapted from | Changes |
|---|---|---|
| `xtask/src/layers.rs` | FieldCraft `xtask/src/layers.rs` (itself from SolidCraft's xtask) | EMCraft's crate table; the SolidCraft gateway rule removed |
| `xtask/src/main.rs` | FieldCraft `xtask/src/main.rs` | reduced to layers, test, oracle, features and ci |
| `crates/tetmesh/src/{predicates,delaunay,cdt,surface,quality,tet}/`, `size.rs`, `seeds.rs`, `vec3.rs`, `message.rs`, `lib.rs` | FieldCraft `crates/mesh/src/` (the same modules) | the crate's name and error type; hex-only quality reports and model-based reports removed; `compat.rs` adapts what they used from FieldCraft's `crates/model` (element kinds, the Jacobian rule, coordinate checks, seeds spacing, number formatting, a no-op progress channel) |
| `crates/solver/src/ordering.rs` | FieldCraft `crates/solver/src/ordering.rs` | the coordinate nested dissection only |
| `crates/io/src/threemf.rs` | FieldCraft `crates/geometry/src/tagged_mesh.rs` | surfaces in metres with EMCraft's own unit table; the stitching and SolidCraft surface types removed |
| `crates/render/src/font.rs` | FieldCraft `crates/render/src/font.rs` | marks `\|`, `^`, `<`, `>`, `*` and the Greek `ω`, `θ`, `φ`, `λ`, `ε`, `σ`, `π` added |
| `crates/testkit/src/oracle.rs` | FieldCraft `crates/testkit/src/oracle.rs` | simplified case format (no pending, slow or nightly cases), EMCraft's engine |
| `AGENTS.md`, `rustfmt.toml`, `clippy.toml`, workspace lints | FieldCraft's | rewritten for EMCraft |

SolidCraft (`../solidcraft`, MIT OR Apache-2.0, Copyright (c) 2026 SolidCraft contributors):

| EMCraft file | Adapted from | Changes |
|---|---|---|
| `crates/ui-egui/src/theme.rs` | SolidCraft `crates/ui-egui/src/theme.rs`, `assets/styles/ui/palettes.rs` | the token names and the light and dark palette values EMCraft uses; per-tab accents for EMCraft's tabs (values from SolidCraft's tab palette); no colour vision modes or theme files |
| `crates/ui-egui/src/ui_kit.rs` | SolidCraft `crates/ui-egui/src/ui_kit/` (panel header, layout rows, actions, status progress) | reduced to a card header, label rows, buttons, error band, section heading and progress bar; drawn marks instead of SVG icons |
| `crates/ui-egui/src/control.rs` | SolidCraft `crates/ui-egui/src/control.rs` | EMCraft's methods; injected pointer and key events as there; replies that wait for a worker job |
| `crates/ui-egui/src/scenario.rs` | SolidCraft `crates/ui-egui/src/scenario.rs` | the handle registry and the headless harness; EMCraft's steps and checks |
| `crates/ui-egui/src/raster.rs` | SolidCraft `crates/ui-egui/src/ui_kit/checks_tests.rs` (`raster`, `textures_update`) | bilinear texture sampling, no panics on partial or missing textures, PNG through `emcraft-render` |
| `crates/ui-egui/src/lib.rs` (`raw_input_hook`, screenshots) | SolidCraft `crates/ui-egui/src/lib.rs` | one pointer event per frame as there |
| `apps/emcraft/src/control_server.rs` | SolidCraft `apps/solidcraft/src/control_server.rs` (itself from CADCraft, MIT OR Apache-2.0) | without the pending-request bookkeeping; EMCraft's waits |
| `apps/emcraft/src/control_auth.rs` | SolidCraft `crates/engine/src/control_auth.rs` | EMCraft's folders and variables; the token made here |
| `docs/control-protocol.md`, the "Running and looking at the app" section of `AGENTS.md` | SolidCraft's | rewritten for EMCraft's methods |

The ribbon, browser, dialogs, viewport and glyphs (`crates/ui-egui/src/{ribbon,browser,dialogs,dialog_ui,viewport,glyphs,worker}.rs`)
are new, laid out after SolidCraft's; no SolidCraft icon or image file is used (the glyphs are
EMCraft's own drawings in code).

The multifrontal factorization (`crates/solver/src/ldlt.rs`) is new; its pivoting follows the
same papers as FieldCraft's supernodal LDLᵀ.
