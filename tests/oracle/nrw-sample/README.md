# Material constants from S (Nicolson–Ross–Weir)

An 8 mm sample of εr = 2.2, tan δ = 0.01 fills WR-90 between 10 mm of empty guide on each
side. EMCraft solves S11 and S21 at the ports, and `results.material` moves them onto the
sample's faces, finds its reflection Γ and transmission T and from them εr and μr. The
reference is the material the model was given: 2.2 − 0.022j and μr = 1. Below a guide
wavelength in the sample (βd < π up to 12 GHz) the first branch of ln(1/T) is the right one.
