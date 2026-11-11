# Attribution

Every non-code asset in this repository is listed here with its author, source and licence;
code adapted from other projects is listed below with its licence.

**Policy:** EMCraft contains no iconography, images, artwork, fonts, templates, examples or
help text from commercial EM products. Every asset is original work by EMCraft contributors or
third-party material under an open licence.

## Assets

| File | Author | Source | Licence |
|---|---|---|---|
| crates/templates/scripts/pcb-{microstrip,split-ground,parallel}.json | EMCraft contributors | Original parameterized PCB routing templates, with continuous, split and neighbouring-trace examples | MIT OR Apache-2.0 |
| crates/io/tests/fixtures/route.kicad_pcb; tests/oracle/pcb-route-line/{recipe,expected,fixture}.json; PCB route examples in crates/engine/src/pcb.rs | EMCraft contributors | Original two-layer routing examples and independently evaluated published HJ/ABCD formulas; no third-party board data or KiCad source code | MIT OR Apache-2.0 |
| docs/media/app-start.png, docs/media/app-sparameters.png, docs/media/app-results-3d.png, docs/media/app-pcb-board.png | EMCraft contributors | Original: the desktop app's own window, drawn by `ui.render` over its control channel (docs/control-protocol.md) — the start page, the `highpass` sample solved (its S-parameters and its field in the 3D view) and the `pcb-board` sample's coupling illustration | MIT OR Apache-2.0 |
| docs/media/highpass-fem-fdtd.png | EMCraft contributors | Original: drawn by `view.sparameters` after `file.sample highpass` solved and `solve.time_domain` from 5 to 9 GHz | MIT OR Apache-2.0 |
| docs/media/bandpass-h-10GHz.png | EMCraft contributors | Original: drawn by `emcraft-cli sample iris-bandpass --frequency 10` (h-10GHz.png) | MIT OR Apache-2.0 |
| docs/media/cavity-te101-h.png | EMCraft contributors | Original: drawn by `emcraft-cli sample cavity` (mode1-h-90deg.png) | MIT OR Apache-2.0 |
| docs/media/sphere-rcs-e-plane.png | EMCraft contributors | Original: drawn by `view.pattern` after the dielectric-sphere-scattering oracle recipe | MIT OR Apache-2.0 |
| tests/oracle/design-post/post.solidcraft | EMCraft contributors | Original: a WR-90 guide with a post of diameter `d` cut across it, written with SolidCraft's document API (MIT OR Apache-2.0) by `tests/oracle/design-post/make-post.rs.txt` | MIT OR Apache-2.0 |
| crates/templates/designs/taper.solidcraft, crates/templates/designs/horn.solidcraft | EMCraft contributors | Original: the `taper` template's linear taper between two rectangular guides and the `horn` template's pyramidal horn (its metal walls and the air inside), their sizes parameters, written with SolidCraft's document API (MIT OR Apache-2.0) by `crates/templates/designs/make-taper.rs.txt` and `make-horn.rs.txt` | MIT OR Apache-2.0 |
| crates/templates/designs/helical-cell.solidcraft | EMCraft contributors | Original: one period of a circular guide whose wall turns as a three-lobed helix (an equation curve swept along z with a twist), written with SolidCraft's document API (MIT OR Apache-2.0) by `crates/templates/designs/make-helical-cell.rs.txt` | MIT OR Apache-2.0 |
| crates/templates/designs/corrugated-cell.solidcraft | EMCraft contributors | Original: one period of a circular guide whose wall waves about its axis (an equation curve revolved), written with SolidCraft's document API (MIT OR Apache-2.0) by `crates/templates/designs/make-corrugated-circ.rs.txt` | MIT OR Apache-2.0 |
| crates/templates/designs/adsb-patch.s1p, crates/templates/designs/adsb-bandpass.s2p | EMCraft contributors | Original: the `adsb-patch` antenna's S11 (solve.radiation_sweep, 1.04–1.14 GHz) and the `adsb-bandpass` filter's S (solve.frequency, 0.9–1.3 GHz), solved by EMCraft from those templates and written as Touchstone files (results.touchstone), the `adsb-receiver` template's antenna and filter | MIT OR Apache-2.0 |
| crates/engine/src/tests/data/horn-crease.solidcraft | EMCraft contributors | Original: the `horn` template's design as first written by `make-horn.rs.txt` (before dade85b: its air straight for the last millimetre past the mouth), kept as the regression case of a crease lying in another body's face | MIT OR Apache-2.0 |
| tests/oracle/imported-post/post.3mf | EMCraft contributors | Original: written by SolidCraft (MIT OR Apache-2.0) from `tests/oracle/imported-post/solidcraft.json` (`solidcraft-cli run solidcraft.json`) | MIT OR Apache-2.0 |
| crates/engine/themes/*.json, docs/themes/night-shift.json | EMCraft contributors | Original: the built-in themes' colours (the palettes and token names of SolidCraft's theme, MIT OR Apache-2.0, with EMCraft's own accents, scene and picture colours; the one-hue themes derived from them, each colour re-hued at the same luminance) and an example user theme | MIT OR Apache-2.0 |
| crates/ui-egui/tests/fixtures/post-and-puck.3mf | EMCraft contributors | Original: a post and a puck modelled in SolidCraft and exported as 3MF with face tags | MIT OR Apache-2.0 |
| tests/oracle/imported-iris-cavity/block.3mf, crates/ui-egui/tests/fixtures/waveguide-block.3mf (the same file) | EMCraft contributors | Original: a WR-90 block with its channel and two iris plates, written by SolidCraft's CLI from `tests/oracle/imported-iris-cavity/block.solidcraft.json` (3MF with face tags) | MIT OR Apache-2.0 |
| tests/oracle/imported-iris-touching/iris.3mf | EMCraft contributors | Original: a WR-90 guide and two iris plates as three bodies, written by SolidCraft's CLI from `tests/oracle/imported-iris-touching/iris.solidcraft.json` (3MF with face tags) | MIT OR Apache-2.0 |
| crates/ui-egui/fonts/full/NotoSansJP-Regular.otf; crates/ui-egui/fonts/NotoSansJP-Regular-subset.otf (the tests' font, not in the app) | Adobe (Source Han Sans), Google (Noto) | Noto Sans JP 2.004, the SubsetOTF JP build of https://github.com/notofonts/noto-cjk (unmodified, kept as the source); the embedded file is its subset (kana, JIS X 0208 symbols and first-level kanji, the translation's characters; outlines and names unchanged) made by `cargo xtask font-subset` — the OFL allows it, Noto has no Reserved Font Name | SIL Open Font License 1.1 — crates/ui-egui/fonts/OFL.txt |
| apps/emcraft/assets/emcraft.ico (and the window icon) | EMCraft contributors | Original: drawn by `emcraft_render::icon` (`cargo xtask icon`) | MIT OR Apache-2.0 |
| apps/emcraft/installer/emcraft.nsi | EMCraft contributors | Original: the Windows installer script, written for NSIS 3's Modern UI (NSIS itself is a build tool, not shipped; its installer stub is under the zlib/libpng licence) | MIT OR Apache-2.0 |
| locales/ja.json | EMCraft contributors | Original: the window's Japanese translation | MIT OR Apache-2.0 |

| crates/render/fonts/full/DejaVuSans.ttf; crates/render/fonts/DejaVuSans-subset.ttf (embedded) | Bitstream, Inc. (Vera); the DejaVu fonts team (DejaVu changes) | DejaVu fonts 2.37, https://dejavu-fonts.github.io/ (unmodified, kept as the source); the embedded file is its subset (Latin, Greek, Cyrillic, symbols, the characters of the sources; outlines, metrics, kerning, names and copyright unchanged) made by `cargo xtask font-subset` — the licence allows modified copies under a name without "Bitstream" or "Vera", as DejaVu Sans is | Bitstream Vera Fonts licence; DejaVu changes public domain — crates/render/fonts/LICENSE-DejaVu.txt |

Text in pictures is set in DejaVu Sans (above). EMCraft's 5 × 7 bitmap font, drawn only if the
face cannot be read, is FieldCraft's (below). The window's Japanese (and any Japanese name)
is set in a Japanese font installed on the system when the window finds one, else in the
subset of Noto Sans JP (above) in the tests only; the app uses the system's fonts (docs/languages.md).

## Code adapted from sibling projects

FieldCraft (`../fieldcraft`, MIT OR Apache-2.0, Copyright (c) 2026 FieldCraft contributors):

| EMCraft file | Adapted from | Changes |
|---|---|---|
| `xtask/src/layers.rs` | FieldCraft `xtask/src/layers.rs` (itself from SolidCraft's xtask) | EMCraft's crate table; the SolidCraft gateway rule removed |
| `xtask/src/main.rs` | FieldCraft `xtask/src/main.rs` | reduced to layers, test, oracle, features and ci |
| `crates/tetmesh/src/{predicates,delaunay,cdt,surface,quality,tet}/`, `size.rs`, `seeds.rs`, `vec3.rs`, `message.rs`, `lib.rs` | FieldCraft `crates/mesh/src/` (the same modules) | the crate's name and error type; hex-only quality reports and model-based reports removed; `compat.rs` adapts what they used from FieldCraft's `crates/model` (element kinds, the Jacobian rule, coordinate checks, seeds spacing, number formatting, a no-op progress channel) |
| `crates/mesh/src/planar/cdt.rs` | FieldCraft `crates/mesh/src/quad/cdt.rs` | constraints of two kinds (the region's outline, segments inside it), the region classified by the outline alone; Steiner points rounded to the planar lattice; a size closure in place of the size field; points kept by id for the caller |
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
| `crates/render/src/view3d/navigate.rs` | SolidCraft `crates/render/src/camera.rs` (`zoom_at`, `orbit_about`, `frame_coords`/`place`, `looking_from`, `interpolate`, `CameraAnim`, `ease_in_out`) and `crates/engine/src/cmd/view_navigation.rs` (the arrows' turn) | on EMCraft's y-up camera without roll or quaternions; distance interpolated geometrically; path-independent drags and the snap's margin (SolidCraft `navigation.rs`: `orbit`, `orbit_drag`, `snapped_axis_camera`) as a `Drag` |
| `crates/ui-core/src/navigation.rs`, `crates/ui-egui/src/navigation.rs`, `navigate`/`wheel`/`keys` in `crates/ui-egui/src/view3d.rs` | SolidCraft `crates/ui-egui/src/prefs.rs` (`NAV_PRESETS`, `NavPreset`, `OrbitAround`), `crates/ui-egui/src/navigation.rs` (`handle`, `immediate_wheel`, `pinch_factor`, `view_key`, `step_key_hold`, `middle_double_click`) | the mouse mappings, wheel and pinch rates, key steps and animation timings as there; no 3D mouse, free orbit, roll, wheel-speed or zoom-to-cursor settings, momentary Space pan kept; EMCraft's in-view handles take a plain left press |
| `crates/ui-egui/src/selection.rs`, `box_select`/`paint_box` in `crates/ui-egui/src/view3d.rs` | SolidCraft `crates/ui-egui/src/selection.rs` (`BoxSel`, `segment_hits_rect`, `segments_cross`, their test), `crates/ui-egui/src/viewport.rs` (`box_select`, `paint_box_selection`), `tool_modifiers.rs` (`selection_click`: Shift adds, Ctrl toggles) | window and crossing on the projected triangles of each 3D part (a crossing box inside a triangle touches it); EMCraft's parts (`Pick`) instead of bodies, faces and edges; the theme's accent and success colours |
| `crates/ui-egui/src/view_cube.rs` | SolidCraft `crates/ui-egui/src/navigation_cube.rs`, `assets/icons/ui/source/navigation.rs` (`paint_cube`, `arrow`) | y up; labels drawn upright (not mapped onto the faces); no roll arrows; the theme's panel, border, text and accent colours |
| `crates/render/src/view3d/bvh.rs` | SolidCraft `crates/geom/src/bvh.rs` (`Bvh::new`, `visit`, `nearest`, the ray–box test) | on EMCraft's scene triangles (`[f64; 3]` boxes), the split ordered by centre then item, boxes grown by their size too; `SceneIndex` (nearest and nearest solid tagged triangle, the triangles near a plane) instead of `MeshIndex`; no pair traversal |
| `crates/render/src/view3d/simplify.rs` | SolidCraft `crates/geom/src/simplify.rs` | on `[f64; 3]` points; no error measure (and no original-surface index); `coarser`, a scene simplified group by group (colour, opacity, tag), is new |
| `crates/ui-egui/src/scene3d/cache.rs` | SolidCraft `crates/ui-egui/src/mesh_lod.rs` (levels made once per mesh on a thread, a cap on jobs, drawn in full until made) | one coarser copy per built scene for moving pictures, held with the scene's picking index; no per-instance level choice |
| `crates/gpu3d/src/` (the 3D view on the GPU) | SolidCraft `crates/gpu/src/view.rs`, `view/oit.rs`, `crates/ui-egui/src/gpu.rs` (the latter itself from CADCraft, MIT OR Apache-2.0) | the structure only — an offscreen frame shown by an `egui_wgpu` paint callback, the weighted blended OIT targets and composite (McGuire and Bavoil 2013), the uncaptured-error handler instead of wgpu's panic; written anew for EMCraft's scene, its CPU renderer's projection and shading; the WGSL shaders are original |

The ribbon, browser, dialogs, viewport and glyphs (`crates/ui-egui/src/{ribbon,browser,dialogs,dialog_ui,viewport,glyphs,worker}.rs`)
are new, laid out after SolidCraft's; no SolidCraft icon or image file is used (the glyphs are
EMCraft's own drawings in code).

The multifrontal factorizations (`crates/solver/src/multifrontal/`, `ldlt/`, `lu.rs`) are new; their pivoting follows the
same papers as FieldCraft's supernodal LDLᵀ.
