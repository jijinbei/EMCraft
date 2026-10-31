# Wall losses through a thin wall

The [wall-loss](../wall-loss/) guide with copper walls 0.5 µm thick, under the skin depth
(0.66 µm at 10 GHz), with free space behind them. The metal is not meshed: the wall is a lossy
line of length t loaded by η₀, so its surface impedance is
Z_s = η_c(η₀ + η_c tanh γt)/(η_c + η₀ tanh γt) with η_c = (1 + j)R_s, γ = (1 + j)/δ (Pozar,
eq. 2.91). Re Z_s = 0.0355 Ω against R_s = 0.0261 Ω for thick copper, so the guide loses
0.0295 dB instead of 0.0217 dB; the reference puts Re Z_s in place of R_s in the textbook TE10
attenuation (Pozar, eq. 3.96).
