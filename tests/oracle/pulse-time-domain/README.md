# A chirped pulse through a WR-90 guide, from its S-parameters and in the time domain

500 mm of WR-90 (22.86 × 10.16 mm, TE10 cut off at 6.557 GHz) carrying a Gaussian pulse 2 ns
wide at half power at 8 GHz, its frequency rising 0.8 GHz over its width. Near cut-off the guide
disperses (the higher frequencies travel faster), so the rising chirp comes out shorter and
taller.

- `results.pulse` makes the pulse from the FEM sweep's S21: its compression, output width and
  group delay against the exact transform of the same pulse through e^{−jβL},
  β = √(k² − (π/a)²), evaluated independently by numerical Fourier integrals (32 768 points over
  ±2.5 GHz, the output sampled every picosecond).
- `solve.time_domain.pulse` launches the chirped pulse into the time-domain grid (1 mm cells) and
  measures it at port 2. The soft source shapes the launched spectrum (its wave impedance), so
  the incident wave is not the ideal Gaussian; the check is that the FEM's S21 applied to the
  measured incident wave gives the time domain's own output: the two compressions' ratio
  (`agreement`) is 1.
