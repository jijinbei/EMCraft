# Microstrip on a lossy substrate

The `microstrip-eps-eff` line — a strip of no thickness on a 0.635 mm substrate of εr = 9.8,
W/h = 0.945, in a box large enough to leave it nearly open — with a loss tangent of 0.02 in the
substrate. The port's cross-section holds air and the lossy substrate, so its quasi-TEM mode is
hybrid and lossy, solved with the complex permittivity per triangle. Its attenuation at 1 GHz is
compared with the textbook dielectric attenuation of a quasi-TEM line, α_d = k0 εr (ε_eff − 1)
tan δ / (2 √ε_eff (εr − 1)), with ε_eff from Hammerstad and Jensen's fit (a published benchmark,
evaluated separately from EMCraft's code).
