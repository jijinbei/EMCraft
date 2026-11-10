# Power handling of an empty guide

30 mm of empty WR-90 at 10 GHz. The TE10 field is largest on the guide's centre line, and for
a power P its amplitude is E0 = √(4 Z_TE P/(ab)) with Z_TE = η0 k0/β: 2.93 kV/m for 1 W. Air
breaks down near 3 MV/m, so the guide carries about 1.05 MW before it does.
`results.breakdown` finds the largest field of the solved 1 W wave over the mesh and scales it.
