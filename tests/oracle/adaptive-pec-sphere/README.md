# Adaptive refinement of the conducting sphere's scattering

The `pec-sphere-scattering` case — a plane wave on a perfectly conducting sphere of radius
5 mm (ka = 1.05) at 10 GHz, in a 50 mm box lined with a 12 mm perfectly matched layer —
meshed by `mesh.adapt` from three elements per wavelength instead of a fixed size: second-order
elements, the residual indicator of the scattered field (its element residual and face jumps,
the conductor's lifted trace −n × E_inc as boundary data, the layer left out), Dörfler marking
of 30 % of it and remeshing with the marked tetrahedra at half their size, until the far field
in 48 directions changes by less than 2 % from one pass to the next. Then `solve.radiation` on
the adapted mesh and the cross-sections against the Mie series (independent of the finite
elements).

Measured, one run (σ against the Mie series' 168.79 mm²):

| mesh | unknowns | far-field change | σ |
|---|---|---|---|
| pass 1 (3 per λ) | 19 660 | — | −0.46 % |
| pass 2 | 27 962 | 2.8e-2 | +1.45 % |
| pass 3 | 62 530 | 1.4e-2 | +0.51 % |
| (pass 4) | 156 142 | 4.7e-3 | +0.64 % |
| uniform, 5 per λ | 53 090 | | +2.38 % |
| uniform, 6 per λ | 92 528 | | +1.04 % |

The adapted mesh halves the uniform error with two thirds of its unknowns; the bistatic RCS
follows (+0.66, +0.96, +0.37, +0.70 % forwards, sideways in the E-plane, backwards and
sideways in the H-plane). The first pass's −0.46 % is errors of either sign cancelling, which
is why the loop judges convergence by the change between passes rather than by one pass. What
remains near +0.6 % is the layer's reflection and the Huygens box's quadrature, which the mesh
inside does not touch.
