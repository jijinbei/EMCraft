# Step from air into a lossy filling

WR-90: 12 mm of air, then 20 mm filled with a material lossy both ways (εr = 2, tan δ = 0.01,
μr = 1.3, tan δm = 0.02). Port 1 is in air, port 2 in the lossy filling. The exact solution is
a transmission-line step with the TE10 wave admittances Y = γ/μ: Γ = (Y1 − Y2)/(Y1 + Y2).

|S11| does not depend on how the lossy port's waves are normalised. S21 does: EMCraft reads S as
pseudo-waves referred to each port's own wave impedance (R. B. Marks, D. F. Williams, *A general
waveguide circuit theory*, J. Res. NIST 97(5):533–562, 1992), normalised by the modes'
unconjugated powers, so that S21 = S12 for this reciprocal structure; the reference applies the
same normalisation, √(Y2/Y1) on the principal branch, which for lossless ports is the usual
power-wave one. The references were evaluated separately from EMCraft's code.
