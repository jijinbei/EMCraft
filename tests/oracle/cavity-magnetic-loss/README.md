# Cavity with magnetic loss

A 22.86 × 10.16 × 30 mm cavity with perfect walls, filled with μr = 2.25, tan δm = 1e-3. With
one material throughout, the curl–curl matrix is the lossless one divided by 1 − j tan δm, so
every eigenvalue k² is too, for the discrete problem as for the continuous one:
ω = ω0 (1 − j tan δm)^{−1/2} and Q_m = Re ω / (2 Im ω) = 1/(2 tan(½ atan tan δm)) = 1000.00025
(≈ 1/tan δm). The eigensolver runs in complex arithmetic and reads Q from the complex frequency,
not by perturbation; the TE101 resonance is the lossless one's, c/(2√(εr μr))√(1/a² + 1/d²),
shifted by Re (1 − j tan δm)^{−1/2}. The references were evaluated separately from EMCraft's
code.
