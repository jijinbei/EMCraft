# Pillbox accelerating cavity

The figures an accelerator's RF cavity is judged by, for the TM010 mode of a copper pillbox
10 mm in radius and 13.06 mm long (half a wavelength at 11.47 GHz: a particle at the speed of
light crosses it in half a period), from `results.cavity` on the eigenmode `solve.eigenmodes`
finds: the voltage along the axis, the transit-time factor T, R/Q in the linac convention
V²/(ωU), the copper walls' Q0 by perturbation and the shunt impedance (R/Q) Q0.

The references are the pillbox's closed forms (Wangler, *RF Linear Accelerators*, §2.3),
evaluated apart from the solver. The voltage is integrated by three-point Gauss–Legendre on
400 intervals, which samples no end point: E_z is largest on the end walls, and the quadrature
reads it just inside them.

| | 2 mm | 1.5 mm | closed form |
|---|---|---|---|
| f (GHz) | 11.47434 | 11.47429 | 11.47425 |
| T | 0.636799 | 0.636798 | 0.636800 |
| R/Q (Ω) | 195.972 | 195.972 | 195.973 |
| Q0 | 9124 | 9146 | 9180 |
| shunt (MΩ) | 1.788 | 1.792 | 1.799 |

With beam pipes (3 mm in radius, 10 mm long each side, the profile revolved with 128 facets),
no closed form holds; the 3D eigenmode (11.6284 GHz at 1.5 mm, 11.6234 GHz at 1 mm) and the
body-of-revolution solver on the same profile (11.6067 GHz at 0.3 mm, 11.6103 GHz at 0.2 mm)
meet at about 11.615 GHz, within 4e-4 once each is extrapolated and the facets allowed for;
R/Q falls to 139.5 Ω and T to 0.535 (the field reaching into the pipes lengthens the gap).
The engine's heavy test `a_beam_pipe_cavity_agrees_with_its_body_of_revolution` keeps that.
