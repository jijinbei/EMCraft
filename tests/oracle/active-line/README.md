# Active reflection of a two-element "array"

The [lossy-line](../lossy-line/) model (30 mm of WR-90 filled with a lossy dielectric, its ports
in the filling, so matched) read as a two-element array, its ports the elements at z = 0 and
30 mm. Driving both at once with the phases that steer a beam toward û, element 1 sees the
wave from element 2 through the line, S12 a2/a1. Along the line (θ = 0) the steering phase
adds the free-space delay k0·L, so Γ1 = e^{−γL} e^{−jk0L} and Γ2 = e^{−γL} e^{+jk0L}; across it
(θ = 90°) there is no path difference and Γ1 is S21 itself. `results.active` reads them from
the solved S and the elements' positions.
