# ADR 0001: Libraries

Status: accepted (2026-10-25).

EMCraft keeps its dependency tree small and permissively licensed (MIT, Apache-2.0, BSD,
Zlib). The numerical core (elements, meshing, ordering, factorization) is our own code, as in
FieldCraft, so its behaviour is deterministic and fully verified.

| Crate | Licence | Used for |
|---|---|---|
| `thiserror` | MIT OR Apache-2.0 | error types |
| `serde`, `serde_json` | MIT OR Apache-2.0 | command parameters and replies, scripts, oracle files; `emcraft-i18n` reads the translations with it when a crate compiles |
| `num-complex` | MIT OR Apache-2.0 | complex arithmetic of the frequency-domain solver |
| `png` | MIT OR Apache-2.0 | PNG pictures |
| `ab_glyph` (with `ab_glyph_rasterizer`, `owned_ttf_parser`) | Apache-2.0 (`owned_ttf_parser`, `ttf-parser` MIT OR Apache-2.0) | text in pictures (`emcraft-render::text`): reads the embedded DejaVu Sans and rasterises its outlines with exact-area anti-aliasing, deterministically |
| `zip` (deflate only), `quick-xml` | MIT | reading 3MF packages (`emcraft-io`), the same lines as FieldCraft's |
| `faer` (with its `gemm`, `pulp`, `rayon`) | MIT (`rayon` MIT OR Apache-2.0) | the dense matrix products inside the multifrontal factorization (`emcraft-solver::ldlt`): pure Rust, SIMD chosen at run time, its sums in an order independent of the threads (checked bit for bit); the factorization, ordering and pivoting stay ours. `rayon` also steps `emcraft-fdtd`'s particle-in-cell grid: its fields plane by plane and its particles chunk by chunk, their currents added in the particles' order, the same bits on any number of threads (tested) |
| `iced` (git, main line at a pinned revision; with its forks of `winit` and `cosmic-text`), `iced_runtime` (the user interface run headless, `ui.render`), `iced_test` (dev-dependency) | MIT (`winit` Apache-2.0, `cosmic-text` MIT OR Apache-2.0) | the desktop window (`emcraft-ui-iced`, `apps/emcraft`; L7 and apps only; it draws with wgpu and, on the CPU, tiny-skia): [ADR 0007](0007-iced-window.md), [ADR 0008](0008-retire-egui.md); wgpu and what it brings in: [ADR 0004](0004-gpu-viewport.md) |
| `rfd` (with its xdg-portal backend) | MIT | the system's file dialog for opening and saving projects (`emcraft-picker` only, which the window uses; it takes it through `emcraft_ui_core::files::Picker`, so the UI crates and the headless harness need none) |
| `cubecl` (with its bundled LLVM, and `cudarc` for the `cuda` feature) | MIT OR Apache-2.0 | the time domain's kernels on GPUs (`emcraft-gpu`, [ADR 0002](0002-gpu.md)); the CPU runtime (`cubecl-cpu` feature) only in the tests; the Windows GNU build (the installer, docs/install-windows.md) links neither runtime, as the bundled LLVM has no build for it |
| `winresource` (build dependency, no default features) | MIT | the icon and version resource of `emcraft.exe` on Windows (`apps/emcraft/build.rs`; calls the toolchain's `windres` or `rc.exe`) |
| `getrandom` | MIT OR Apache-2.0 | the control channel's per-launch token (`crates/control` only; already in the tree through wgpu and iced) |
| `fontdb` | MIT | the system's fonts for the window's language (`emcraft-ui-core::fonts`: the font folders, fontconfig's on Linux, each face's families, weight and style; nothing embedded in the app) |
| `ttf-parser` (direct) | MIT OR Apache-2.0 | whether a face shows the language's characters (its character map; already in the tree through `ab_glyph` and `fontdb`) |
| `allsorts` (pure-Rust deflate), `ttf-parser` — xtask only, its `fonts` feature | Apache-2.0 (`allsorts`) | `cargo xtask font-subset`: the pictures' font's subset (glyphs cut by `allsorts`, checked by `ttf-parser`); never in the apps |
| `approx`, `proptest`, `rstest` — dev-dependencies only, without default features | Apache-2.0 (`approx`), MIT OR Apache-2.0 | tests: tolerance asserts that print both values (`approx::assert_abs_diff_eq!`); property tests of hostile input (`proptest`, always with a fixed `rng_seed` and no failure persistence, so the gate stays deterministic); table tests whose cases pass or fail one by one (`rstest`, `#[case]`) |

Rejected: any GPL/LGPL/AGPL solver or mesher (Elmer, openEMS, MEEP, GetDP, NGSolve, Gmsh, …) —
not read, not linked. External sparse solvers (MUMPS, PARDISO, SuiteSparse) are not used: they
are C/Fortran, some under licences we do not take, and our own multifrontal factorization keeps
the build pure Rust and deterministic.
