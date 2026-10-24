# Empty waveguide: TE10 transmission

A WR-90 waveguide (a = 22.86 mm, b = 10.16 mm), 30 mm long, empty, with TE10 ports on both
ends, solved with second-order Nédélec elements of about 3 mm at 8, 10 and 12 GHz (the TE10
cut-off is c/2a = 6.557 GHz, TE20's 13.11 GHz).

With nothing in the guide, the wave passes unreflected and only its phase turns:
S11 = 0 and S21 = exp(−jβL), β = √(k0² − (π/a)²). At 10 GHz β = 158.25 rad/m and
S21 = exp(−j·4.7476), a phase of 88.01°.
