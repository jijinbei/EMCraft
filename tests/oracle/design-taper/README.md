# A linear taper from a template's SolidCraft design

The `taper` template is a SolidCraft design compiled into EMCraft
(`crates/templates/designs/taper.solidcraft`, written by `make-taper.rs.txt`): the air of a
guide whose sides go linearly from WR-90 (22.86 × 10.16 mm) at z = 0 to WR-75 (19.05 ×
9.525 mm) at z = l, one ruled loft. The template's script opens it with
`geometry.import {path: "template:taper.solidcraft"}` and passes its parameters to the
design's (`parameters: {"l": "=l", …}`). This case opens the template with l = 10 mm (a short
taper, so its reflection is large enough to measure well) and solves it adaptively.

The reference is the theory of small reflections (Pozar §5.8–5.9, Collin §5.10) for the TE10
line with Z ∝ b/β, evaluated by `emcraft-theory::taper` (θ in closed form). It is a first-order
approximation: it neglects multiple reflections and the higher modes the sloping walls couple
to, and the impedance of a guide whose width changes is a model. The finite elements lie
1.4e-3 to 1.7e-3 below it at 10, 11 and 12 GHz (|S11| 0.048, 0.022, 0.010 against 0.049,
0.023, 0.012): the tolerance, 4e-3, is the theory's, not the solver's.
