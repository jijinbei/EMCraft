# Conductor loss of a microstrip with thick copper

The alumina microstrip of `pcb-microstrip-fem` (0.6 mm strip on 0.635 mm of εr = 9.8, 4 mm long,
in a smaller box: 7.2 mm wide, 6 mm of air above) with its strip 35 µm thick (`copper` on the
layer) and every conductor of copper (`model.walls` σ = 5.8e7 S/m): the strip stands into the
air, a hole in the mesh whose surfaces carry Leontovich's surface impedance, as do the ground and
the box. Its |S21| is compared with the conductor loss of Pucel, Massé and Hartwig in Hammerstad
and Jensen's closed form.

As a sheet of no thickness the strip's edge current is singular and its loss has no limit
(the formula's ln(2h/t) grows without bound): the mesh then gives +40 %. With the copper's
thickness the loss comes down towards the formula as the edges are graded finer (+10.3 % at
0.1 mm, +5.8 % at 0.05 mm).
