# Units

The model, the mesh, the solvers and the results are in SI: metres, hertz, V/m, A/m. Time
dependence is e^{jωt}.

Command parameters and replies are in the session's **display units**, millimetres and
gigahertz by default (`units.set` changes them: `m` or `mm`; `GHz`, `MHz` or `Hz`). Conversions
happen only at that boundary, through `crates/model/src/units.rs` (`Length`, `Frequency`); no
conversion factor is written anywhere else. Physical constants (c₀, μ₀, ε₀, η₀) live there too.

S-parameters are power waves normalised to each port's TE₁₀ wave impedance, so |S| is
unitary for a lossless structure; their phase is referred to the port planes. Fields are for a
unit (1 V/m peak) incident TE₁₀ wave, e = ŷ sin(πx/a).
