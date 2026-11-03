# Lossy microstrip through the planar mesher

The board of [`pcb-microstrip-fem`](../pcb-microstrip-fem/README.md) — a 0.6 mm strip on 0.635 mm
of εr = 9.8 over a ground floor — with a loss tangent of 0.02 in its substrate (the layer's
`loss_below`, which a KiCad stackup's `loss_tangent` sets). `pcb.region` makes the substrate a
lossy material, the planar mesher fills it and the port's cross-section solver sees it: the
quasi-TEM mode's attenuation is compared with the textbook dielectric attenuation of a quasi-TEM
line (the reference of [`lossy-microstrip`](../lossy-microstrip/README.md), computed independently
of EMCraft's code), and the 4 mm line's |S21| with the loss it gives over its length.
