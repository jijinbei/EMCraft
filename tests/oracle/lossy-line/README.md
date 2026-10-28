# Lossy line, ports in the lossy medium

30 mm of WR-90 filled throughout with a lossy dielectric (εr = 2.2, tan δ = 0.02), its two ports
in the filling. The ports' TE10 has the complex propagation constant
γ = √((π/a)² − k0² εr(1 − j tan δ)) = α + jβ, so the line is matched (S11 = 0) and
S21 = e^{−γL}: |S21| = e^{−αL} (0.78–1.05 dB) and arg S21 = −βL. The references were evaluated
from that closed form separately from EMCraft's code. The case checks that the port condition
takes the complex γ and κ of the lossy medium (a lossless port mode would reflect at the ports)
and that S is read with the right normalisation for a lossy line.
