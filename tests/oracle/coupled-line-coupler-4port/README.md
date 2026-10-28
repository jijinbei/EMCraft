# Four-port coupled-line coupler

Two strips side by side, a narrow gap apart, run the whole length of a stripline box: a quarter
wavelength at 5 GHz. Every end of every strip is a port of its own, so the box has two terminal
ports, one at each end, each with a terminal per strip (`1:1`, `1:2` at z = 0; `2:1`, `2:2` at
the far end), all referred to 50 Ω.

A wave into `1:1` leaves mostly through the same strip at the far end (`2:1`, the through
port); a tenth of its power crosses to the other strip and comes back out at the near end
(`1:2`, the coupled port), a quarter period ahead of the through wave; nothing reaches the
other strip's far end (`2:2`, isolated) and nothing is reflected. That is the backward-wave
directional coupler: the strips' even and odd modes travel at the same speed in one dielectric,
and the terminations are the geometric mean of their impedances.

The solver excites each port's two quasi-TEM modes, the even and odd ones, and converts its
modal S-matrix to the strips' voltages and currents and their 50 Ω power waves. The strips
were sized for 10 dB with Cohn's mapping inverted; the reference is the ideal TEM coupler made
of those strips as built. With no open ends and no bends, the only approximation left is the
mesh, chiefly the field in the 0.18 mm gap, which sets the odd mode's impedance and so the
coupling. The half below the strips' plane is meshed, under a magnetic wall that cuts both
strips, so each terminal stays a terminal.
