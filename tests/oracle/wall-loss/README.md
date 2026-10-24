# Wall losses

200 mm of empty WR-90 with copper walls (σ = 5.8e7 S/m) at 10 GHz. The walls are modelled by the
Leontovich surface-impedance condition E_t = Z_s n̂ × H, Z_s = (1 + j)√(ωμ0/2σ) (skin depth
0.66 µm). The reference is the textbook perturbation result for TE10, α_c = 0.01248 Np/m
(0.108 dB/m), so the guide loses 0.0217 dB. The solver's tests also check that
1 − |S11|² − |S21|² equals the power ∫|E_t|²/(4R_s) dS the walls absorb, to 1e-9.
