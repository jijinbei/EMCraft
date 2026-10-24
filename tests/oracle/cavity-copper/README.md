# Copper cavity

A rectangular cavity of WR-90 cross-section (22.86 × 10.16 mm) and 30 mm long, copper walls
(σ = 5.8e7 S/m): the `cavity` sample. `solve.eigenmodes` finds its four modes nearest 12 GHz —
TE101, TE102, TE201 and TE011, the four lowest — by shift-invert subspace iteration on the
lossless cavity, then the walls' Q by perturbation, Q = ωU/P_c with P_c = ½R_s∫|H_t|² dS.

References: the closed-form resonances of a rectangular cavity and the TE101 Q of its walls.
