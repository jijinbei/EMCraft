# Lossy stripline of two plies

A stripline built as a board is two laminates: a core under the strip and a prepreg over it. Here
both are FR-4-like (εr = 4.4, tan δ = 0.02) but separate materials, so each port's cross-section
holds two materials, one of them lossy, and its modes come from the hybrid lossy mode solver
(complex εr per triangle, complex γ). With one dielectric in effect the line still carries an
exact TEM wave, γ = jk0 √(εr(1 − j tan δ)), whatever the box and strip: the port mode's
attenuation and −Re γ²/k0² are checked against it, and the 6 mm line's S21 against e^{−γL}. The
references were evaluated from that closed form separately from EMCraft's code.
