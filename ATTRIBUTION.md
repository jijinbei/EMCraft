# Attribution

Every non-code asset in this repository is listed here with its author, source and licence;
code adapted from other projects is listed below with its licence.

**Policy:** EMCraft contains no iconography, images, artwork, fonts, templates, examples or
help text from commercial EM products. Every asset is original work by EMCraft contributors or
third-party material under an open licence.

## Assets

| File | Author | Source | Licence |
|---|---|---|---|
| docs/media/app-results-3d.png, docs/media/app-start.png | EMCraft contributors | Original: drawn by the UI scenarios `view3d_field_arrows` and `start_page` (`EMCRAFT_SCENARIO_SHOTS=DIR cargo test -p emcraft-ui-egui scenario`) | MIT OR Apache-2.0 |
| docs/media/highpass-fem-fdtd.png | EMCraft contributors | Original: drawn by `view.sparameters` after `file.sample highpass` solved and `solve.time_domain` from 5 to 9 GHz | MIT OR Apache-2.0 |
| docs/media/bandpass-h-10GHz.png | EMCraft contributors | Original: drawn by `emcraft-cli sample iris-bandpass --frequency 10` (h-10GHz.png) | MIT OR Apache-2.0 |
| docs/media/cavity-te101-h.png | EMCraft contributors | Original: drawn by `emcraft-cli sample cavity` (mode1-h-90deg.png) | MIT OR Apache-2.0 |
| docs/media/sphere-rcs-e-plane.png | EMCraft contributors | Original: drawn by `view.pattern` after the dielectric-sphere-scattering oracle recipe | MIT OR Apache-2.0 |
| tests/oracle/imported-post/post.3mf | EMCraft contributors | Original: written by SolidCraft (MIT OR Apache-2.0) from `tests/oracle/imported-post/solidcraft.json` (`solidcraft-cli run solidcraft.json`) | MIT OR Apache-2.0 |
| crates/engine/themes/*.json, docs/themes/night-shift.json | EMCraft contributors | Original: the built-in themes' colours (the palettes and token names of SolidCraft's theme, MIT OR Apache-2.0, with EMCraft's own accents, scene and picture colours; the one-hue themes derived from them, each colour re-hued at the same luminance) and an example user theme | MIT OR Apache-2.0 |
| crates/ui-egui/tests/fixtures/post-and-puck.3mf | EMCraft contributors | Original: a post and a puck modelled in SolidCraft and exported as 3MF with face tags | MIT OR Apache-2.0 |
| tests/oracle/imported-iris-cavity/block.3mf, crates/ui-egui/tests/fixtures/waveguide-block.3mf (the same file) | EMCraft contributors | Original: a WR-90 block with its channel and two iris plates, written by SolidCraft's CLI from `tests/oracle/imported-iris-cavity/block.solidcraft.json` (3MF with face tags) | MIT OR Apache-2.0 |
| tests/oracle/imported-iris-touching/iris.3mf | EMCraft contributors | Original: a WR-90 guide and two iris plates as three bodies, written by SolidCraft's CLI from `tests/oracle/imported-iris-touching/iris.solidcraft.json` (3MF with face tags) | MIT OR Apache-2.0 |
| crates/ui-egui/fonts/full/NotoSansJP-Regular.otf; crates/ui-egui/fonts/NotoSansJP-Regular-subset.otf (embedded) | Adobe (Source Han Sans), Google (Noto) | Noto Sans JP 2.004, the SubsetOTF JP build of https://github.com/notofonts/noto-cjk (unmodified, kept as the source); the embedded file is its subset (kana, JIS X 0208 symbols and first-level kanji, the translation's characters; outlines and names unchanged) made by `cargo xtask font-subset` — the OFL allows it, Noto has no Reserved Font Name | SIL Open Font License 1.1 — crates/ui-egui/fonts/OFL.txt |
| apps/emcraft/assets/emcraft.ico (and the window icon) | EMCraft contributors | Original: drawn by `emcraft_render::icon` (`cargo xtask icon`) | MIT OR Apache-2.0 |
| apps/emcraft/installer/emcraft.nsi | EMCraft contributors | Original: the Windows installer script, written for NSIS 3's Modern UI (NSIS itself is a build tool, not shipped; its installer stub is under the zlib/libpng licence) | MIT OR Apache-2.0 |
| crates/ui-egui/locales/ja.json | EMCraft contributors | Original: the window's Japanese translation | MIT OR Apache-2.0 |

| crates/render/fonts/full/DejaVuSans.ttf; crates/render/fonts/DejaVuSans-subset.ttf (embedded) | Bitstream, Inc. (Vera); the DejaVu fonts team (DejaVu changes) | DejaVu fonts 2.37, https://dejavu-fonts.github.io/ (unmodified, kept as the source); the embedded file is its subset (Latin, Greek, Cyrillic, symbols, the characters of the sources; outlines, metrics, kerning, names and copyright unchanged) made by `cargo xtask font-subset` — the licence allows modified copies under a name without "Bitstream" or "Vera", as DejaVu Sans is | Bitstream Vera Fonts licence; DejaVu changes public domain — crates/render/fonts/LICENSE-DejaVu.txt |

Text in pictures is set in DejaVu Sans (above). EMCraft's 5 × 7 bitmap font, drawn only if the
face cannot be read, is FieldCraft's (below). The window's Japanese (and any Japanese name)
is set in a Japanese font installed on the system when the window finds one, else in the
embedded subset of Noto Sans JP (above), a fallback after egui's own fonts (docs/languages.md).

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
| `crates/ui-egui/src/lib.rs` (`raw_input_hook`), `crates/ui-egui/src/shots.rs` (screenshots) | SolidCraft `crates/ui-egui/src/lib.rs` | one pointer event per frame as there |
| `apps/emcraft/src/control_server.rs` | SolidCraft `apps/solidcraft/src/control_server.rs` (itself from CADCraft, MIT OR Apache-2.0) | without the pending-request bookkeeping; EMCraft's waits |
| `apps/emcraft/src/control_auth.rs` | SolidCraft `crates/engine/src/control_auth.rs` | EMCraft's folders and variables; the token made here |
| `docs/control-protocol.md`, the "Running and looking at the app" section of `AGENTS.md` | SolidCraft's | rewritten for EMCraft's methods |

The ribbon, browser, dialogs, viewport and glyphs (`crates/ui-egui/src/{ribbon,browser,dialogs,dialog_ui,viewport,glyphs,worker}.rs`)
are new, laid out after SolidCraft's; no SolidCraft icon or image file is used (the glyphs are
EMCraft's own drawings in code).

The multifrontal factorizations (`crates/solver/src/multifrontal/`, `ldlt/`, `lu.rs`) are new; their pivoting follows the
same papers as FieldCraft's supernodal LDLᵀ.
