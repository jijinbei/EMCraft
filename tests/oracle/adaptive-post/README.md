# Adaptive refinement of the inductive post

The `inductive-post` sample — a round conducting post, 3 mm across, from one broad wall of
WR-90 to the other, centred in 30 mm of guide — meshed by `mesh.adapt` instead of a fixed size:
second-order elements, a first mesh at 6 mm, then passes of the residual error indicator
(element residual and face jumps of tangential μ⁻¹∇×E and normal εE, summed over both ports'
excitations), Dörfler marking of 30 % of it and remeshing with the marked tetrahedra at half
their size, until S changes by less than 1e-3 from one pass to the next.

The reference is independent of the finite elements: a post running the whole height leaves the
fields independent of y, so E_y solves a two-dimensional Helmholtz problem between the side
walls with E_y = 0 on the post. Its current is found by a Nyström method on the post's circle
with the parallel-plate Green's function (the modal series with its static part in closed form),
which converges exponentially (1e-11 at 32 nodes), conserves power and is reciprocal to rounding
(`emcraft-theory::post`). Against it the finite elements agree to about 1e-4 at every mesh, two
independent methods meeting.

Measured (|S11 − reference|), one run:

| pass | unknowns | ΔS | error |
|---|---|---|---|
| 1 | 48 978 | — | 6.7e-4 |
| 2 | 53 722 | 3.4e-3 | 2.8e-3 |
| 3 | 103 528 | 2.9e-3 | 1.1e-4 |
| 4 | 287 116 | 2.4e-4 | 1.3e-4 |

The post is not a singularity (its surface is smooth and meets the broad walls square), and the
mesher already sizes its neighbourhood by curvature, so the indicator first marks the largest
tetrahedra away from it. What is left near 1e-4 is the geometry's: S moves by 478 per metre of
the post's radius, so 1e-4 is the radius off by 0.2 µm, the order of the quadratic edges'
departure from the circle at the curvature sizing's angle. Uniform meshes stall there as well
(5.9e-5 at 3 mm, 79 918 unknowns; 2.1e-4 at 2 mm; 1.3e-4 at 1.5 mm; 1.1e-4 at 1 mm, 372 700;
8.1e-5 at 0.8 mm, 649 940). The case checks that the loop converges to the reference within that
floor; `adaptive-iris`, with the r^{-1/3} field at its edges, shows what adaptivity gains. Each
pass remeshes the whole solid, so the error between passes is not monotone (pass 2), which is
why the loop asks for a change below the tolerance rather than trusting one pass.
