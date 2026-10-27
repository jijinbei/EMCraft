# Half of the inductive-iris band-pass filter

The `iris-bandpass` filter (three WR-90 cavities between four centred 2 mm irises, passband
9.75–10.25 GHz) is symmetric about x = a/2, and its TE10 excitation is even about that plane:
tangential E (E_y) even, normal E (E_x) odd, so tangential H vanishes on it. The case cuts the
model there with `model.symmetry` (`axis` x, `kind` magnetic) and solves only x ∈ [0, a/2]: the
plane is a magnetic wall, the natural boundary condition of edge elements (its unknowns are kept,
no surface term is added), and the ports carry the closed-form modes even about it (TE10, TE30,
TE11, TM11, …) integrated over the half face, their norms half the whole's.

The mesh is the whole filter's (3 mm, graded to 0.3 mm at the 16 iris edges, one layer across y)
with x = a/2 a grid plane; the S-parameters must meet the whole filter's mode-matching reference
within the whole filter's tolerances. A solver test (`crates/solver/tests/symmetry.rs`) checks the
identity behind it to rounding: the whole mesh made of the half and its mirror image gives the
half's S within 1e-10.

Measured (three frequencies, each solved in full, one machine): the whole filter has 108 974
unknowns and solves in 2.4 s; the half has 54 912 and solves in 1.0 s. Its largest error against
mode matching is 7.2e-3, the whole's 6.9e-3 (their meshes differ by the lines the plane adds).

Reference: mode matching through all eight iris faces with 640 modes in the guide (the whole
filter's values, oracle `iris-bandpass`).
