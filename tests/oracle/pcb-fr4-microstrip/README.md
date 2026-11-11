# A 50 Ω microstrip on FR-4 at 1.09 GHz

The line of an ADS-B receiver's board: a 3.06 mm strip on 1.6 mm of FR-4 (εr = 4.4,
tan δ = 0.02) over a ground floor, 10 mm long, loaded with `pcb.load` and meshed by the planar
mesher, its box scaled from `pcb-microstrip-fem` (walls 12 W to either side, the cover 37 h up).
The port mode's ε_eff is compared with Hammerstad and Jensen's fit carried to 1.09 GHz by
Kirschning and Jansen's dispersion (+0.5 % at f·h = 1.7 GHz·mm, which the FEM finds), its
attenuation with the quasi-TEM line's dielectric loss, and the line's S21 phase with the delay ε_eff gives —
the closed forms `pcb-microstrip-fem` and `pcb-lossy-microstrip-fem` check on alumina, here on
the substrate and at the frequency of the `adsb-patch` template. (A lossy mode has no impedance
in the reply: Z0 is checked on the lossless alumina line.)
