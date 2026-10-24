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
| `crates/render/src/font.rs` | FieldCraft `crates/render/src/font.rs` | marks `\|`, `^`, `<`, `>`, `*`, `ω` added |
| `crates/testkit/src/oracle.rs` | FieldCraft `crates/testkit/src/oracle.rs` | simplified case format (no pending, slow or nightly cases), EMCraft's engine |
| `AGENTS.md`, `rustfmt.toml`, `clippy.toml`, workspace lints | FieldCraft's | rewritten for EMCraft |

The multifrontal factorization (`crates/solver/src/ldlt.rs`) is new; its pivoting follows the
same papers as FieldCraft's supernodal LDLᵀ.
