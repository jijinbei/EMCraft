# Adaptive refinement of the half-wave wire dipole

The `halfwave-dipole` case — two conducting cylinders (radius 0.5 mm, 7 mm long) with a 1 mm
gap fed by a lumped port (a band of radius 0.4 mm), 15 mm in all, in a 60 mm box lined with a 15 mm perfectly matched
layer — meshed by `mesh.adapt` from three elements per wavelength: second-order elements, the
residual indicator with the layer left out (its mesh kept), Dörfler marking of 30 % and
remeshing, until the feed's reflection changes by less than 0.02. The indicator puts the error
at the gap and the wires' ends, where the field is singular: each pass marks only a few dozen
tetrahedra there.

Checked on the adapted mesh: the power balance (the port's ½ Re(V I*) against the power through
the Huygens box) and the directivity against the sinusoidal-current closed form.

| mesh | unknowns | ΔS | Z_in (Ω) | efficiency |
|---|---|---|---|---|
| pass 1 (3 per λ) | 79 554 | — | 114.8 + j38.2 | |
| pass 2 | 153 082 | 9.9e-3 | 113.9 + j35.9 | 0.9993 |
| uniform, 4 per λ | 109 852 | | 114.7 + j36.3 | 0.9948 |

The input impedance has no closed form for a wire this thick (README of `halfwave-dipole`):
the passes stay within the ±1–2 Ω the uniform meshes scatter by (114.0 to 114.7 Ω at four to
seven elements per wavelength). Each pass remeshes the whole solid, so the reflection moves by
about 1e-2 between passes even where little was refined: a tolerance below that would not be
met. The directivity is the thick wire's (+1.6 % broadside against the thin-wire form, +2.1 %
uniform), not the mesh's.
