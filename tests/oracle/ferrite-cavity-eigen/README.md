# Ferrite-filled cavity, eigenmode

The [ferrite-cavity](../ferrite-cavity/) box solved for its resonance directly. A magnetized
ferrite's permeability depends on the frequency, so the eigenproblem is nonlinear in it, and
its system is unsymmetric: `solve.eigenmodes` takes the modes of the tensor at a guess, then
follows each by solving again at its own frequency until it settles, each step by
shift-invert with a sparse LU. The resonance is the closed form's root of
k₀² εr μe(f) = (π/a)² + (π/d)², 1.8014 GHz.
