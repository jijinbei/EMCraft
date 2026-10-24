# Inductive iris

A centred iris across the broad wall of WR-90 (a = 22.86 mm, b = 10.16 mm): 2 mm thick, leaving
a 12 mm wide aperture, 15 mm from each port. The iris acts as a shunt inductance: it reflects
three quarters of the incident amplitude at 10 GHz.

The iris' edges run along y, so the field stays independent of y and only TE_m0 modes take part:
mode matching (the TE_m0 modes of the guide and of the aperture matched on both faces of the
iris, cascaded through its thickness) converges to the exact answer as the number of modes
grows, an independent method. The FEM meshes the iris in 3D (all four elements' layers across
b), graded towards its edges.
