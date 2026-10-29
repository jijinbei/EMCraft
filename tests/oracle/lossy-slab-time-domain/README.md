# Lossy dielectric slab in the time domain

A 20 mm slab of a lossy dielectric (εr = 2.2, tan δ = 0.05) filling a WR-90 guide between
10 mm of empty guide on each side, solved by the time-domain solver (`solve.time_domain`) on
1 mm cells at 8, 10 and 12 GHz.

A constant loss tangent has no causal, finite time-domain form, so the time domain realises it
as a conductivity matched at the centre of the run's band (10 GHz here, the reply's
`loss_reference`): σ = 2π f_r ε0 εr tan δ, and the material it solves is
ε(ω) = εr − jσ/(ωε0) = 2.2 (1 − j 0.05 · 10 GHz/f). The reference is the TE10 cascade for exactly
that material, the complex propagation constant β = √(k0² ε(ω) − (π/a)²) in the slab, evaluated
with ABCD matrices separately from EMCraft's code. (The frequency-domain case `lossy-slab`
solves tan δ constant over frequency; the engine's tests check that the FEM given tan δ·f_r/f at
each frequency agrees with the time domain.)

The conduction current is averaged over the time step with the exponential update; the slab
absorbs about 30 % of the incident power.
