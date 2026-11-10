# Tolerance analysis of a lossy line

The [lossy-line](../lossy-line/) model: WR-90 filled with εr = 2.2, tan δ = 0.02, its ports in
the filling, so it is matched. Here its length is a variable, drawn 40 times uniformly within
30 ± 5 mm by `study.tolerance`. The line's loss in dB is linear in its length, so the closed
form gives the draws' mean (the loss at 30 mm, 0.903 dB), their spread (0.0301 dB/mm × 5/√3 =
0.0869 dB) and the share meeting |S21| ≥ −0.9 dB (the lines up to 29.9 mm: 49 %). The draws are
random, so each tolerance is three standard errors of the estimate from 40 draws; the seed
fixes them, so the fixture holds EMCraft's numbers exactly.
