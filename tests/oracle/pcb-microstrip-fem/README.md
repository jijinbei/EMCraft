# Microstrip through the planar mesher

The board itself — a 0.6 mm strip on 0.635 mm of alumina over a ground floor — is loaded with
`pcb.load`, its region set with `pcb.region` (the box of `microstrip-eps-eff`) and meshed by the
planar mesher: one triangulation for the board's layers, extruded through the substrate and the
air, the strip a conducting sheet. The port mode's ε_eff and Z0 are compared with Hammerstad and
Jensen's fit, and the 4 mm line's S21 with the phase that ε_eff gives.
