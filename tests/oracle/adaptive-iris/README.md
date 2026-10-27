# Adaptive refinement of the inductive iris

The `boolean-iris` case's model — WR-90 with two 2 mm plates subtracted, leaving a centred
12 mm window — meshed by `mesh.adapt` from 4 mm at order 2 instead of a fixed size. The plates'
four edges run the guide's height, and E grows as r^{-1/3} towards them (Meixner), so a uniform
mesh converges only as h^{4/3}. The residual indicator (element residual and face jumps of
tangential μ⁻¹∇×E and normal εE) is largest in the tetrahedra at the edges; Dörfler marking of
30 % of it picks a few of them each pass (8, 12, 32, 64, 96), and the remeshed solid grades from
half their size there.

| | unknowns | |S11| − reference |
|---|---|---|
| pass 1 (4 mm) | 10 588 | +0.0128 |
| pass 2 | 37 584 | +0.0047 |
| pass 3 | 79 326 | +0.0026 |
| pass 4 | 143 516 | +0.0014 |
| pass 5 | 244 302 | +0.0008 |
| pass 6 (ΔS 3.7e-4: converged) | 282 366 | +0.0006 |
| uniform 3 mm | 23 166 | +0.0108 |
| uniform 2 mm | 61 634 | +0.0058 |
| uniform 1.5 mm | 126 826 | +0.0044 |
| uniform 1.2 mm | 239 344 | +0.0032 |

At the same number of unknowns the adaptive mesh is four times closer; the uniform 1.2 mm
mesh's error is reached by pass 3 with a third of its unknowns. The reference is the same
converged mode matching as `boolean-iris` (640 modes), independent of the finite elements.
