# Noise figure of a lossy line

The [lossy-line](../lossy-line/) model: 30 mm of WR-90 filled with εr = 2.2, tan δ = 0.02,
its ports in the filling, so the line is matched. A passive network in thermal equilibrium
at temperature T adds the noise Te = (L − 1)T, so at T₀ = 290 K its noise figure is its loss:
0.903 dB at 10 GHz. Cooled to 77 K the same line's figure falls to
1 + (77/290)(L − 1), 0.259 dB. `results.noise` reads it from the solved S alone.
