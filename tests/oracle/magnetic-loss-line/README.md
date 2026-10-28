# Magnetically lossy line

30 mm of WR-90 filled with a magnetically lossy material (εr = 1.5, μr = 1.6, tan δm = 0.03:
μ = μr(1 − j tan δm)), its two ports in it. The loss enters the finite elements through the
curl–curl term, μ⁻¹ complex, and the ports through k² = k0² εr μr(1 − j tan δm) and the factor
κ/μ. The exact solution is S11 = 0, S21 = e^{−γL} with γ = √((π/a)² − k²); the references were
evaluated from it separately from EMCraft's code. The solver's tests also check the discrete
Poynting theorem with magnetic loss: 1 − |S11|² − |S21|² equals the power ½ωμ₀∫μ″|H|² a lossy
slab absorbs, to 1e-9.
