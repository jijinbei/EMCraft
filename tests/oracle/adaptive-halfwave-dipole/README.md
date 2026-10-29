# Adaptive refinement of the half-wave wire dipole

The `halfwave-dipole` case — two conducting cylinders (radius 0.5 mm, 7 mm long) with a 1 mm
gap fed by a lumped port, 15 mm in all, in a 60 mm box lined with a 15 mm perfectly matched
layer — meshed by `mesh.adapt` from three elements per wavelength: second-order elements, the
residual indicator with the layer left out (its mesh kept), Dörfler marking of 30 % and
remeshing, until the feed's reflection changes by less than 0.02. The indicator puts the error
at the gap and the wires' ends, where the field is singular: each pass marks only a few dozen
tetrahedra there (97, then 62).

Checked on the adapted mesh: the power balance (the port's ½ Re(V I*) against the power through
the Huygens box) and the directivity against the sinusoidal-current closed form.

| mesh | unknowns | ΔS | Z_in (Ω) | efficiency |
|---|---|---|---|---|
| pass 1 (3 per λ) | 79 554 | — | 110.4 + j27.9 | |
| pass 2 | 124 328 | 8.4e-3 | 110.3 + j25.9 | 0.9974 |
| (pass 3) | 127 218 | 1.0e-2 | 111.8 + j27.9 | 0.9993 |
| uniform, 4 per λ | 109 852 | | 110.9 + j27.9 | 0.9948 |

The input impedance has no closed form for a wire this thick (README of `halfwave-dipole`):
the passes stay within the ±1–2 Ω the uniform meshes scatter by (110.2 to 111.1 Ω at four to
seven elements per wavelength). Each pass remeshes the whole solid, so the reflection moves by
about 1e-2 between passes even where little was refined: a tolerance below that would not be
met. The directivity is the thick wire's (+2.8 % broadside against the thin-wire form, +2.1 %
uniform), not the mesh's.
