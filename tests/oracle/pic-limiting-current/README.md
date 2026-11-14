# A ring beam in a drift tube: space-charge depression and the limiting current

Particle-in-cell (`solve.particles`, docs/adr/0012-particles.md stage D): a steady ring of
electrons (`particles.beam`: 5.5 to 6.5 mm in radius) is injected at 127.75 keV (γ₀ = 1.25)
through the grounded end of a perfectly conducting tube 10 mm in radius and 80 mm long, held on
its lines by a 10 T axial field. Its own charge sinks its potential: the electrons slow to γ_b
with I = I₀(γ₀ − γ_b)β_b/(2 ln(R/r_b)) (Bogdankevich & Rukhadze 1971; Miller 1982, §3.2). At
750 A, 0.7 of the limiting current I_L = I₀(γ₀^{2/3} − 1)^{3/2}/(2 ln(R/r_b)) = 1072 A, all of it
passes and γ_b = 1.1617 in the middle of the tube (1.1593 here: the ring a cell thick). At
1400 A a virtual cathode forms, 367 A goes back to the cathode and 1045 A, about the limit,
passes.

At γ₀ = 1.25 the electrons (β = 0.6) are slower than the Yee grid's shortest waves (0.67c at
the time step here). Faster beams radiate into those waves — numerical Cherenkov radiation —
which deepens their depression (by 16 % at γ₀ = 1.5, 25 % at 2); a solver for them is ADR 0012's
stage E's to add.
