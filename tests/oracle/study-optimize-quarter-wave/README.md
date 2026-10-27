# Optimization of a quarter-wave matching section

An empty WR-90 guide (port 1) and one filled with εr = 2.2 (port 2), joined by a section of
εr_m = 1.380001…, whose length is a variable `l`. At 8 GHz that εr_m makes the section's TE10
propagation constant the geometric mean of the two guides', β_m = √(β₁β₃), so its wave
impedance ωμ/β_m is the geometric mean of theirs and the reflection vanishes exactly where the
section is a quarter of its guide wavelength long: l = π/(2β_m) = 11.1326 mm (εr_m and λg/4
evaluated separately from the closed form). `study.optimize` minimizes |S11| in dB at 8 GHz
over l in [4, 18] mm by the Nelder–Mead simplex from l = 8 mm, each point the model rebuilt,
meshed and solved; it must land on λg/4 and leave the model there (the last step solves the
model it left).
