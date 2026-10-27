# Stripline stubs

A stripline, a strip of no thickness 2 mm wide midway between planes 2 mm apart in PTFE
(εr = 2.2), runs through a closed box from port to port. A stub of the same width branches off
it at right angles, 10 mm long, its end inside the box: first open, then shorted to both planes
by a via (a post from the bottom plane to the top across the stub's end, 2 mm past its edges).
The strips lie on one plane, the stripline's TEM is even about it, so only the half below is
meshed, a magnetic wall closing it with the strips conductors on that wall; the box's own
waveguide modes, odd about that plane, are then left out (the whole box's TE10 would propagate
from 4.8 GHz, just below the open stub's notch).

A shunt stub shorts the line where its input impedance vanishes: |S21| has a notch where the
open stub is a quarter wavelength long, the shorted one half a wavelength. The line model takes
the stub from the main line's edge, the junction as a node there, and adds the open end's
extension (see `stripline-ends`); λ is exact for TEM. The notch frequencies are read as the
vertex of the parabola through |S21|² at the sampled minimum and its neighbours (|S21| ∝ |f − f₀|
at a notch), the sweeps solved by the reduced-basis fast sweep.

What the line model leaves out is the T-junction's own equivalent circuit, which grows with the
strips' width: with strips 4 mm wide (twice the spacing) the notches fall 1.9 % and 3.5 % below
the line model, at 2 mm (this case) 0.45 % and 0.05 % above it.
