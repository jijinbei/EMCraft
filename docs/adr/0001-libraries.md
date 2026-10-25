# ADR 0001: Libraries

Status: accepted (2026-10-25).

EMCraft keeps its dependency tree small and permissively licensed (MIT, Apache-2.0, BSD,
Zlib). The numerical core (elements, meshing, ordering, factorization) is our own code, as in
FieldCraft, so its behaviour is deterministic and fully verified.

| Crate | Licence | Used for |
|---|---|---|
| `thiserror` | MIT OR Apache-2.0 | error types |
| `serde`, `serde_json` | MIT OR Apache-2.0 | command parameters and replies, scripts, oracle files |
| `num-complex` | MIT OR Apache-2.0 | complex arithmetic of the frequency-domain solver |
| `png` | MIT OR Apache-2.0 | PNG pictures |
| `zip` (deflate only), `quick-xml` | MIT | reading 3MF packages (`emcraft-io`), the same lines as FieldCraft's |
| `egui`, `eframe` (glow backend) | MIT OR Apache-2.0 | the desktop UI (L7 and apps only) |
| `getrandom` | MIT OR Apache-2.0 | the control channel's per-launch token (`apps/emcraft` only; already in the tree through eframe) |

Rejected: any GPL/LGPL/AGPL solver or mesher (Elmer, openEMS, MEEP, GetDP, NGSolve, Gmsh, …) —
not read, not linked. External sparse solvers (MUMPS, PARDISO, SuiteSparse) are not used: they
are C/Fortran, some under licences we do not take, and our own multifrontal factorization keeps
the build pure Rust and deterministic.
