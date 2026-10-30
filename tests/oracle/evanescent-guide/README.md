# A guide below cut-off

30 mm of WR-90 filled with air, solved at 5 and 6 GHz, below its TE10 cut-off (6.557 GHz). The
ports' first mode is evanescent: it goes as e^{−αz} with α = √((π/a)² − k0²) and carries no
power, so S is read in the pseudo-waves of Marks and Williams (1992) referred to the ports' own
mode. The guide is then matched (S11 = 0) and S21 = e^{−αL}, real: 23.2 dB of decay at 5 GHz,
14.4 dB at 6 GHz. The references were evaluated from that closed form separately from
EMCraft's code. The case checks that a sweep may start below a port's cut-off and shows the
decay there, as an experiment on evanescent waves wants.
