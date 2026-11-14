# A bunch through a closed pillbox: its loss factor

Particle-in-cell (`solve.particles`, docs/adr/0012-particles.md stage C): a 1 nC Gaussian bunch,
σ_z = 2 mm, of 10 GeV electrons enters a closed, perfectly conducting pillbox (radius 10 mm,
length 13.06 mm) through one end wall on its axis and leaves through the other. Its current is
the only source; once it has gone, the energy left in the cavity is q² Σ k_n e^{−(ω_n σ_t)²} over
the modes, k_n = |V_n|²/(4U_n) = ω_n (R/Q)_n /4 (Wilson 1989, Wangler §11.6): 5.805 V/pC, a little
more than TM010's share (3.54 V/pC times e^{−(ωσ_t)²} = 0.79) with TM011, TM020 and the rest.

The cylinder is a staircase of 0.25 mm cells (`set_metal`: a cell is metal when its centre lies
outside the solid); E on the edges touching it is held at zero. The run gives 5.810 V/pC, 0.07 %
high (0.17 % at 0.5 mm cells). The reference sums the TM0np modes with the Bessel functions taken
by Bessel's integral, apart from the code under test (`emcraft_theory::wake` gives the same).
