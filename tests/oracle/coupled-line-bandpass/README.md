# Parallel-coupled band-pass filter

A stripline filter of the classical kind: three resonators, each a strip half a wavelength long
with both ends open, staggered across the box so that each half of one runs beside a half of
the next, a narrow gap apart; the 50 Ω feed lines run beside the first and last resonators'
outer halves. Every coupled length is a quarter wavelength at the centre frequency, less the
open end's extension. The strips' widths and gaps come from the textbook design: a Chebyshev
low-pass prototype of order three with 0.1 dB ripple, turned into the inverters a 10 %
band needs and into each coupled section's even and odd impedances, and from those into widths
and gaps by inverting Cohn's mapping of coupled strips. Where one resonator's half meets the
next half, the strip steps in width, aligned on the side away from the neighbour it leaves, so
the corners of strips that end and begin on one plane stay a gap apart.

The strips lie midway between the planes, so only the half below them is meshed, a magnetic
wall closing it. The reference is the ideal circuit: four open-circuited coupled sections in
cascade, each an exact chain matrix of its even and odd impedances, all at the speed of light in
the fill. What it leaves out — the steps in width, the open ends' fringing beside a neighbour
rather than alone, the box — shows as the band lying about 0.4 % low, its width within 0.2 %.

The mesh is coarse for the end sections' 0.18 mm gaps (the cells grow threefold away from the
strips' edges): one full solve has about 300 000 unknowns, the sweep of 25 points takes six.
With the cells growing by half again instead (830 000 unknowns), |S21| at the centre is 0.9997.
