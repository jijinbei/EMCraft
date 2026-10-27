# De-embedding

An empty WR-90 guide 30 mm long, solved once with both ports' reference planes moved 10 mm
into the guide (`deembed: 10`, leaving 10 mm of line between them) and once moved 5 mm out of
it (`deembed: -5`, as if 5 mm of line were added at each end). A lossless matched line's S21
is e^{−jβℓ} for the length ℓ between the planes, so the references are the TE10 phase over 10
and 40 mm, with β = √(k0² − (π/a)²) evaluated independently of the solver. `results.compare`
moves the exact line's S to the same planes.
