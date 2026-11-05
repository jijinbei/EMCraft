# Circular guide as a body of revolution

The straight circular guide of `circular-guide` (radius 10 mm, 30 mm long, ports on both ends)
solved on its meridian half-plane by `solve.revolution.frequency`, one azimuthal order at a
time: m = 1 passes TE11, m = 0 TM01, each port taking its circle's modes of that order in closed
form. Each transmission is compared with e^{−jβL}; the 3D solves of the same guide are
`circular-guide` (whole) and `cyclic-circular-guide` (a quarter).
