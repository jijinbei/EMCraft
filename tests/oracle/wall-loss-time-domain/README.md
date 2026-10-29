# Wall losses in the time domain

200 mm of empty WR-90 whose walls have a conductivity of 1e6 S/m, solved by the time-domain
solver (`solve.time_domain`) on 1 mm cells at 8, 10 and 12 GHz.

The walls take Leontovich's surface-impedance condition E_t = Z_s n̂ × H,
Z_s = (1 + j)√(ωμ0/2σ), the frequency-domain solver's. Z_s ∝ √(jω) has no rational form; the
time domain writes the admittance 1/Z_s = √(σ/μ0) (jω)^{−1/2} as a sum of some 25 first-order
branches of surface current (the trapezoidal rule on its Stieltjes integral, exponentially
accurate: within 2e-5 over the band, every branch passive), each advanced by the trapezoidal
rule on the walls' edges, which are updated by Ampère's law over the half of their dual faces
inside the guide.

The reference is the textbook perturbation result for TE10, α_c = R_s(2bπ² + a³k²)/(a³bβkη):
0.129, 0.095 and 0.086 Np/m, so the guide loses 0.22, 0.17 and 0.15 dB. A conductivity lower
than copper's makes the loss large enough to read against the transforms' settling (about 1e-4
in |S21|).
