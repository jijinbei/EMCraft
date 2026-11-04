# Empty guide as a solid, time domain

A WR-90 box 30 mm long built with `geometry.solid` (not `geometry.waveguide`), solved by the
time-domain solver for solids: the box staircased on a tensor grid, each port's face extended
outward by a feed and a CPML, TE10 launched from each port in turn (`excite` both). Its S21
against the empty guide's e^{−jβL}.
