# ADR 0011: Fallible allocation of the solver's large buffers

Status: accepted (2026-11-14); done (2026-11-14).

## Context

A failed allocation in Rust does not panic: `vec![x; n]`, `Vec::with_capacity(n)` and every
growth of a vector call `handle_alloc_error`, which aborts the process. `Session::execute`'s
`catch_unwind` cannot stop an abort, so a model too large for the machine lost the user's work.
The factorization already plans its fronts against the memory the system has
(`multifrontal::Plan`), but the buffers themselves, the assembled matrices before it and the
right-hand sides after it were allocated infallibly; and Linux promises memory it may not have,
so a large allocation succeeds and the process is killed as it is filled.

Rust 1.100 stabilised the `Allocator` trait and the never type. A custom allocator needs an
`unsafe impl`, which the workspace forbids (`unsafe_code = "forbid"`), and a failing allocator
still aborts in every infallible method; neither helps here.

## Decision

- **One place for large buffers:** `emcraft_solver::memory` (`with_capacity`, `zeros`,
  `filled`, `copied`) returns `Result<Vec<T>, OutOfMemory>`: the size checked for overflow, a
  buffer of 256 MB or more weighed against `MemAvailable` first, then `try_reserve_exact`. The
  error says what the buffer was for and its size, and reaches the user through `FactorError` and
  `SolveError` like any refusal.
- **Where:** every buffer whose size grows with the problem — the frontal matrices, the update
  matrices, the factor's columns (and their trip to and from the scratch file), the blocks'
  products in LDLᵀ and LU, the assembled matrix's pattern and entries (`Csr::from_groups`,
  `Csr::zeroed`), the right-hand sides of `solve_many`, a revolution's field picture. Buffers of a
  bounded size (a port's matrix, an element's, a subspace's) stay as they are.
- **Kept:** `cargo xtask style` refuses a product-sized `vec!` in the factorization and the
  assembly outside tests.
- **No unsafe, no custom allocator.** Zeroing a buffer the safe way writes it once more than
  `calloc`'s zero pages; measured on the high-pass filter at 125 482 unknowns (order 2) the solve
  took the same time before and after (9.5 s, within the runs' spread), so the buffers are not
  pooled or left unzeroed.

## Consequences

A model too large for the machine is refused with what it needed instead of closing the window.
Allocations elsewhere (small vectors, a vector's growth inside a loop) still abort on failure;
they are bounded by the limits on unknowns, frequencies and picture sizes.
